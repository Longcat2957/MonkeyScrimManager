"""Persistent dummy-user reads and atomic creation."""

import json
import uuid
from datetime import datetime, timedelta
from typing import Any, cast

from sqlalchemy import text
from sqlalchemy.exc import IntegrityError
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.riot_identity import normalize_riot_id
from monkeybot_api.core.time import timestamp
from monkeybot_api.schemas.dev.users import LANES, CreateMembership, CreateRiotAccount, CreateUser

BASE_SELECT = """
    SELECT person.user_id, person.display_name, profile.stable_key,
           profile.avatar_seed, profile.notes, profile.state, profile.revision,
           COALESCE(privacy.public_profile_enabled, false) AS public_profile_enabled,
           COALESCE(privacy.show_riot_ids, false) AS show_riot_ids,
           COALESCE(privacy.allow_external_record_search, false)
             AS allow_external_record_search,
           profile.created_at, profile.updated_at
    FROM dummy_user_profiles AS profile
    JOIN users AS person ON person.user_id=profile.user_id
      AND person.identity_kind='DUMMY'
    LEFT JOIN user_privacy_settings AS privacy ON privacy.user_id=profile.user_id
"""

LIST_WHERE = """
    WHERE (CAST(:state AS text) IS NULL OR profile.state=CAST(:state AS text))
      AND (CAST(:query AS text) IS NULL OR profile.stable_key ILIKE '%' || :query || '%'
           OR person.display_name ILIKE '%' || :query || '%')
      AND (CAST(:guild AS text) IS NULL OR EXISTS (
          SELECT 1 FROM guild_members AS member
          WHERE member.user_id=profile.user_id AND member.guild_id=CAST(:guild AS text)))
      AND (CAST(:group AS uuid) IS NULL OR EXISTS (
          SELECT 1 FROM dummy_user_group_members AS grouped
          WHERE grouped.user_id=profile.user_id AND grouped.group_id=CAST(:group AS uuid)))
"""


def missing() -> ApiError:
    return ApiError(404, "dummy_user_not_found", "더미 사용자를 찾을 수 없습니다.")


def invariant() -> ApiError:
    return ApiError(
        500,
        "dummy_user_invariant_failed",
        "저장된 더미 사용자 데이터의 일관성을 확인할 수 없습니다.",
    )


async def list_base(
    c: AsyncConnection, filters: dict[str, Any]
) -> tuple[list[dict[str, Any]], int]:
    rows = (
        (
            await c.execute(
                text(
                    BASE_SELECT
                    + LIST_WHERE
                    + " ORDER BY profile.updated_at DESC, profile.user_id"
                    + " LIMIT :limit OFFSET :offset"
                ),
                filters,
            )
        )
        .mappings()
        .all()
    )
    total = await c.scalar(
        text(
            "SELECT count(*) FROM dummy_user_profiles AS profile "
            "JOIN users AS person ON person.user_id=profile.user_id " + LIST_WHERE
        ),
        filters,
    )
    return [dict(row) for row in rows], cast(int, total)


async def get_base(c: AsyncConnection, user: str) -> dict[str, Any]:
    row = (
        (await c.execute(text(BASE_SELECT + " WHERE profile.user_id=:user"), {"user": user}))
        .mappings()
        .one_or_none()
    )
    if row is None:
        raise missing()
    return dict(row)


async def guild_catalog(c: AsyncConnection) -> list[dict[str, Any]]:
    rows = (
        (
            await c.execute(
                text("""
                SELECT guild.guild_id,
                       COALESCE(config.display_name, guild.discord_name) AS display_name,
                       guild.status, season.id AS active_season_id
                FROM guilds AS guild
                LEFT JOIN guild_config AS config ON config.guild_id=guild.guild_id
                LEFT JOIN seasons AS season ON season.guild_id=guild.guild_id
                  AND season.status='ACTIVE'
                WHERE guild.status <> 'LEFT'
                ORDER BY display_name, guild.guild_id
            """)
            )
        )
        .mappings()
        .all()
    )
    return [
        {
            "guildId": row["guild_id"],
            "displayName": row["display_name"],
            "status": row["status"],
            "activeSeasonId": str(row["active_season_id"])
            if row["active_season_id"] is not None
            else None,
        }
        for row in rows
    ]


async def snapshots(c: AsyncConnection, base: list[dict[str, Any]]) -> list[dict[str, Any]]:
    if not base:
        return []
    ids = [row["user_id"] for row in base]
    params = {"ids": ids}
    riot_rows = (
        (
            await c.execute(
                text("""
                SELECT user_id, platform, routing_region, game_name, tag_line,
                       profile_icon_id, main_position
                FROM riot_accounts
                WHERE user_id=ANY(CAST(:ids AS text[])) AND origin='DUMMY'
            """),
                params,
            )
        )
        .mappings()
        .all()
    )
    riot_by_user: dict[str, dict[str, Any]] = {}
    for row in riot_rows:
        user = row["user_id"]
        if user in riot_by_user:
            raise invariant()
        riot_by_user[user] = {
            "platform": row["platform"],
            "routingRegion": row["routing_region"],
            "gameName": row["game_name"],
            "tagLine": row["tag_line"],
            "profileIconId": row["profile_icon_id"],
            "mainPosition": row["main_position"],
        }
    pref_rows = (
        (
            await c.execute(
                text("""
                SELECT user_id, lane, position, champion_id
                FROM user_champion_preferences
                WHERE user_id=ANY(CAST(:ids AS text[]))
                ORDER BY user_id,
                  CASE lane WHEN 'TOP' THEN 1 WHEN 'JUNGLE' THEN 2 WHEN 'MID' THEN 3
                            WHEN 'BOTTOM' THEN 4 WHEN 'SUPPORT' THEN 5 END, position
            """),
                params,
            )
        )
        .mappings()
        .all()
    )
    preferences: dict[str, dict[str, list[str]]] = {}
    for row in pref_rows:
        user, lane = row["user_id"], row["lane"]
        if lane not in LANES:
            raise invariant()
        values = preferences.setdefault(user, {name: [] for name in LANES})[lane]
        if row["position"] != len(values) + 1:
            raise invariant()
        values.append(row["champion_id"])
    member_rows = (
        (
            await c.execute(
                text("""
                SELECT member.user_id, member.guild_id,
                       COALESCE(config.display_name, guild.discord_name) AS guild_name,
                       member.display_name, member.status, member.main_lane,
                       season.id AS active_season_id
                FROM guild_members AS member
                JOIN guilds AS guild ON guild.guild_id=member.guild_id
                LEFT JOIN guild_config AS config ON config.guild_id=member.guild_id
                LEFT JOIN seasons AS season ON season.guild_id=member.guild_id
                  AND season.status='ACTIVE'
                WHERE member.user_id=ANY(CAST(:ids AS text[]))
                ORDER BY member.user_id, guild_name, member.guild_id
            """),
                params,
            )
        )
        .mappings()
        .all()
    )
    mmr_rows = (
        (
            await c.execute(
                text("""
                SELECT mmr.user_id, mmr.guild_id, mmr.lane, mmr.mmr
                FROM user_lane_mmr AS mmr
                JOIN seasons AS season ON season.guild_id=mmr.guild_id
                  AND season.id=mmr.season_id AND season.status='ACTIVE'
                WHERE mmr.user_id=ANY(CAST(:ids AS text[]))
                ORDER BY mmr.user_id, mmr.guild_id, mmr.lane
            """),
                params,
            )
        )
        .mappings()
        .all()
    )
    mmr_by_member: dict[tuple[str, str], dict[str, float]] = {}
    for row in mmr_rows:
        key = (row["user_id"], row["guild_id"])
        lane = row["lane"]
        mmr_values = mmr_by_member.setdefault(key, {})
        if lane not in LANES or lane in mmr_values:
            raise invariant()
        mmr_values[lane] = row["mmr"]
    members_by_user: dict[str, list[dict[str, Any]]] = {}
    for row in member_rows:
        mmr = mmr_by_member.pop((row["user_id"], row["guild_id"]), {})
        if mmr and set(mmr) != set(LANES):
            raise invariant()
        lane_mmr = {lane.lower(): mmr[lane] for lane in LANES} if mmr else None
        members_by_user.setdefault(row["user_id"], []).append(
            {
                "guildId": row["guild_id"],
                "guildName": row["guild_name"],
                "displayName": row["display_name"],
                "status": row["status"],
                "mainLane": row["main_lane"],
                "laneMmr": lane_mmr,
            }
        )
    if mmr_by_member:
        raise invariant()
    group_rows = (
        (
            await c.execute(
                text("""
                SELECT user_id, group_id FROM dummy_user_group_members
                WHERE user_id=ANY(CAST(:ids AS text[])) ORDER BY user_id, group_id
            """),
                params,
            )
        )
        .mappings()
        .all()
    )
    groups_by_user: dict[str, list[str]] = {}
    for row in group_rows:
        groups_by_user.setdefault(row["user_id"], []).append(str(row["group_id"]))
    return [
        {
            "userId": row["user_id"],
            "identityKind": "DUMMY",
            "stableKey": row["stable_key"],
            "displayName": row["display_name"],
            "avatarSeed": row["avatar_seed"],
            "notes": row["notes"],
            "state": row["state"],
            "revision": row["revision"],
            "privacy": {
                "publicProfileEnabled": row["public_profile_enabled"],
                "showRiotIds": row["show_riot_ids"],
                "allowExternalRecordSearch": row["allow_external_record_search"],
            },
            "riotAccount": riot_by_user.get(row["user_id"]),
            "championPreferences": preferences.get(row["user_id"], {name: [] for name in LANES}),
            "guildMemberships": members_by_user.get(row["user_id"], []),
            "groupIds": groups_by_user.get(row["user_id"], []),
            "createdAt": timestamp(row["created_at"]),
            "updatedAt": timestamp(row["updated_at"]),
        }
        for row in base
    ]


async def acquire_create(
    c: AsyncConnection, actor: str, key: str, route: str, request_hash: bytes, now: datetime
) -> tuple[uuid.UUID | None, dict[str, Any] | None]:
    params = {
        "actor": actor,
        "key": key,
        "route": route,
        "hash": request_hash,
        "now": now,
        "expires": now + timedelta(hours=24),
        "record": uuid.uuid7(),
    }
    locked = await c.scalar(
        text("""
        SELECT pg_try_advisory_xact_lock(hashtextextended(jsonb_build_array(
            'monkeybot:idempotency:v1', 'GLOBAL', 'USER', CAST(:actor AS text),
            'POST', CAST(:route AS text), CAST(:key AS text))::text, 0))
    """),
        params,
    )
    if not locked:
        raise ApiError(409, "idempotency_in_progress", "같은 요청을 처리하고 있습니다.")
    await c.execute(
        text("""
        DELETE FROM idempotency_records
        WHERE scope_key='GLOBAL' AND guild_id IS NULL AND principal_type='USER'
          AND principal_id=:actor AND method='POST' AND route_template=:route
          AND idempotency_key=:key AND expires_at<=:now
    """),
        params,
    )
    inserted = await c.scalar(
        text("""
        INSERT INTO idempotency_records
          (id, guild_id, scope_key, principal_type, principal_id, method, route_template,
           idempotency_key, request_hash, state, created_at, expires_at)
        VALUES (:record, NULL, 'GLOBAL', 'USER', :actor, 'POST', :route,
                :key, :hash, 'IN_PROGRESS', :now, :expires)
        ON CONFLICT ON CONSTRAINT idempotency_records_command_key DO NOTHING RETURNING id
    """),
        params,
    )
    if inserted is not None:
        return cast(uuid.UUID, inserted), None
    existing = (
        (
            await c.execute(
                text("""
                SELECT request_hash, state, status_code, response_body
                FROM idempotency_records
                WHERE scope_key='GLOBAL' AND guild_id IS NULL AND principal_type='USER'
                  AND principal_id=:actor AND method='POST' AND route_template=:route
                  AND idempotency_key=:key FOR UPDATE
            """),
                params,
            )
        )
        .mappings()
        .one_or_none()
    )
    if existing is None:
        raise invariant()
    if bytes(existing["request_hash"]) != request_hash:
        raise ApiError(
            409, "idempotency_conflict", "같은 Idempotency-Key가 다른 요청에 사용되었습니다."
        )
    if existing["state"] == "IN_PROGRESS":
        raise ApiError(409, "idempotency_in_progress", "같은 요청을 처리하고 있습니다.")
    if existing["state"] != "COMPLETED" or existing["status_code"] != 201:
        raise invariant()
    body = existing["response_body"]
    if not isinstance(body, dict):
        raise invariant()
    return None, body


async def complete_create(
    c: AsyncConnection, record: uuid.UUID, body: dict[str, Any], now: datetime
) -> None:
    completed = await c.scalar(
        text("""
        UPDATE idempotency_records SET state='COMPLETED', status_code=201,
            response_body=CAST(:body AS jsonb), completed_at=:now
        WHERE id=:record AND guild_id IS NULL AND scope_key='GLOBAL'
          AND state='IN_PROGRESS' RETURNING id
    """),
        {"record": record, "body": json.dumps(body, ensure_ascii=False), "now": now},
    )
    if completed is None:
        raise invariant()


async def create_one(
    c: AsyncConnection, command: CreateUser, actor: str, request_id: str, now: datetime
) -> str:
    user = f"dmy_{uuid.uuid7()}"
    await c.execute(
        text("""
        INSERT INTO users
          (user_id, identity_kind, display_name, avatar_hash, created_at, updated_at, deleted_at)
        VALUES (:user, 'DUMMY', :name, NULL, :now, :now, NULL)
    """),
        {"user": user, "name": command.displayName, "now": now},
    )
    try:
        await c.execute(
            text("""
            INSERT INTO dummy_user_profiles
              (user_id, stable_key, state, avatar_seed, notes, revision,
               created_by, updated_by, created_at, updated_at)
            VALUES (:user, :stable, 'ACTIVE', :avatar, :notes, 0,
                    :actor, :actor, :now, :now)
        """),
            {
                "user": user,
                "stable": command.stableKey,
                "avatar": command.avatarSeed,
                "notes": command.notes or None,
                "actor": actor,
                "now": now,
            },
        )
    except IntegrityError as error:
        if getattr(error.orig, "constraint_name", None) == "dummy_user_profiles_stable_key_key":
            raise ApiError(
                409, "dummy_stable_key_conflict", "이미 사용 중인 더미 사용자 식별 키입니다."
            ) from None
        raise
    privacy = command.privacy
    await c.execute(
        text("""
        INSERT INTO user_privacy_settings
          (user_id, public_profile_enabled, show_riot_ids,
           allow_external_record_search, updated_at)
        VALUES (:user, :public, :show_riot, :external, :now)
    """),
        {
            "user": user,
            "public": privacy.publicProfileEnabled,
            "show_riot": privacy.showRiotIds,
            "external": privacy.allowExternalRecordSearch,
            "now": now,
        },
    )
    await c.execute(
        text("""
        INSERT INTO user_guild_preferences (user_id, default_guild_id, updated_at)
        VALUES (:user, NULL, :now)
    """),
        {"user": user, "now": now},
    )
    for membership in command.guildMemberships:
        await attach_membership(c, user, actor, request_id, membership, now)
    if command.riotAccount is not None:
        await attach_riot(c, user, command.riotAccount, now)
    await audit_dummy(
        c,
        actor,
        "DUMMY_USER_CREATED",
        user,
        request_id,
        {
            "stableKey": command.stableKey,
            "displayName": command.displayName,
            "privacy": privacy.model_dump(),
        },
        now,
    )
    return user


async def attach_membership(
    c: AsyncConnection,
    user: str,
    actor: str,
    request_id: str,
    membership: CreateMembership,
    now: datetime,
) -> None:
    guild = membership.guildId
    exists = await c.scalar(
        text("SELECT true FROM guilds WHERE guild_id=:guild AND status<>'LEFT' FOR SHARE"),
        {"guild": guild},
    )
    if not exists:
        raise ApiError(404, "guild_not_found", "배치할 길드를 찾을 수 없습니다.")
    await c.execute(
        text("""
        INSERT INTO guild_members
          (guild_id, user_id, display_name, status, main_lane,
           first_seen_at, registered_at, discord_joined_at, last_active_at,
           last_synced_at, inactivated_at)
        VALUES (:guild, :user, :name, :status, :lane,
                :now, :now, NULL, NULL, NULL,
                CASE WHEN CAST(:status AS text)='INACTIVE'
                     THEN CAST(:now AS timestamptz) ELSE NULL END)
    """),
        {
            "guild": guild,
            "user": user,
            "name": membership.displayName,
            "status": membership.status,
            "lane": membership.mainLane,
            "now": now,
        },
    )
    if membership.laneMmr is not None:
        season = await c.scalar(
            text("""
            SELECT id FROM seasons WHERE guild_id=:guild AND status='ACTIVE' FOR SHARE
        """),
            {"guild": guild},
        )
        if season is None:
            raise ApiError(
                409,
                "active_season_required",
                "초기 MMR을 저장하려면 길드의 활성 시즌이 필요합니다.",
            )
        mmr = membership.laneMmr.model_dump()
        for lane in LANES:
            await c.execute(
                text("""
                INSERT INTO user_lane_mmr
                  (guild_id, season_id, user_id, lane, mmr,
                   games_played, wins, revision, updated_at)
                VALUES (:guild, :season, :user, :lane, :mmr, 0, 0, 0, :now)
            """),
                {
                    "guild": guild,
                    "season": season,
                    "user": user,
                    "lane": lane,
                    "mmr": mmr[lane.lower()],
                    "now": now,
                },
            )
    await c.execute(
        text("""
        INSERT INTO guild_admin_state (guild_id, scope, revision, updated_by, updated_at)
        VALUES (:guild, 'MEMBERS', 1, NULL, :now)
        ON CONFLICT (guild_id, scope) DO UPDATE
        SET revision=guild_admin_state.revision+1,
            updated_by=NULL, updated_at=EXCLUDED.updated_at
    """),
        {"guild": guild, "now": now},
    )
    await c.execute(
        text("""
        INSERT INTO admin_audit_log
          (guild_id, id, principal_type, principal_id, action, target_type,
           target_id, checkpoint_id, request_id, before_summary, after_summary,
           result, note, created_at)
        VALUES (:guild, :id, 'USER', :actor, 'DUMMY_MEMBER_ATTACHED',
                'GUILD_MEMBER', :user, NULL, :request_id, NULL,
                CAST(:after AS jsonb), 'APPLIED', NULL, :now)
    """),
        {
            "guild": guild,
            "id": uuid.uuid7(),
            "actor": actor,
            "user": user,
            "request_id": request_id,
            "after": json.dumps(
                {
                    "displayName": membership.displayName,
                    "status": membership.status,
                    "mainLane": membership.mainLane,
                    "initialMmrProvided": membership.laneMmr is not None,
                },
                ensure_ascii=False,
            ),
            "now": now,
        },
    )


async def attach_riot(
    c: AsyncConnection, user: str, account: CreateRiotAccount, now: datetime
) -> None:
    await c.execute(
        text("""
        INSERT INTO riot_accounts
          (puuid, user_id, platform, routing_region, game_name, tag_line,
           normalized_game_name, normalized_tag_line, profile_icon_id, is_main,
           main_position, main_position_updated_at, last_synced_at,
           created_at, updated_at, origin)
        VALUES (:puuid, :user, :platform, :region, :game, :tag,
                :normalized_game, :normalized_tag, :icon, true,
                :position, CASE WHEN CAST(:position AS text) IS NULL
                                THEN NULL ELSE CAST(:now AS timestamptz) END,
                NULL, :now, :now, 'DUMMY')
    """),
        {
            "puuid": f"dummy:{uuid.uuid7()}",
            "user": user,
            "platform": account.platform,
            "region": account.routingRegion,
            "game": account.gameName,
            "tag": account.tagLine,
            "normalized_game": normalize_riot_id(account.gameName),
            "normalized_tag": normalize_riot_id(account.tagLine),
            "icon": account.profileIconId,
            "position": account.mainPosition,
            "now": now,
        },
    )


async def audit_dummy(
    c: AsyncConnection,
    actor: str,
    action: str,
    user: str,
    request_id: str,
    after: dict[str, Any],
    now: datetime,
) -> None:
    await c.execute(
        text("""
        INSERT INTO dummy_user_audit_log
          (id, actor_user_id, action, target_type, target_id,
           request_id, before_summary, after_summary, created_at)
        VALUES (:id, :actor, :action, 'DUMMY_USER', :user,
                :request_id, NULL, CAST(:after AS jsonb), :now)
    """),
        {
            "id": uuid.uuid7(),
            "actor": actor,
            "action": action,
            "user": user,
            "request_id": request_id,
            "after": json.dumps(after, ensure_ascii=False),
            "now": now,
        },
    )
