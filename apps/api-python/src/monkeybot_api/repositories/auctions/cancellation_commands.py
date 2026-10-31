"""SQL operations for auction cancellation workflows."""

from typing import Any

from sqlalchemy import text
from sqlalchemy.engine import CursorResult
from sqlalchemy.ext.asyncio import AsyncConnection

import monkeybot_api.repositories.auctions.recruitment_queries as sql
from monkeybot_api.db.queries import one


async def lock_member_status(c: AsyncConnection, values: dict[str, Any]) -> Any:
    return await c.scalar(
        text(
            "SELECT status FROM guild_members WHERE guild_id=:guild "
            "AND user_id=:actor_id FOR UPDATE"
        ),
        values,
    )


async def cancel_source(c: AsyncConnection, values: dict[str, Any], *, converted: bool) -> Any:
    state = "status='CONVERTED'" if converted else "status IN ('OPEN','CLOSED')"
    return await c.scalar(
        text(f"""UPDATE auction_recruitments SET status='CANCELLED',revision=revision+1,
                     updated_by=:actor_id,updated_at=:now,cancelled_at=:now
                 WHERE guild_id=:guild AND id=:source AND {state} RETURNING revision"""),
        values,
    )


async def lock_tournament(c: AsyncConnection, *values: Any) -> dict[str, Any] | None:
    return await one(
        c,
        """SELECT source_recruitment_id,status,revision FROM auction_tournaments
           WHERE guild_id=:p1 AND id=:p2 AND deleted_at IS NULL FOR UPDATE""",
        *values,
    )


async def lock_source_recruitment(c: AsyncConnection, *values: Any) -> dict[str, Any] | None:
    return await one(c, sql.LOCK_RECRUITMENT, *values)


async def lock_active_matches(c: AsyncConnection, values: dict[str, Any]) -> CursorResult[Any]:
    return await c.execute(
        text("""SELECT id FROM auction_matches WHERE guild_id=:guild AND tournament_id=:tournament
                AND deleted_at IS NULL AND status<>'CANCELLED' ORDER BY id FOR UPDATE"""),
        values,
    )


async def cancel_active_matches(c: AsyncConnection, values: dict[str, Any]) -> CursorResult[Any]:
    return await c.execute(
        text("""UPDATE auction_matches SET status='CANCELLED',winner_team_id=NULL,
                lineups_locked_at=NULL,lineups_locked_by=NULL,ended_at=:now,
                revision=revision+1,updated_by=:actor_id,updated_at=:now
                WHERE guild_id=:guild AND tournament_id=:tournament
                  AND status<>'CANCELLED' AND deleted_at IS NULL"""),
        values,
    )


async def cancel_tournament(c: AsyncConnection, values: dict[str, Any]) -> Any:
    return await c.scalar(
        text("""UPDATE auction_tournaments SET status='CANCELLED',bracket_locked_at=NULL,
                bracket_locked_by=NULL,champion_team_id=NULL,ended_at=:now,revision=revision+1,
                updated_by=:actor_id,updated_at=:now WHERE guild_id=:guild AND id=:tournament
                AND status IN ('BIDDING','BRACKET_SETUP','IN_GAME') AND deleted_at IS NULL
                RETURNING revision"""),
        values,
    )


async def lock_latest_confirmed_game(c: AsyncConnection, *values: Any) -> dict[str, Any] | None:
    return await one(
        c,
        """SELECT game.id,game.season_id,game.source_input_id,game.game_number
           FROM games game JOIN auction_matches auction_match
             ON auction_match.guild_id=game.guild_id AND auction_match.id=game.auction_match_id
           WHERE game.guild_id=:p1 AND auction_match.tournament_id=:p2
             AND auction_match.deleted_at IS NULL AND auction_match.status<>'CANCELLED'
             AND game.status='CONFIRMED'
           ORDER BY game.confirmed_at DESC,game.id DESC LIMIT 1 FOR UPDATE OF game""",
        *values,
    )


async def lock_guild(c: AsyncConnection, values: dict[str, Any]) -> Any:
    return await c.scalar(text(sql.LOCK_GUILD), values)


async def lock_config(c: AsyncConnection, values: dict[str, Any]) -> Any:
    return await c.scalar(text(sql.LOCK_CONFIG), values)


async def lock_recruitment(c: AsyncConnection, *values: Any) -> dict[str, Any] | None:
    return await one(c, sql.LOCK_RECRUITMENT, *values)


async def find_tournament_id(c: AsyncConnection, values: dict[str, Any]) -> Any:
    return await c.scalar(
        text("""SELECT id FROM auction_tournaments WHERE guild_id=:guild
                        AND source_recruitment_id=:id AND deleted_at IS NULL"""),
        values,
    )
