"""Persistence and global idempotency for dummy-user groups."""

import json
import uuid
from datetime import datetime
from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.time import timestamp


async def list_ids(c: AsyncConnection) -> list[uuid.UUID]:
    result = await c.execute(text("SELECT id FROM dummy_user_groups ORDER BY lower(name), id"))
    return list(result.scalars())


async def fetch(c: AsyncConnection, group_id: uuid.UUID) -> dict[str, Any]:
    row = (
        (
            await c.execute(
                text("""
            SELECT id, name, description, revision, created_at, updated_at
            FROM dummy_user_groups WHERE id=:id
        """),
                {"id": group_id},
            )
        )
        .mappings()
        .one_or_none()
    )
    if row is None:
        raise ApiError(404, "dummy_group_not_found", "더미 사용자 그룹을 찾을 수 없습니다.")
    result = await c.execute(
        text("""
        SELECT member.user_id, user_record.display_name, member.position
        FROM dummy_user_group_members AS member
        JOIN users AS user_record ON user_record.user_id=member.user_id
        WHERE member.group_id=:id ORDER BY member.position
    """),
        {"id": group_id},
    )
    members = [
        {"userId": member.user_id, "displayName": member.display_name, "position": member.position}
        for member in result
    ]
    return {
        "id": str(row["id"]),
        "name": row["name"],
        "description": row["description"],
        "revision": row["revision"],
        "memberCount": len(members),
        "members": members,
        "createdAt": timestamp(row["created_at"]),
        "updatedAt": timestamp(row["updated_at"]),
    }


async def lock(c: AsyncConnection, group_id: uuid.UUID) -> dict[str, Any]:
    row = (
        (
            await c.execute(
                text("""
            SELECT id, name, description, revision
            FROM dummy_user_groups WHERE id=:id FOR UPDATE
        """),
                {"id": group_id},
            )
        )
        .mappings()
        .one_or_none()
    )
    if row is None:
        raise ApiError(404, "dummy_group_not_found", "더미 사용자 그룹을 찾을 수 없습니다.")
    return dict(row)


async def active_member_count(c: AsyncConnection, member_ids: list[str]) -> int:
    if not member_ids:
        return 0
    value = await c.scalar(
        text("""
        SELECT count(*) FROM dummy_user_profiles
        WHERE user_id=ANY(:member_ids) AND state='ACTIVE'
    """),
        {"member_ids": member_ids},
    )
    return int(value)


async def insert(c: AsyncConnection, p: dict[str, Any]) -> None:
    await c.execute(
        text("""
        INSERT INTO dummy_user_groups
          (id, name, description, revision, created_by, updated_by, created_at, updated_at)
        VALUES (:id, :name, :description, 0, :actor, :actor, :now, :now)
    """),
        p,
    )


async def update(c: AsyncConnection, p: dict[str, Any]) -> None:
    await c.execute(
        text("""
        UPDATE dummy_user_groups
        SET name=:name, description=:description, revision=revision+1,
            updated_by=:actor, updated_at=:now WHERE id=:id
    """),
        p,
    )


async def bump_revision(c: AsyncConnection, p: dict[str, Any]) -> None:
    await c.execute(
        text("""
        UPDATE dummy_user_groups
        SET revision=revision+1, updated_by=:actor, updated_at=:now WHERE id=:id
    """),
        p,
    )


async def replace_members(
    c: AsyncConnection, group_id: uuid.UUID, member_ids: list[str], now: datetime
) -> None:
    await c.execute(
        text("DELETE FROM dummy_user_group_members WHERE group_id=:id"), {"id": group_id}
    )
    for position, member_id in enumerate(member_ids, start=1):
        await c.execute(
            text("""
            INSERT INTO dummy_user_group_members (group_id, user_id, position, added_at)
            VALUES (:id, :user, :position, :now)
        """),
            {"id": group_id, "user": member_id, "position": position, "now": now},
        )


async def delete(c: AsyncConnection, group_id: uuid.UUID) -> None:
    await c.execute(text("DELETE FROM dummy_user_groups WHERE id=:id"), {"id": group_id})


async def audit(
    c: AsyncConnection,
    p: dict[str, Any],
    action: str,
    before: dict[str, Any] | None,
    after: dict[str, Any] | None,
) -> None:
    await c.execute(
        text("""
        INSERT INTO dummy_user_audit_log
          (id, actor_user_id, action, target_type, target_id,
           request_id, before_summary, after_summary, created_at)
        VALUES (:audit_id, :actor, :action, 'DUMMY_USER_GROUP', :target,
                :request_id, CAST(:before AS jsonb), CAST(:after AS jsonb), :now)
    """),
        {
            **p,
            "action": action,
            "before": json.dumps(before, ensure_ascii=False) if before is not None else None,
            "after": json.dumps(after, ensure_ascii=False) if after is not None else None,
        },
    )


async def acquire(c: AsyncConnection, p: dict[str, Any]) -> dict[str, Any] | None:
    locked = await c.scalar(
        text("""
        SELECT pg_try_advisory_xact_lock(hashtextextended(jsonb_build_array(
          'monkeybot:idempotency:v1', 'GLOBAL', 'USER', CAST(:actor AS text),
          CAST(:method AS text), CAST(:route AS text), CAST(:key AS text)
        )::text, 0))
    """),
        p,
    )
    if not locked:
        raise ApiError(409, "idempotency_in_progress", "같은 요청을 처리하고 있습니다.")
    where = """
        scope_key='GLOBAL' AND guild_id IS NULL AND principal_type='USER'
        AND principal_id=:actor AND method=:method AND route_template=:route
        AND idempotency_key=:key
    """
    await c.execute(text(f"DELETE FROM idempotency_records WHERE {where} AND expires_at<=:now"), p)
    inserted = await c.scalar(
        text("""
        INSERT INTO idempotency_records
          (id, guild_id, scope_key, principal_type, principal_id, method, route_template,
           idempotency_key, request_hash, state, created_at, expires_at)
        VALUES (:idem_id, NULL, 'GLOBAL', 'USER', :actor, :method, :route,
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
        raise ApiError.from_status(500)
    if bytes(row["request_hash"]) != p["hash"]:
        raise ApiError(
            409, "idempotency_conflict", "같은 Idempotency-Key가 다른 요청에 사용되었습니다."
        )
    if row["state"] == "IN_PROGRESS":
        raise ApiError(409, "idempotency_in_progress", "같은 요청을 처리하고 있습니다.")
    if row["state"] != "COMPLETED" or not isinstance(row["response_body"], dict):
        raise ApiError.from_status(500)
    return dict(row["response_body"])


async def complete(
    c: AsyncConnection, p: dict[str, Any], body: dict[str, Any], status: int
) -> None:
    changed = await c.scalar(
        text("""
        UPDATE idempotency_records
        SET state='COMPLETED', status_code=:status, response_body=CAST(:body AS jsonb),
            completed_at=:now
        WHERE id=:idem_id AND guild_id IS NULL AND scope_key='GLOBAL'
          AND state='IN_PROGRESS' RETURNING id
    """),
        {**p, "status": status, "body": json.dumps(body, ensure_ascii=False)},
    )
    if changed is None:
        raise ApiError.from_status(500)
