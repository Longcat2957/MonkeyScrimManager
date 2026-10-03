"""Atomic persistence for dummy-user lifecycle commands."""

import json
import uuid
from datetime import datetime, timedelta
from typing import Any, cast

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError
from monkeybot_api.repositories.dev import users as users_repository
from monkeybot_api.schemas.dev.user_lifecycle import UpdateUser

ROUTE = "/api/dev/dummy-users/{userId}"


async def acquire(
    c: AsyncConnection,
    actor: str,
    method: str,
    route: str,
    key: str,
    request_hash: bytes,
    now: datetime,
) -> tuple[uuid.UUID | None, dict[str, Any] | None]:
    params = {
        "record": uuid.uuid7(),
        "actor": actor,
        "method": method,
        "route": route,
        "key": key,
        "hash": request_hash,
        "now": now,
        "expires": now + timedelta(hours=24),
    }
    locked = await c.scalar(
        text("""
        SELECT pg_try_advisory_xact_lock(hashtextextended(jsonb_build_array(
            'monkeybot:idempotency:v1', 'GLOBAL', 'USER', CAST(:actor AS text),
            CAST(:method AS text), CAST(:route AS text), CAST(:key AS text))::text, 0))
    """),
        params,
    )
    if not locked:
        raise ApiError(409, "idempotency_in_progress", "같은 요청을 처리하고 있습니다.")
    await c.execute(
        text("""
        DELETE FROM idempotency_records
        WHERE scope_key='GLOBAL' AND guild_id IS NULL AND principal_type='USER'
          AND principal_id=:actor AND method=:method AND route_template=:route
          AND idempotency_key=:key AND expires_at<=:now
    """),
        params,
    )
    inserted = await c.scalar(
        text("""
        INSERT INTO idempotency_records
          (id, guild_id, scope_key, principal_type, principal_id, method, route_template,
           idempotency_key, request_hash, state, created_at, expires_at)
        VALUES (:record, NULL, 'GLOBAL', 'USER', :actor, :method, :route,
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
                  AND principal_id=:actor AND method=:method AND route_template=:route
                  AND idempotency_key=:key FOR UPDATE
            """),
                params,
            )
        )
        .mappings()
        .one_or_none()
    )
    if existing is None:
        raise users_repository.invariant()
    if bytes(existing["request_hash"]) != request_hash:
        raise ApiError(
            409, "idempotency_conflict", "같은 Idempotency-Key가 다른 요청에 사용되었습니다."
        )
    if existing["state"] == "IN_PROGRESS":
        raise ApiError(409, "idempotency_in_progress", "같은 요청을 처리하고 있습니다.")
    if existing["state"] != "COMPLETED" or existing["status_code"] != 200:
        raise users_repository.invariant()
    body = existing["response_body"]
    if not isinstance(body, dict):
        raise users_repository.invariant()
    return None, body


async def complete(
    c: AsyncConnection, record: uuid.UUID, body: dict[str, Any], now: datetime
) -> None:
    completed = await c.scalar(
        text("""
        UPDATE idempotency_records SET state='COMPLETED', status_code=200,
            response_body=CAST(:body AS jsonb), completed_at=:now
        WHERE id=:record AND guild_id IS NULL AND scope_key='GLOBAL'
          AND state='IN_PROGRESS' RETURNING id
    """),
        {"record": record, "body": json.dumps(body, ensure_ascii=False), "now": now},
    )
    if completed is None:
        raise users_repository.invariant()


async def lock_profile(c: AsyncConnection, user: str) -> dict[str, Any]:
    row = (
        (
            await c.execute(
                text("""
                SELECT person.display_name, profile.avatar_seed, profile.notes,
                       profile.state, profile.revision
                FROM dummy_user_profiles AS profile
                JOIN users AS person ON person.user_id=profile.user_id
                  AND person.identity_kind='DUMMY'
                WHERE profile.user_id=:user
                FOR UPDATE OF profile, person
            """),
                {"user": user},
            )
        )
        .mappings()
        .one_or_none()
    )
    if row is None:
        raise users_repository.missing()
    return dict(row)


def ensure_revision(expected: int, current: int) -> None:
    if expected != current:
        raise ApiError(
            409,
            "revision_conflict",
            "더미 사용자 데이터가 변경되었습니다. 최신 상태를 다시 확인해 주세요.",
            {"expectedRevision": expected, "currentRevision": current},
        )


async def snapshot(c: AsyncConnection, user: str) -> dict[str, Any]:
    base = await users_repository.get_base(c, user)
    return (await users_repository.snapshots(c, [base]))[0]


async def apply_update(
    c: AsyncConnection, user: str, actor: str, request_id: str, command: UpdateUser, now: datetime
) -> dict[str, Any]:
    current = await lock_profile(c, user)
    ensure_revision(command.expectedRevision, current["revision"])
    name = command.displayName if command.displayName is not None else current["display_name"]
    avatar = command.avatarSeed if command.avatarSeed is not None else current["avatar_seed"]
    notes = command.notes if command.notes is not None else current["notes"]
    await c.execute(
        text("""
        UPDATE users SET display_name=:name, updated_at=:now
        WHERE user_id=:user AND identity_kind='DUMMY'
    """),
        {"name": name, "now": now, "user": user},
    )
    await c.execute(
        text("""
        UPDATE dummy_user_profiles
        SET avatar_seed=:avatar, notes=:notes, revision=revision+1,
            updated_by=:actor, updated_at=:now
        WHERE user_id=:user
    """),
        {"avatar": avatar, "notes": notes, "actor": actor, "now": now, "user": user},
    )
    if command.privacy is not None:
        privacy = command.privacy
        await c.execute(
            text("""
            INSERT INTO user_privacy_settings
              (user_id, public_profile_enabled, show_riot_ids,
               allow_external_record_search, updated_at)
            VALUES (:user, :public, :riot, :external, :now)
            ON CONFLICT (user_id) DO UPDATE
            SET public_profile_enabled=EXCLUDED.public_profile_enabled,
                show_riot_ids=EXCLUDED.show_riot_ids,
                allow_external_record_search=EXCLUDED.allow_external_record_search,
                updated_at=EXCLUDED.updated_at
        """),
            {
                "user": user,
                "public": privacy.publicProfileEnabled,
                "riot": privacy.showRiotIds,
                "external": privacy.allowExternalRecordSearch,
                "now": now,
            },
        )
    await audit_dummy(
        c,
        actor,
        "DUMMY_USER_UPDATED",
        user,
        request_id,
        {
            "displayName": current["display_name"],
            "avatarSeed": current["avatar_seed"],
            "notes": current["notes"],
            "revision": current["revision"],
        },
        {"revision": current["revision"] + 1},
        now,
    )
    return await snapshot(c, user)


async def active_recruitments(c: AsyncConnection, user: str) -> tuple[int, int]:
    row = (
        await c.execute(
            text("""
            SELECT
              (SELECT count(*) FROM recruitment_participants AS participant
               JOIN recruitments AS recruitment
                 ON recruitment.guild_id=participant.guild_id
                AND recruitment.id=participant.recruitment_id
               WHERE participant.user_id=:user AND participant.status='JOINED'
                 AND recruitment.status IN ('OPEN','CLOSED')) AS joined_recruitments,
              (SELECT count(*) FROM auction_recruitment_participants AS participant
               JOIN auction_recruitments AS recruitment
                 ON recruitment.guild_id=participant.guild_id
                AND recruitment.id=participant.recruitment_id
               WHERE participant.user_id=:user AND participant.status='JOINED'
                 AND recruitment.status IN ('OPEN','CLOSED')) AS joined_auction_recruitments
        """),
            {"user": user},
        )
    ).one()
    return row.joined_recruitments, row.joined_auction_recruitments


async def apply_archived(
    c: AsyncConnection,
    user: str,
    actor: str,
    request_id: str,
    expected_revision: int,
    archived: bool,
    now: datetime,
) -> dict[str, Any]:
    current = await lock_profile(c, user)
    ensure_revision(expected_revision, current["revision"])
    desired = "ARCHIVED" if archived else "ACTIVE"
    if current["state"] == desired:
        return await snapshot(c, user)
    if archived:
        joined, auction = await active_recruitments(c, user)
        if joined or auction:
            raise ApiError(
                409,
                "dummy_user_archive_blocked_by_recruitment",
                "진행 중인 일반 또는 경매 모집에 참가 중이라 보관할 수 없습니다. "
                "먼저 참가를 취소해 주세요.",
                {"joinedRecruitments": joined, "joinedAuctionRecruitments": auction},
            )
    candidates = (
        (
            await c.execute(
                text("""
                SELECT guild_id FROM guild_members
                WHERE user_id=:user AND status='ACTIVE' ORDER BY guild_id
            """),
                {"user": user},
            )
        )
        .scalars()
        .all()
        if archived
        else []
    )
    await c.execute(
        text("""
        UPDATE dummy_user_profiles
        SET state=:state, revision=revision+1, updated_by=:actor, updated_at=:now
        WHERE user_id=:user
    """),
        {"state": desired, "actor": actor, "now": now, "user": user},
    )
    await c.execute(
        text("UPDATE users SET deleted_at=NULL, updated_at=:now WHERE user_id=:user"),
        {"now": now, "user": user},
    )
    inactivated: list[str] = []
    for guild in candidates:
        await lock_members_state(c, guild, now)
        status = await c.scalar(
            text("""
            SELECT status FROM guild_members WHERE guild_id=:guild AND user_id=:user FOR UPDATE
        """),
            {"guild": guild, "user": user},
        )
        if status != "ACTIVE":
            continue
        updated = await c.execute(
            text("""
            UPDATE guild_members SET status='INACTIVE', inactivated_at=:now
            WHERE guild_id=:guild AND user_id=:user AND status='ACTIVE'
        """),
            {"guild": guild, "user": user, "now": now},
        )
        if updated.rowcount != 1:
            raise users_repository.invariant()
        await touch_members_state(c, guild, now)
        await audit_guild(c, guild, actor, user, request_id, now)
        inactivated.append(guild)
    await audit_dummy(
        c,
        actor,
        "DUMMY_USER_ARCHIVED" if archived else "DUMMY_USER_RESTORED",
        user,
        request_id,
        {"state": current["state"], "revision": current["revision"]},
        {
            "state": desired,
            "revision": current["revision"] + 1,
            "inactivatedGuildIds": inactivated,
        },
        now,
    )
    return await snapshot(c, user)


async def lock_members_state(c: AsyncConnection, guild: str, now: datetime) -> None:
    await c.execute(
        text("""
        INSERT INTO guild_admin_state (guild_id, scope, revision, updated_by, updated_at)
        VALUES (:guild, 'MEMBERS', 0, NULL, :now)
        ON CONFLICT (guild_id, scope) DO NOTHING
    """),
        {"guild": guild, "now": now},
    )
    await c.scalar(
        text("""
        SELECT revision FROM guild_admin_state
        WHERE guild_id=:guild AND scope='MEMBERS' FOR UPDATE
    """),
        {"guild": guild},
    )


async def touch_members_state(c: AsyncConnection, guild: str, now: datetime) -> None:
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


async def audit_dummy(
    c: AsyncConnection,
    actor: str,
    action: str,
    user: str,
    request_id: str,
    before: dict[str, Any] | None,
    after: dict[str, Any] | None,
    now: datetime,
) -> None:
    await c.execute(
        text("""
        INSERT INTO dummy_user_audit_log
          (id, actor_user_id, action, target_type, target_id,
           request_id, before_summary, after_summary, created_at)
        VALUES (:id, :actor, :action, 'DUMMY_USER', :user,
                :request_id, CAST(:before AS jsonb), CAST(:after AS jsonb), :now)
    """),
        {
            "id": uuid.uuid7(),
            "actor": actor,
            "action": action,
            "user": user,
            "request_id": request_id,
            "before": json.dumps(before, ensure_ascii=False) if before is not None else None,
            "after": json.dumps(after, ensure_ascii=False) if after is not None else None,
            "now": now,
        },
    )


async def audit_guild(
    c: AsyncConnection, guild: str, actor: str, user: str, request_id: str, now: datetime
) -> None:
    before = {"status": "ACTIVE", "source": "DUMMY_USER_ARCHIVE"}
    after = {"status": "INACTIVE", "source": "DUMMY_USER_ARCHIVE"}
    await c.execute(
        text("""
        INSERT INTO admin_audit_log
          (guild_id, id, principal_type, principal_id, action, target_type,
           target_id, checkpoint_id, request_id, before_summary, after_summary,
           result, note, created_at)
        VALUES (:guild, :id, 'USER', :actor, 'MEMBER_STATUS_CHANGED', 'GUILD_MEMBER',
                :user, NULL, :request_id, CAST(:before AS jsonb), CAST(:after AS jsonb),
                'APPLIED', NULL, :now)
    """),
        {
            "guild": guild,
            "id": uuid.uuid7(),
            "actor": actor,
            "user": user,
            "request_id": request_id,
            "before": json.dumps(before),
            "after": json.dumps(after),
            "now": now,
        },
    )


async def deletion_impact(c: AsyncConnection, user: str) -> dict[str, Any]:
    row = (
        (
            await c.execute(
                text("""
            SELECT
              (SELECT count(*) FROM guild_members WHERE user_id=:user) AS guild_memberships,
              (SELECT count(*) FROM recruitment_participants WHERE user_id=:user)
                AS recruitment_participants,
              (SELECT count(*) FROM auction_recruitment_participants WHERE user_id=:user)
                AS auction_participants,
              (SELECT count(*) FROM game_participants WHERE user_id=:user)
                AS game_participants,
              (SELECT count(*) FROM user_sessions WHERE user_id=:user) AS user_sessions,
              (SELECT count(*) FROM user_discord_identities WHERE user_id=:user)
                AS discord_identities,
              (SELECT count(*) FROM riot_accounts WHERE user_id=:user AND origin='RIOT')
                AS real_riot_accounts,
              (SELECT count(*) FROM dummy_user_group_members WHERE user_id=:user)
                AS group_memberships,
              (SELECT count(*) FROM user_champion_preferences WHERE user_id=:user)
                AS champion_preferences,
              (SELECT count(*) FROM riot_accounts WHERE user_id=:user AND origin='DUMMY')
                AS synthetic_riot_accounts
        """),
                {"user": user},
            )
        )
        .mappings()
        .one()
    )
    blocking = {
        "guildMemberships": row["guild_memberships"],
        "recruitmentParticipants": row["recruitment_participants"],
        "auctionParticipants": row["auction_participants"],
        "gameParticipants": row["game_participants"],
        "userSessions": row["user_sessions"],
        "discordIdentities": row["discord_identities"],
        "realRiotAccounts": row["real_riot_accounts"],
    }
    removable = {
        "groupMemberships": row["group_memberships"],
        "championPreferences": row["champion_preferences"],
        "syntheticRiotAccounts": row["synthetic_riot_accounts"],
    }
    return {
        "canDelete": all(
            blocking[name] == 0
            for name in (
                "guildMemberships",
                "userSessions",
                "discordIdentities",
                "realRiotAccounts",
            )
        ),
        "blockingReferences": blocking,
        "removableReferences": removable,
    }


async def apply_delete(
    c: AsyncConnection, user: str, actor: str, request_id: str, now: datetime
) -> dict[str, Any]:
    await lock_profile(c, user)
    impact = await deletion_impact(c, user)
    if not impact["canDelete"]:
        raise ApiError(
            409,
            "dummy_user_deletion_blocked",
            "연결된 내전 데이터가 있어 삭제할 수 없습니다. 더미 사용자를 보관해 주세요.",
            {"impact": impact},
        )
    await audit_dummy(
        c, actor, "DUMMY_USER_DELETED", user, request_id, {"deletionImpact": impact}, None, now
    )
    for table in (
        "dummy_user_group_members",
        "user_champion_preferences",
        "riot_accounts",
        "user_privacy_settings",
        "user_guild_preferences",
        "dummy_user_profiles",
    ):
        suffix = " AND origin='DUMMY'" if table == "riot_accounts" else ""
        await c.execute(text(f"DELETE FROM {table} WHERE user_id=:user{suffix}"), {"user": user})
    deleted = await c.execute(
        text("DELETE FROM users WHERE user_id=:user AND identity_kind='DUMMY'"),
        {"user": user},
    )
    if deleted.rowcount != 1:
        raise users_repository.invariant()
    return impact
