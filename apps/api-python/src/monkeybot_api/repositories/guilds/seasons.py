from typing import Any, cast

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.db.queries import one, params
from monkeybot_api.repositories.guilds import queries as sql


async def list_seasons(c: AsyncConnection, guild: str) -> list[dict[str, Any]]:
    rows = (await c.execute(text(sql.SEASONS), params(guild))).mappings().all()
    return [dict(row) for row in rows]


async def replay_status(c: AsyncConnection, p: dict[str, Any]) -> int | None:
    return cast(
        int | None,
        await c.scalar(
            text("""
            SELECT status_code FROM idempotency_records WHERE guild_id=:guild AND scope_key=:guild
                AND principal_type='USER' AND principal_id=:principal AND method='POST'
                AND route_template=:route AND idempotency_key=:key
        """),
            p,
        ),
    )


async def lock_active(c: AsyncConnection, guild: str) -> dict[str, Any] | None:
    await c.execute(text(sql.ROTATION_LOCK), params(guild))
    return await one(c, sql.ACTIVE_SEASON, guild)


async def close_season(c: AsyncConnection, p: dict[str, Any], previous: Any) -> int:
    closed = await c.execute(
        text("""
        UPDATE seasons SET status='CLOSED', ended_at=:now
        WHERE guild_id=:guild AND id=:previous AND status='ACTIVE' AND ended_at IS NULL
        """),
        {**p, "previous": previous},
    )
    return closed.rowcount


async def create_season(c: AsyncConnection, p: dict[str, Any], name: str) -> None:
    await c.execute(
        text("""
        INSERT INTO seasons
            (guild_id, id, name, status, started_at, ended_at, created_by, created_at)
        VALUES (:guild, :season, :name, 'ACTIVE', :now, NULL, :actor_id, :now)
        """),
        {**p, "name": name},
    )
