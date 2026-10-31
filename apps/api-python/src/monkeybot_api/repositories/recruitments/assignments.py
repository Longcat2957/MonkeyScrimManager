from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection


async def is_joined(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return await connection.scalar(
        text(
            "SELECT EXISTS (SELECT 1 FROM recruitment_participants WHERE guild_id = :guild "
            "AND recruitment_id = :id AND user_id = :member AND status = 'JOINED')"
        ),
        params,
    )


async def lock_assignment(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return (
        (
            await connection.execute(
                text(
                    "SELECT team, lane FROM recruitment_entry_assignments WHERE guild_id = :guild "
                    "AND recruitment_id = :id AND user_id = :member FOR UPDATE"
                ),
                params,
            )
        )
        .mappings()
        .one_or_none()
    )


async def lock_slot_occupant(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return await connection.scalar(
        text(
            "SELECT user_id FROM recruitment_entry_assignments WHERE guild_id = :guild "
            "AND recruitment_id = :id AND team = :team AND lane = :lane FOR UPDATE"
        ),
        params,
    )


async def remove_assignment(connection: AsyncConnection, params: dict[str, Any]) -> None:
    await connection.execute(
        text(
            "DELETE FROM recruitment_entry_assignments "
            "WHERE guild_id = :guild AND recruitment_id = :id AND user_id = :member"
        ),
        params,
    )


async def move_assignment(connection: AsyncConnection, params: dict[str, Any]) -> None:
    await connection.execute(
        text(
            "UPDATE recruitment_entry_assignments SET team = :team, lane = :lane "
            "WHERE guild_id = :guild AND recruitment_id = :id AND user_id = :member"
        ),
        params,
    )


async def insert_assignment(connection: AsyncConnection, params: dict[str, Any]) -> None:
    await connection.execute(
        text(
            "INSERT INTO recruitment_entry_assignments "
            "(guild_id, recruitment_id, user_id, team, lane) "
            "VALUES (:guild, :id, :member, :team, :lane)"
        ),
        params,
    )
