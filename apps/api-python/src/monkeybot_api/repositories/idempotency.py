from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError


def in_progress(bot: bool) -> ApiError:
    return ApiError(
        409,
        "idempotency_in_progress",
        "같은 interaction 요청을 처리하고 있습니다."
        if bot
        else "같은 내전 생성 요청을 처리하고 있습니다.",
    )


async def acquire(
    connection: AsyncConnection, params: dict[str, Any], bot: bool
) -> dict[str, Any] | None:
    params.setdefault("method", "POST")
    locked = await connection.scalar(
        text("""
        SELECT pg_try_advisory_xact_lock(hashtextextended(jsonb_build_array(
            'monkeybot:idempotency:v1', CAST(:guild AS text), CAST(:principal_type AS text),
            CAST(:principal AS text), CAST(:method AS text),
            CAST(:route AS text), CAST(:key AS text)
        )::text, 0))
    """),
        params,
    )
    if not locked:
        raise in_progress(bot)
    where = """scope_key = :guild AND guild_id = :guild AND principal_type = :principal_type
               AND principal_id = :principal AND method = :method AND route_template = :route
               AND idempotency_key = :key"""
    await connection.execute(
        text(f"DELETE FROM idempotency_records WHERE {where} AND expires_at <= :now"), params
    )
    inserted = await connection.scalar(
        text("""
        INSERT INTO idempotency_records
          (id, guild_id, scope_key, principal_type, principal_id, method, route_template,
           idempotency_key, request_hash, state, created_at, expires_at)
        VALUES (:idem_id, :guild, :guild, :principal_type, :principal, :method, :route,
                :key, :hash, 'IN_PROGRESS', :now, :expires)
        ON CONFLICT ON CONSTRAINT idempotency_records_command_key DO NOTHING RETURNING id
    """),
        params,
    )
    if inserted is not None:
        return None
    row = (
        (
            await connection.execute(
                text(f"""
        SELECT request_hash, state, status_code, response_body FROM idempotency_records
        WHERE {where} FOR UPDATE
    """),
                params,
            )
        )
        .mappings()
        .one()
    )
    if row["request_hash"] != params["hash"]:
        raise ApiError(
            409,
            "idempotency_conflict",
            "같은 interaction ID가 다른 요청에 사용되었습니다."
            if bot
            else "같은 Idempotency-Key가 다른 요청에 사용되었습니다.",
        )
    if row["state"] == "IN_PROGRESS":
        raise in_progress(bot)
    if row["state"] != "COMPLETED" or not isinstance(row["response_body"], dict):
        raise ApiError.from_status(500)
    return dict(row["response_body"])
