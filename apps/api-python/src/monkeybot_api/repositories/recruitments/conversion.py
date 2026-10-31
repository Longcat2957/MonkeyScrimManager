from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

import monkeybot_api.repositories.auctions.recruitment_queries as sql
from monkeybot_api.db.queries import execute


async def lock_entry_revision(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return await connection.scalar(
        text(
            "SELECT revision FROM recruitment_entry_drafts "
            "WHERE guild_id=:guild AND recruitment_id=:id FOR UPDATE"
        ),
        params,
    )


async def series_exists(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return await connection.scalar(
        text(
            """
        SELECT EXISTS (SELECT 1 FROM series WHERE guild_id=:guild
            AND source_recruitment_id=:id AND status <> 'CANCELLED' AND deleted_at IS NULL)
    """
        ),
        params,
    )


async def lock_roster(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return (
        (
            await connection.execute(
                text(
                    (
                        """
        SELECT participant.user_id, assignment.lane
        FROM recruitment_participants AS participant
        LEFT JOIN recruitment_entry_assignments AS assignment
          ON assignment.guild_id=participant.guild_id
         AND assignment.recruitment_id=participant.recruitment_id
         AND assignment.user_id=participant.user_id
        WHERE participant.guild_id=:guild AND participant.recruitment_id=:id
          AND participant.status='JOINED'
        ORDER BY participant.joined_at, participant.user_id FOR UPDATE OF participant
    """
                    )
                ),
                params,
            )
        )
        .mappings()
        .all()
    )


async def next_auction_number(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return await connection.scalar(text(sql.COUNTER), params)


async def insert_auction(connection: AsyncConnection, params: dict[str, Any]) -> None:
    await connection.execute(
        text(
            """
        INSERT INTO auction_recruitments (guild_id,id,season_id,display_number,title,
            format,target_count,semifinal_best_of,final_best_of,hard_fearless,status,
            revision,created_by,created_at,updated_by,updated_at,closed_at)
        VALUES (:guild,:auction_id,:season,:number,:title,'TEN_PERSON',10,NULL,:best_of,
            :fearless,'CLOSED',1,:actor_id,:now,:actor_id,:now,:now)
    """
        ),
        params,
    )


async def insert_participant(connection: AsyncConnection, params: dict[str, Any]) -> None:
    await connection.execute(
        text(
            """
            INSERT INTO auction_recruitment_participants
                (guild_id,recruitment_id,user_id,status,declared_lane,mmr_at_join,
                 snapshot_origin,joined_at)
            VALUES (:guild,:auction_id,:user,'JOINED',:lane,:mmr,:origin,:joined)
        """
        ),
        params,
    )


async def mark_converted(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return await connection.execute(
        text(
            """
        UPDATE recruitments SET status='CONVERTED',revision=revision+1,
            updated_by=:actor_id,updated_at=:now,converted_at=:now
        WHERE guild_id=:guild AND id=:id AND status='CLOSED'
    """
        ),
        params,
    )


async def insert_auction_draft(connection: AsyncConnection, values: dict[str, Any]) -> None:
    await execute(
        connection,
        sql.INSERT_DRAFT,
        values["guild"],
        values["auction_id"],
        values["actor_id"],
        values["now"],
    )


async def insert_auction_team(connection: AsyncConnection, values: dict[str, Any]) -> None:
    await execute(
        connection,
        sql.INSERT_TEAM,
        values["guild"],
        values["auction_id"],
        values["team_index"],
        values["team_name"],
        1000,
    )
