import uuid
from datetime import datetime
from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

import monkeybot_api.repositories.auctions.recruitment_queries as source_sql
import monkeybot_api.repositories.auctions.tournament_queries as sql
from monkeybot_api.db.queries import one, params
from monkeybot_api.repositories.auctions.recruitments import rows
from monkeybot_api.repositories.auctions.state import change


async def insert_event(connection: AsyncConnection, values: dict[str, Any]) -> None:
    await connection.execute(
        text("""INSERT INTO outbox_events
        (id,guild_id,topic,aggregate_type,aggregate_id,aggregate_revision,
         event_type,
         payload,occurred_at,available_at)
        VALUES (:event,:guild,:topic,:kind,:target,:revision,:name,
                CAST(:payload AS jsonb),:now,:now)"""),
        values,
    )


async def source_lock_recruitment(
    connection: AsyncConnection, guild: str, source_id: uuid.UUID
) -> dict[str, Any] | None:
    return await one(connection, source_sql.LOCK_RECRUITMENT, guild, source_id)


async def source_lock_draft(connection: AsyncConnection, guild: str, source_id: uuid.UUID) -> Any:
    return await connection.scalar(text(source_sql.LOCK_DRAFT), params(guild, source_id))


async def source_lock_teams(
    connection: AsyncConnection, guild: str, source_id: uuid.UUID
) -> list[dict[str, Any]]:
    return await rows(connection, source_sql.LOCK_TEAMS, guild, source_id)


async def source_lock_participants(
    connection: AsyncConnection, guild: str, source_id: uuid.UUID
) -> list[dict[str, Any]]:
    return await rows(connection, source_sql.LOCK_PARTICIPANTS, guild, source_id)


async def existing_tournament(connection: AsyncConnection, guild: str, source_id: uuid.UUID) -> Any:
    return await connection.scalar(text(sql.EXISTING), params(guild, source_id))


async def next_display_number(connection: AsyncConnection, guild: str) -> Any:
    return await connection.scalar(text(sql.COUNTER), params(guild))


async def create_tournament(
    connection: AsyncConnection,
    guild: str,
    identifier: uuid.UUID,
    season_id: uuid.UUID,
    source_id: uuid.UUID,
    number: int,
    title: str,
    format_name: str,
    semifinal_best_of: int | None,
    final_best_of: int,
    hard_fearless: bool,
    occurred_at: datetime,
    actor_id: str,
) -> None:
    await change(
        connection,
        sql.CREATE,
        guild,
        identifier,
        season_id,
        source_id,
        number,
        title,
        format_name,
        semifinal_best_of,
        final_best_of,
        hard_fearless,
        occurred_at,
        actor_id,
    )


async def create_team(
    connection: AsyncConnection,
    guild: str,
    identifier: uuid.UUID,
    team_id: uuid.UUID,
    team_index: int,
    team_name: str,
    initial_points: int,
    occurred_at: datetime,
) -> None:
    await change(
        connection,
        sql.CREATE_TEAM,
        guild,
        identifier,
        team_id,
        team_index,
        team_name,
        initial_points,
        occurred_at,
    )


async def insert_captain(
    connection: AsyncConnection,
    guild: str,
    identifier: uuid.UUID,
    team_id: uuid.UUID,
    member_id: uuid.UUID,
    user_id: str,
    declared_lane: str | None,
    mmr_at_join: float | None,
    snapshot_origin: str,
    occurred_at: datetime,
) -> None:
    await change(
        connection,
        sql.CAPTAIN,
        guild,
        identifier,
        team_id,
        member_id,
        user_id,
        declared_lane,
        mmr_at_join,
        snapshot_origin,
        occurred_at,
    )


async def insert_initial_ledger(
    connection: AsyncConnection,
    guild: str,
    identifier: uuid.UUID,
    ledger_id: uuid.UUID,
    team_id: uuid.UUID,
    initial_points: int,
    actor_id: str,
    occurred_at: datetime,
) -> None:
    await change(
        connection,
        sql.INITIAL_LEDGER,
        guild,
        identifier,
        ledger_id,
        team_id,
        initial_points,
        actor_id,
        occurred_at,
    )


async def create_candidate(
    connection: AsyncConnection,
    guild: str,
    identifier: uuid.UUID,
    user: str,
    declared_lane: str | None,
    mmr_at_join: float | None,
    snapshot_origin: str,
    position: int,
    occurred_at: datetime,
) -> None:
    await change(
        connection,
        sql.CREATE_CANDIDATE,
        guild,
        identifier,
        user,
        declared_lane,
        mmr_at_join,
        snapshot_origin,
        position,
        occurred_at,
    )


async def create_slot(
    connection: AsyncConnection, guild: str, identifier: uuid.UUID, slot: str, occurred_at: datetime
) -> None:
    await change(connection, sql.CREATE_SLOT, guild, identifier, slot, occurred_at)


async def convert_recruitment(
    connection: AsyncConnection,
    actor_id: str,
    occurred_at: datetime,
    guild: str,
    source_id: uuid.UUID,
) -> None:
    await change(connection, sql.CONVERT, actor_id, occurred_at, guild, source_id)


async def source_lock_guild(connection: AsyncConnection, guild: str) -> Any:
    return await connection.scalar(text(source_sql.LOCK_GUILD), params(guild))


async def source_lock_config(connection: AsyncConnection, guild: str) -> Any:
    return await connection.scalar(text(source_sql.LOCK_CONFIG), params(guild))


async def lock_actor_membership(connection: AsyncConnection, values: dict[str, Any]) -> Any:
    return await connection.scalar(
        text(
            "SELECT status FROM guild_members WHERE guild_id=:guild "
            "AND user_id=:actor_id FOR UPDATE"
        ),
        values,
    )


async def lock_tournament(
    connection: AsyncConnection, guild: str, identifier: uuid.UUID
) -> dict[str, Any] | None:
    return await one(connection, sql.LOCK_TOURNAMENT, guild, identifier)


async def result_artifacts_exist(
    connection: AsyncConnection, guild: str, identifier: uuid.UUID
) -> Any:
    return await connection.scalar(text(sql.ARTIFACTS), params(guild, identifier))


async def advance_revision(
    connection: AsyncConnection,
    status: str,
    actor_id: str,
    occurred_at: datetime,
    guild: str,
    identifier: uuid.UUID,
) -> Any:
    return await connection.scalar(
        text(sql.BUMP), params(status, actor_id, occurred_at, guild, identifier)
    )


async def lock_draft(
    connection: AsyncConnection,
    guild: str,
    identifier: uuid.UUID,
) -> list[list[dict[str, Any]]]:
    return [
        await rows(connection, query, guild, identifier)
        for query in (
            sql.LOCK_TEAMS,
            sql.LOCK_MEMBERS,
            sql.LOCK_CANDIDATES,
            sql.LOCK_LOTS,
            sql.ACTIONS,
            sql.LEDGER,
        )
    ]
