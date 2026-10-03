from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.repositories.recruitments.entry_queries import ASSIGNMENTS


async def lock_entry_revision(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return await connection.scalar(
        text(
            "SELECT revision FROM recruitment_entry_drafts "
            "WHERE guild_id = :guild AND recruitment_id = :id FOR UPDATE"
        ),
        params,
    )


async def joined_count(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return await connection.scalar(
        text(
            "SELECT count(*) FROM recruitment_participants "
            "WHERE guild_id = :guild AND recruitment_id = :id AND status = 'JOINED'"
        ),
        params,
    )


async def lock_assignments(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return (
        (await connection.execute(text(ASSIGNMENTS + " FOR UPDATE OF assignment"), params))
        .mappings()
        .all()
    )


async def ensure_lane_mmr(connection: AsyncConnection, params: dict[str, Any]) -> None:
    await connection.execute(
        text(
            """
            INSERT INTO user_lane_mmr
                (guild_id, season_id, user_id, lane, mmr, games_played, wins, revision, updated_at)
            VALUES (:guild, :season, :user, :lane, 1500, 0, 0, 0, :now)
            ON CONFLICT (guild_id, season_id, user_id, lane) DO NOTHING
        """
        ),
        params,
    )


async def lock_lane_mmr(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return await connection.scalar(
        text(
            "SELECT mmr FROM user_lane_mmr "
            "WHERE guild_id = :guild AND season_id = :season AND user_id = :user "
            "AND lane = :lane FOR UPDATE"
        ),
        params,
    )


async def next_series_number(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return await connection.scalar(
        text(
            "UPDATE guild_counters SET next_value = next_value + 1 "
            "WHERE guild_id = :guild AND counter_key = 'SERIES' RETURNING next_value - 1"
        ),
        params,
    )


async def insert_series(connection: AsyncConnection, params: dict[str, Any]) -> None:
    await connection.execute(
        text(
            """
        INSERT INTO series (guild_id, id, season_id, source_recruitment_id, display_number,
            title, best_of, hard_fearless, status, revision, started_at, created_by,
            updated_by, updated_at)
        VALUES (:guild, :series_id, :season, :id, :number, :title, :best_of, :hard_fearless,
            'IN_PROGRESS', 0, :now, :actor_id, :actor_id, :now)
    """
        ),
        params,
    )


async def insert_participant(connection: AsyncConnection, params: dict[str, Any]) -> None:
    await connection.execute(
        text(
            """
            INSERT INTO series_participants
                (guild_id, series_id, user_id, team, lane, mmr_at_lock, mmr_snapshot_origin)
            VALUES (:guild, :series_id, :user, :team, :lane, :mmr, 'CAPTURED')
        """
        ),
        params,
    )


async def mark_converted(connection: AsyncConnection, params: dict[str, Any]) -> None:
    await connection.execute(
        text(
            "UPDATE recruitments SET status = 'CONVERTED', revision = revision + 1, "
            "updated_by = :actor_id, updated_at = :now, converted_at = :now "
            "WHERE guild_id = :guild AND id = :id"
        ),
        params,
    )


async def member_names(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return (
        await connection.execute(
            text(
                "SELECT user_id, display_name FROM guild_members "
                "WHERE guild_id = :guild AND user_id = ANY(:users)"
            ),
            params,
        )
    ).all()
