"""SQL operations for auction lineups workflows."""

import uuid
from typing import Any

from sqlalchemy import text
from sqlalchemy.engine import CursorResult
from sqlalchemy.ext.asyncio import AsyncConnection

import monkeybot_api.repositories.auctions.recruitment_queries as auction_queries
import monkeybot_api.repositories.auctions.tournament_queries as tournament_queries
from monkeybot_api.db.queries import one
from monkeybot_api.repositories.auctions.recruitments import rows

LINEUPS = """
    SELECT team_id,lane,team_member_id,user_id FROM auction_match_lineups
    WHERE guild_id=:p1 AND tournament_id=:p2 AND match_id=:p3 ORDER BY team_id,lane FOR UPDATE
"""


async def lock_guild(c: AsyncConnection, values: dict[str, Any]) -> Any:
    return await c.scalar(text(auction_queries.LOCK_GUILD), values)


async def lock_config(c: AsyncConnection, values: dict[str, Any]) -> Any:
    return await c.scalar(text(auction_queries.LOCK_CONFIG), values)


async def lock_member_status(c: AsyncConnection, values: dict[str, Any]) -> Any:
    return await c.scalar(
        text(
            "SELECT status FROM guild_members "
            "WHERE guild_id=:guild AND user_id=:actor_id FOR UPDATE"
        ),
        values,
    )


async def find_tournament_id(c: AsyncConnection, values: dict[str, Any]) -> Any:
    return await c.scalar(
        text(
            "SELECT tournament_id FROM auction_matches "
            "WHERE guild_id=:guild AND id=:id AND deleted_at IS NULL"
        ),
        values,
    )


async def lock_match(c: AsyncConnection, *values: Any) -> dict[str, Any] | None:
    return await one(
        c,
        "SELECT * FROM auction_matches WHERE guild_id=:p1 AND id=:p2 "
        "AND deleted_at IS NULL FOR UPDATE",
        *values,
    )


async def lock_teams(c: AsyncConnection, *values: Any) -> list[dict[str, Any]]:
    return await rows(c, tournament_queries.LOCK_TEAMS, *values)


async def lock_members(c: AsyncConnection, *values: Any) -> list[dict[str, Any]]:
    return await rows(c, tournament_queries.LOCK_MEMBERS, *values)


async def lock_lineups(c: AsyncConnection, *values: Any) -> list[dict[str, Any]]:
    return await rows(c, LINEUPS, *values)


async def update_lineup_state(c: AsyncConnection, values: dict[str, Any], *, kind: str) -> Any:
    update = {
        "ASSIGN": "",
        "LOCK": "status='READY',lineups_locked_at=:now,lineups_locked_by=:actor_id,",
        "UNLOCK": "status='LINEUP_PENDING',lineups_locked_at=NULL,lineups_locked_by=NULL,",
    }[kind]
    return await c.scalar(
        text(
            f"UPDATE auction_matches SET {update} revision=revision+1, "
            "updated_by=:actor_id,updated_at=:now "
            "WHERE guild_id=:guild AND id=:id RETURNING revision"
        ),
        values,
    )


async def start_tournament(c: AsyncConnection, values: dict[str, Any]) -> CursorResult[Any]:
    return await c.execute(
        text("""
            UPDATE auction_tournaments SET status='IN_GAME',revision=revision+1,
                updated_by=:actor_id,updated_at=:now WHERE guild_id=:guild AND id=:tournament
        """),
        values,
    )


async def entry_channel(c: AsyncConnection, values: dict[str, Any]) -> Any:
    return await c.scalar(
        text("SELECT entry_channel_id FROM guild_config WHERE guild_id=:guild"), values
    )


async def clear_assignment(c: AsyncConnection, values: dict[str, Any]) -> CursorResult[Any]:
    return await c.execute(
        text(
            "DELETE FROM auction_match_lineups WHERE guild_id=:guild "
            "AND tournament_id=:tournament AND match_id=:id "
            "AND team_id=:team AND lane=:lane"
        ),
        values,
    )


async def assign_member(c: AsyncConnection, values: dict[str, Any]) -> CursorResult[Any]:
    return await c.execute(
        text("""
                INSERT INTO auction_match_lineups
                    (guild_id,tournament_id,match_id,team_id,lane,team_member_id,user_id)
                VALUES (:guild,:tournament,:id,:team,:lane,:member,:user)
                ON CONFLICT (guild_id,tournament_id,match_id,team_id,lane)
                DO UPDATE SET team_member_id=EXCLUDED.team_member_id,user_id=EXCLUDED.user_id
            """),
        values,
    )


async def reset_unstarted_tournament(
    c: AsyncConnection, values: dict[str, Any]
) -> CursorResult[Any]:
    return await c.execute(
        text("""
            UPDATE auction_tournaments t SET status='BRACKET_SETUP',revision=t.revision+1,
                updated_by=:actor_id,updated_at=:now
            WHERE t.guild_id=:guild AND t.id=:tournament AND t.status='IN_GAME'
                AND t.deleted_at IS NULL AND NOT EXISTS (
                    SELECT 1 FROM auction_matches m WHERE m.guild_id=t.guild_id
                        AND m.tournament_id=t.id AND m.deleted_at IS NULL
                        AND m.status <> 'CANCELLED'
                        AND (m.lineups_locked_at IS NOT NULL
                            OR EXISTS (SELECT 1 FROM games g WHERE g.guild_id=m.guild_id
                                AND g.auction_match_id=m.id)
                            OR EXISTS (SELECT 1 FROM game_inputs i WHERE i.guild_id=m.guild_id
                                AND i.auction_match_id=m.id)))
        """),
        values,
    )


async def lock_tournament(
    c: AsyncConnection, guild: str, tournament: uuid.UUID
) -> dict[str, Any] | None:
    return await one(c, tournament_queries.TOURNAMENT + " FOR UPDATE", guild, tournament)
