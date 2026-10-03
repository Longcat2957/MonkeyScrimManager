import uuid
from datetime import datetime
from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

import monkeybot_api.repositories.auctions.recruitment_queries as sql
from monkeybot_api.db.queries import execute, one, params
from monkeybot_api.repositories.auctions.recruitments import rows


async def lock_membership(connection: AsyncConnection, values: dict[str, Any]) -> Any:
    return await connection.scalar(
        text("SELECT status FROM guild_members WHERE guild_id=:guild AND user_id=:user FOR UPDATE"),
        values,
    )


async def insert_event(connection: AsyncConnection, values: dict[str, Any]) -> None:
    await connection.execute(
        text("""
        INSERT INTO outbox_events (id,guild_id,topic,aggregate_type,aggregate_id,
            aggregate_revision,event_type,payload,occurred_at,available_at)
        VALUES (:event_id,:guild,:topic,'AUCTION_RECRUITMENT',:target,:revision,:event,
            CAST(:payload AS jsonb),:now,:now)
    """),
        values,
    )


async def recruitment_channel(connection: AsyncConnection, values: dict[str, Any]) -> Any:
    return await connection.scalar(
        text("SELECT recruitment_channel_id FROM guild_config WHERE guild_id=:guild"), values
    )


async def active_season(connection: AsyncConnection, guild: str) -> Any:
    return await connection.scalar(text(sql.SEASON), params(guild))


async def next_display_number(connection: AsyncConnection, guild: str) -> Any:
    return await connection.scalar(text(sql.COUNTER), params(guild))


async def insert_recruitment(
    connection: AsyncConnection,
    guild: str,
    identifier: uuid.UUID,
    season: uuid.UUID,
    number: int,
    title: str,
    format_name: str,
    count: int,
    semifinal_best_of: int | None,
    final_best_of: int,
    hard_fearless: bool,
    actor_id: str,
    occurred_at: datetime,
) -> None:
    await execute(
        connection,
        sql.INSERT_RECRUITMENT,
        guild,
        identifier,
        season,
        number,
        title,
        format_name,
        count,
        semifinal_best_of,
        final_best_of,
        hard_fearless,
        actor_id,
        occurred_at,
    )


async def insert_draft(
    connection: AsyncConnection,
    guild: str,
    identifier: uuid.UUID,
    actor_id: str,
    occurred_at: datetime,
) -> None:
    await execute(connection, sql.INSERT_DRAFT, guild, identifier, actor_id, occurred_at)


async def insert_team(
    connection: AsyncConnection,
    guild: str,
    identifier: uuid.UUID,
    index: int,
    title: str,
    initial_points: int,
) -> None:
    await execute(connection, sql.INSERT_TEAM, guild, identifier, index, title, initial_points)


async def lock_draft(connection: AsyncConnection, guild: str, identifier: uuid.UUID) -> Any:
    return await connection.scalar(text(sql.LOCK_DRAFT), params(guild, identifier))


async def lock_teams(
    connection: AsyncConnection, guild: str, identifier: uuid.UUID
) -> list[dict[str, Any]]:
    return await rows(connection, sql.LOCK_TEAMS, guild, identifier)


async def lock_participants(
    connection: AsyncConnection, guild: str, identifier: uuid.UUID
) -> list[dict[str, Any]]:
    return await rows(connection, sql.LOCK_PARTICIPANTS, guild, identifier)


async def update_team(
    connection: AsyncConnection,
    team_name: str,
    captain_user_id: str,
    initial_points: int,
    guild: str,
    identifier: uuid.UUID,
    team_index: int,
) -> None:
    await execute(
        connection,
        sql.UPDATE_TEAM,
        team_name,
        captain_user_id,
        initial_points,
        guild,
        identifier,
        team_index,
    )


async def update_draft(
    connection: AsyncConnection,
    actor_id: str,
    occurred_at: datetime,
    guild: str,
    identifier: uuid.UUID,
) -> Any:
    return await connection.scalar(
        text(sql.UPDATE_DRAFT), params(actor_id, occurred_at, guild, identifier)
    )


async def lock_guild(connection: AsyncConnection, guild: str) -> Any:
    return await connection.scalar(text(sql.LOCK_GUILD), params(guild))


async def lock_config(connection: AsyncConnection, guild: str) -> Any:
    return await connection.scalar(text(sql.LOCK_CONFIG), params(guild))


async def lock_recruitment(
    connection: AsyncConnection, guild: str, identifier: uuid.UUID
) -> dict[str, Any] | None:
    return await one(connection, sql.LOCK_RECRUITMENT, guild, identifier)


async def main_lane(connection: AsyncConnection, guild: str, target: str) -> Any:
    return await connection.scalar(text(sql.MAIN_LANE), params(guild, target))


async def participant_status(
    connection: AsyncConnection, guild: str, identifier: uuid.UUID, target: str
) -> Any:
    return await connection.scalar(text(sql.PARTICIPANT_STATUS), params(guild, identifier, target))


async def join_participant(
    connection: AsyncConnection,
    guild: str,
    identifier: uuid.UUID,
    target: str,
    lane: str,
    mmr: float,
    occurred_at: datetime,
) -> None:
    await execute(connection, sql.JOIN, guild, identifier, target, lane, mmr, occurred_at)


async def withdraw_participant(
    connection: AsyncConnection,
    occurred_at: datetime,
    guild: str,
    identifier: uuid.UUID,
    target: str,
) -> Any:
    return await connection.execute(text(sql.LEAVE), params(occurred_at, guild, identifier, target))


async def clear_captain(
    connection: AsyncConnection, guild: str, identifier: uuid.UUID, target: str
) -> None:
    await connection.execute(text(sql.CLEAR_CAPTAIN), params(guild, identifier, target))


async def close(
    connection: AsyncConnection,
    actor_id: str,
    occurred_at: datetime,
    guild: str,
    identifier: uuid.UUID,
) -> None:
    await execute(connection, sql.CLOSE, actor_id, occurred_at, guild, identifier)


async def restore(
    connection: AsyncConnection,
    status: str,
    actor_id: str,
    occurred_at: datetime,
    guild: str,
    identifier: uuid.UUID,
    expected: int,
) -> None:
    await execute(
        connection, sql.RESTORE, status, actor_id, occurred_at, guild, identifier, expected
    )


async def reopen_or_cancel(
    connection: AsyncConnection,
    actor_id: str,
    occurred_at: datetime,
    guild: str,
    identifier: uuid.UUID,
    expected: int,
    reopen: bool,
) -> None:
    await execute(
        connection,
        sql.REOPEN if reopen else sql.CANCEL,
        actor_id,
        occurred_at,
        guild,
        identifier,
        expected,
    )


async def bump_membership_revisions(
    connection: AsyncConnection,
    actor_id: str,
    occurred_at: datetime,
    guild: str,
    identifier: uuid.UUID,
) -> None:
    for query in (sql.BUMP_RECRUITMENT, sql.BUMP_DRAFT):
        await execute(connection, query, actor_id, occurred_at, guild, identifier)
