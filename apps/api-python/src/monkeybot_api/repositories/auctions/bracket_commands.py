"""SQL operations for auction bracket workflows."""

import uuid
from typing import Any

from sqlalchemy import text
from sqlalchemy.engine import CursorResult
from sqlalchemy.ext.asyncio import AsyncConnection

import monkeybot_api.repositories.auctions.recruitment_queries as auction_queries
import monkeybot_api.repositories.auctions.tournament_queries as tournament_queries
from monkeybot_api.db.queries import one
from monkeybot_api.repositories.auctions.bracket import (
    ACTIVE_MATCHES,
    RESULT_ARTIFACTS,
    SLOT_ROWS,
    TOURNAMENT,
)
from monkeybot_api.repositories.auctions.recruitments import rows


async def clear_slot_assignment(c: AsyncConnection, values: dict[str, Any]) -> CursorResult[Any]:
    return await c.execute(
        text("""
            UPDATE auction_bracket_slots SET team_id=NULL,updated_by=:actor_id,updated_at=:now
            WHERE guild_id=:guild AND tournament_id=:id AND (slot_key=:slot OR team_id=:team)
        """),
        values,
    )


async def assign_slot_team(c: AsyncConnection, values: dict[str, Any]) -> CursorResult[Any]:
    return await c.execute(
        text("""
                UPDATE auction_bracket_slots SET team_id=:team,updated_by=:actor_id,updated_at=:now
                WHERE guild_id=:guild AND tournament_id=:id AND slot_key=:slot
            """),
        values,
    )


async def has_result_artifacts(c: AsyncConnection, values: dict[str, Any]) -> Any:
    return await c.scalar(text(RESULT_ARTIFACTS), values)


async def cancel_active_matches(c: AsyncConnection, values: dict[str, Any]) -> CursorResult[Any]:
    return await c.execute(
        text("""
            UPDATE auction_matches SET status='CANCELLED',winner_team_id=NULL,
                lineups_locked_at=NULL,lineups_locked_by=NULL,ended_at=:now,
                revision=revision+1,updated_by=:actor_id,updated_at=:now
            WHERE guild_id=:guild AND tournament_id=:id
                AND status <> 'CANCELLED' AND deleted_at IS NULL
        """),
        values,
    )


async def create_match(c: AsyncConnection, values: dict[str, Any]) -> CursorResult[Any]:
    return await c.execute(
        text("""
                INSERT INTO auction_matches (guild_id,tournament_id,id,season_id,match_key,
                    best_of,hard_fearless,left_team_id,right_team_id,status,revision,updated_by,updated_at)
                VALUES (:guild,:id,:match,:season,:key,:best_of,:fearless,:left,:right,
                    'LINEUP_PENDING',0,:actor_id,:now)
            """),
        values,
    )


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


async def lock_teams(c: AsyncConnection, *values: Any) -> list[dict[str, Any]]:
    return await rows(c, tournament_queries.LOCK_TEAMS, *values)


async def lock_members(c: AsyncConnection, *values: Any) -> list[dict[str, Any]]:
    return await rows(c, tournament_queries.LOCK_MEMBERS, *values)


async def lock_active_matches(c: AsyncConnection, *values: Any) -> list[dict[str, Any]]:
    return await rows(c, ACTIVE_MATCHES, *values)


async def update_bracket(c: AsyncConnection, values: dict[str, Any], *, kind: str) -> Any:
    if kind == "LOCK_BRACKET":
        update = "bracket_locked_at=:now,bracket_locked_by=:actor_id,"
    elif kind == "UNLOCK_BRACKET":
        update = (
            "status='BRACKET_SETUP',bracket_locked_at=NULL,bracket_locked_by=NULL,"
            "champion_team_id=NULL,ended_at=NULL,"
        )
    else:
        update = "status='BRACKET_SETUP',"
    return await c.scalar(
        text(f"""
        UPDATE auction_tournaments SET {update} revision=revision+1,
            updated_by=:actor_id,updated_at=:now
        WHERE guild_id=:guild AND id=:id AND deleted_at IS NULL RETURNING revision
    """),
        values,
    )


async def ensure_slot(c: AsyncConnection, values: dict[str, Any]) -> CursorResult[Any]:
    return await c.execute(
        text("""
            INSERT INTO auction_bracket_slots (guild_id,tournament_id,slot_key,updated_at)
            VALUES (:guild,:id,:slot,:now) ON CONFLICT (guild_id,tournament_id,slot_key) DO NOTHING
        """),
        values,
    )


async def lock_tournament(
    c: AsyncConnection, guild: str, tournament: uuid.UUID
) -> dict[str, Any] | None:
    return await one(c, TOURNAMENT + " FOR UPDATE", guild, tournament)


async def lock_slots(c: AsyncConnection, guild: str, tournament: uuid.UUID) -> list[dict[str, Any]]:
    return await rows(c, SLOT_ROWS + " FOR UPDATE", guild, tournament)
