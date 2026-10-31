"""Transactional persistence for dummy-user membership and profile links."""

import json
import uuid
from typing import Any

from sqlalchemy import text
from sqlalchemy.exc import IntegrityError
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.riot_identity import normalize_riot_id
from monkeybot_api.repositories.dev import users
from monkeybot_api.schemas.dev.users import LANES


def missing_membership() -> ApiError:
    return ApiError(404, "dummy_membership_not_found", "해당 길드의 더미 회원을 찾을 수 없습니다.")


def history_conflict() -> ApiError:
    return ApiError(
        409,
        "dummy_membership_has_history",
        "내전 이력이 있는 길드 소속은 제거할 수 없습니다. 비활성 상태로 변경해 주세요.",
    )


async def acquire(c: AsyncConnection, p: dict[str, Any]) -> dict[str, Any] | None:
    locked = await c.scalar(
        text("""
        SELECT pg_try_advisory_xact_lock(hashtextextended(jsonb_build_array(
          'monkeybot:idempotency:v1', CAST(:scope AS text), 'USER', CAST(:actor AS text),
          CAST(:method AS text), CAST(:route AS text), CAST(:key AS text)
        )::text, 0))
        """),
        p,
    )
    if not locked:
        raise ApiError(409, "idempotency_in_progress", "같은 요청을 처리하고 있습니다.")
    where = """
        scope_key=:scope AND guild_id IS NOT DISTINCT FROM CAST(:guild AS text)
        AND principal_type='USER' AND principal_id=:actor AND method=:method
        AND route_template=:route AND idempotency_key=:key
    """
    await c.execute(text(f"DELETE FROM idempotency_records WHERE {where} AND expires_at<=:now"), p)
    inserted = await c.scalar(
        text("""
        INSERT INTO idempotency_records
          (id, guild_id, scope_key, principal_type, principal_id, method, route_template,
           idempotency_key, request_hash, state, created_at, expires_at)
        VALUES (:idem_id, :guild, :scope, 'USER', :actor, :method, :route,
                :key, :hash, 'IN_PROGRESS', :now, :expires)
        ON CONFLICT ON CONSTRAINT idempotency_records_command_key DO NOTHING RETURNING id
        """),
        p,
    )
    if inserted is not None:
        return None
    row = (
        (
            await c.execute(
                text(f"""
                SELECT request_hash, state, status_code, response_body
                FROM idempotency_records WHERE {where} FOR UPDATE
                """),
                p,
            )
        )
        .mappings()
        .one_or_none()
    )
    if row is None:
        raise users.invariant()
    if bytes(row["request_hash"]) != p["hash"]:
        raise ApiError(
            409, "idempotency_conflict", "같은 Idempotency-Key가 다른 요청에 사용되었습니다."
        )
    if row["state"] == "IN_PROGRESS":
        raise ApiError(409, "idempotency_in_progress", "같은 요청을 처리하고 있습니다.")
    if row["state"] != "COMPLETED" or row["status_code"] != 200:
        raise users.invariant()
    body = row["response_body"]
    if not isinstance(body, dict):
        raise users.invariant()
    return body


async def complete(c: AsyncConnection, p: dict[str, Any], user: dict[str, Any]) -> None:
    changed = await c.scalar(
        text("""
        UPDATE idempotency_records SET state='COMPLETED', status_code=200,
          response_body=CAST(:body AS jsonb), completed_at=:now
        WHERE id=:idem_id AND scope_key=:scope
          AND guild_id IS NOT DISTINCT FROM CAST(:guild AS text)
          AND state='IN_PROGRESS' RETURNING id
        """),
        {**p, "body": json.dumps({"user": user}, ensure_ascii=False)},
    )
    if changed is None:
        raise users.invariant()


async def lock_profile(c: AsyncConnection, user: str) -> str:
    row = (
        (
            await c.execute(
                text("""
                SELECT profile.state FROM dummy_user_profiles AS profile
                JOIN users AS person ON person.user_id=profile.user_id
                  AND person.identity_kind='DUMMY'
                WHERE profile.user_id=:user FOR UPDATE OF profile, person
                """),
                {"user": user},
            )
        )
        .mappings()
        .one_or_none()
    )
    if row is None:
        raise users.missing()
    return str(row["state"])


async def snapshot(c: AsyncConnection, user: str) -> dict[str, Any]:
    return (await users.snapshots(c, [await users.get_base(c, user)]))[0]


async def guild_exists(c: AsyncConnection, guild: str) -> None:
    exists = await c.scalar(
        text("SELECT true FROM guilds WHERE guild_id=:guild AND status <> 'LEFT' FOR SHARE"),
        {"guild": guild},
    )
    if not exists:
        raise ApiError(404, "guild_not_found", "배치할 길드를 찾을 수 없습니다.")


async def previous_membership(c: AsyncConnection, user: str, guild: str) -> dict[str, Any] | None:
    row = (
        (
            await c.execute(
                text("""
                SELECT display_name, status, main_lane FROM guild_members
                WHERE guild_id=:guild AND user_id=:user FOR UPDATE
                """),
                {"guild": guild, "user": user},
            )
        )
        .mappings()
        .one_or_none()
    )
    if row is None:
        return None
    return {
        "displayName": row["display_name"],
        "status": row["status"],
        "mainLane": row["main_lane"],
    }


async def upsert_membership(c: AsyncConnection, p: dict[str, Any]) -> None:
    await c.execute(
        text("""
        INSERT INTO guild_members
          (guild_id, user_id, display_name, status, main_lane,
           first_seen_at, registered_at, discord_joined_at, last_active_at,
           last_synced_at, inactivated_at)
        VALUES (:guild, :user, :name, :status, :lane,
                CAST(:now AS timestamptz), CAST(:now AS timestamptz), NULL, NULL, NULL,
                CASE WHEN :status='INACTIVE' THEN CAST(:now AS timestamptz) ELSE NULL END)
        ON CONFLICT (guild_id, user_id) DO UPDATE
        SET display_name=EXCLUDED.display_name, status=EXCLUDED.status,
            main_lane=EXCLUDED.main_lane,
            registered_at=COALESCE(guild_members.registered_at, EXCLUDED.registered_at),
            inactivated_at=CASE WHEN EXCLUDED.status='INACTIVE'
                                THEN EXCLUDED.inactivated_at ELSE NULL END
        """),
        p,
    )


async def upsert_lane_mmr(c: AsyncConnection, p: dict[str, Any], mmr: dict[str, float]) -> None:
    season = await c.scalar(
        text("SELECT id FROM seasons WHERE guild_id=:guild AND status='ACTIVE' FOR SHARE"), p
    )
    if season is None:
        raise ApiError(
            409, "active_season_required", "초기 MMR을 저장하려면 길드의 활성 시즌이 필요합니다."
        )
    for lane in LANES:
        await c.execute(
            text("""
            INSERT INTO user_lane_mmr
              (guild_id, season_id, user_id, lane, mmr,
               games_played, wins, revision, updated_at)
            VALUES (:guild, :season, :user, :lane_name, :mmr, 0, 0, 0, :now)
            ON CONFLICT (guild_id, season_id, user_id, lane) DO UPDATE
            SET mmr=EXCLUDED.mmr, revision=user_lane_mmr.revision+1,
                updated_at=EXCLUDED.updated_at
            """),
            {**p, "season": season, "lane_name": lane, "mmr": mmr[lane.lower()]},
        )


async def has_membership_history(c: AsyncConnection, p: dict[str, Any]) -> bool:
    return bool(
        await c.scalar(
            text("""
        SELECT EXISTS (
          SELECT 1 FROM recruitment_participants WHERE guild_id=:guild AND user_id=:user)
          OR EXISTS (SELECT 1 FROM game_participants WHERE guild_id=:guild AND user_id=:user)
          OR EXISTS (
            SELECT 1 FROM auction_recruitment_participants
            WHERE guild_id=:guild AND user_id=:user)
          OR EXISTS (SELECT 1 FROM mmr_ledger_entries WHERE guild_id=:guild AND user_id=:user)
          OR EXISTS (SELECT 1 FROM series_participants WHERE guild_id=:guild AND user_id=:user)
          OR EXISTS (SELECT 1 FROM auction_bid_candidates WHERE guild_id=:guild AND user_id=:user)
          OR EXISTS (SELECT 1 FROM auction_team_members WHERE guild_id=:guild AND user_id=:user)
          OR EXISTS (SELECT 1 FROM game_input_picks WHERE guild_id=:guild AND user_id=:user)
    """),
            p,
        )
    )


async def remove_membership(c: AsyncConnection, p: dict[str, Any]) -> None:
    if await has_membership_history(c, p):
        raise history_conflict()
    try:
        await c.execute(
            text("DELETE FROM user_lane_mmr WHERE guild_id=:guild AND user_id=:user"), p
        )
        removed = await c.execute(
            text("DELETE FROM guild_members WHERE guild_id=:guild AND user_id=:user"), p
        )
    except IntegrityError as error:
        if getattr(error.orig, "sqlstate", None) == "23503":
            raise history_conflict() from None
        raise
    if removed.rowcount != 1:
        raise missing_membership()


async def touch_members_state(c: AsyncConnection, p: dict[str, Any]) -> None:
    await c.execute(
        text("""
        INSERT INTO guild_admin_state (guild_id, scope, revision, updated_by, updated_at)
        VALUES (:guild, 'MEMBERS', 1, NULL, :now)
        ON CONFLICT (guild_id, scope) DO UPDATE
        SET revision=guild_admin_state.revision+1,
            updated_by=NULL, updated_at=EXCLUDED.updated_at
    """),
        p,
    )


async def guild_audit(
    c: AsyncConnection,
    p: dict[str, Any],
    action: str,
    before: dict[str, Any] | None,
    after: dict[str, Any] | None,
) -> None:
    await c.execute(
        text("""
        INSERT INTO admin_audit_log
          (guild_id, id, principal_type, principal_id, action, target_type,
           target_id, checkpoint_id, request_id, before_summary, after_summary,
           result, note, created_at)
        VALUES (:guild, :id, 'USER', :actor, :action, 'GUILD_MEMBER',
                :user, NULL, :request_id, CAST(:before AS jsonb), CAST(:after AS jsonb),
                'APPLIED', NULL, :now)
    """),
        {
            **p,
            "id": uuid.uuid7(),
            "action": action,
            "before": json.dumps(before, ensure_ascii=False) if before is not None else None,
            "after": json.dumps(after, ensure_ascii=False) if after is not None else None,
        },
    )


async def dummy_audit(
    c: AsyncConnection,
    p: dict[str, Any],
    action: str,
    target_type: str,
    target: str,
    before: dict[str, Any] | None,
    after: dict[str, Any] | None,
) -> None:
    await c.execute(
        text("""
        INSERT INTO dummy_user_audit_log
          (id, actor_user_id, action, target_type, target_id,
           request_id, before_summary, after_summary, created_at)
        VALUES (:id, :actor, :action, :target_type, :target,
                :request_id, CAST(:before AS jsonb), CAST(:after AS jsonb), :now)
    """),
        {
            **p,
            "id": uuid.uuid7(),
            "action": action,
            "target_type": target_type,
            "target": target,
            "before": json.dumps(before, ensure_ascii=False) if before is not None else None,
            "after": json.dumps(after, ensure_ascii=False) if after is not None else None,
        },
    )


async def replace_preferences(c: AsyncConnection, p: dict[str, Any], champions: list[str]) -> None:
    await c.execute(
        text("DELETE FROM user_champion_preferences WHERE user_id=:user AND lane=:lane"), p
    )
    for position, champion in enumerate(champions, start=1):
        await c.execute(
            text("""
            INSERT INTO user_champion_preferences (user_id, lane, position, champion_id)
            VALUES (:user, :lane, :position, :champion)
        """),
            {**p, "position": position, "champion": champion},
        )


async def bump_profile(c: AsyncConnection, p: dict[str, Any]) -> None:
    await c.execute(
        text("""
        UPDATE dummy_user_profiles
        SET revision=revision+1, updated_by=:actor, updated_at=:now
        WHERE user_id=:user
    """),
        p,
    )


async def upsert_riot(c: AsyncConnection, p: dict[str, Any]) -> None:
    puuid = await c.scalar(
        text("""
        SELECT puuid FROM riot_accounts
        WHERE user_id=:user AND origin='DUMMY' FOR UPDATE
    """),
        p,
    )
    await c.execute(
        text("""
        INSERT INTO riot_accounts
          (puuid, user_id, platform, routing_region, game_name, tag_line,
           normalized_game_name, normalized_tag_line, profile_icon_id, is_main,
           main_position, main_position_updated_at, last_synced_at,
           created_at, updated_at, origin)
        VALUES (:puuid, :user, :platform, :region, :game, :tag,
                :normalized_game, :normalized_tag, :icon, true,
                :position, CASE WHEN CAST(:position AS text) IS NULL THEN NULL
                                ELSE CAST(:now AS timestamptz) END,
                NULL, CAST(:now AS timestamptz), CAST(:now AS timestamptz), 'DUMMY')
        ON CONFLICT (puuid) DO UPDATE
        SET platform=EXCLUDED.platform, routing_region=EXCLUDED.routing_region,
            game_name=EXCLUDED.game_name, tag_line=EXCLUDED.tag_line,
            normalized_game_name=EXCLUDED.normalized_game_name,
            normalized_tag_line=EXCLUDED.normalized_tag_line,
            profile_icon_id=EXCLUDED.profile_icon_id,
            main_position=EXCLUDED.main_position,
            main_position_updated_at=EXCLUDED.main_position_updated_at,
            updated_at=EXCLUDED.updated_at
        WHERE riot_accounts.user_id=EXCLUDED.user_id AND riot_accounts.origin='DUMMY'
    """),
        {
            **p,
            "puuid": puuid or f"dummy:{uuid.uuid7()}",
            "normalized_game": normalize_riot_id(p["game"]),
            "normalized_tag": normalize_riot_id(p["tag"]),
        },
    )


async def remove_riot(c: AsyncConnection, p: dict[str, Any]) -> None:
    removed = await c.execute(
        text("DELETE FROM riot_accounts WHERE user_id=:user AND origin='DUMMY'"), p
    )
    if removed.rowcount != 1:
        raise ApiError(404, "dummy_riot_account_not_found", "합성 Riot 계정을 찾을 수 없습니다.")
