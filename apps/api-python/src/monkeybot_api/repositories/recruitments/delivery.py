from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection


async def lock_actor_membership(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return await connection.scalar(
        text(
            "SELECT status FROM guild_members "
            "WHERE guild_id = :guild AND user_id = :actor_id FOR UPDATE"
        ),
        params,
    )


async def lock_guild(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return await connection.scalar(
        text("SELECT status FROM guilds WHERE guild_id = :guild FOR UPDATE"), params
    )


async def lock_config(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return (
        (
            await connection.execute(
                text(
                    "SELECT operator_role_id, entry_channel_id, recruitment_channel_id "
                    "FROM guild_config "
                    "WHERE guild_id = :guild FOR UPDATE"
                ),
                params,
            )
        )
        .mappings()
        .one_or_none()
    )


async def lock_recruitment(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return (
        (
            await connection.execute(
                text(
                    (
                        "SELECT status, revision, created_by, season_id, target_count, "
                        "best_of, hard_fearless FROM recruitments "
                        "WHERE guild_id = :guild AND id = :id FOR UPDATE"
                    )
                ),
                params,
            )
        )
        .mappings()
        .one_or_none()
    )


async def bind_message(connection: AsyncConnection, params: dict[str, Any]) -> None:
    await connection.execute(
        text(
            "UPDATE recruitments SET channel_id = :channel, message_id = :message, "
            "revision = revision + 1, updated_by = :actor_id, updated_at = :now "
            "WHERE guild_id = :guild AND id = :id"
        ),
        params,
    )
