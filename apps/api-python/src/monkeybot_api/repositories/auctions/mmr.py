"""SQL operations for auction mmr workflows."""

from typing import Any

from sqlalchemy import text
from sqlalchemy.engine import CursorResult
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.repositories.series.snapshots import rows


async def result_ledger(c: AsyncConnection, *values: Any) -> list[dict[str, Any]]:
    return await rows(
        c,
        "SELECT * FROM mmr_ledger_entries WHERE guild_id=:guild "
        "AND game_id=:game AND source_type='GAME_RESULT' ORDER BY user_id,lane",
        *values,
    )


async def has_blocking_ledger_entry(c: AsyncConnection, values: dict[str, Any]) -> Any:
    return await c.scalar(
        text("""SELECT EXISTS (SELECT 1 FROM mmr_ledger_entries later
            WHERE later.guild_id=:guild AND later.season_id=:season AND later.user_id=:user_id
            AND later.lane=:lane AND later.lane_revision>:lane_revision AND (
                later.source_type NOT IN ('GAME_RESULT','GAME_REVERSAL') OR (
                    later.source_type='GAME_RESULT' AND NOT EXISTS (SELECT 1 FROM
                    mmr_ledger_entries reversal WHERE reversal.guild_id=later.guild_id
                    AND reversal.reverses_entry_id=later.id
                    AND reversal.source_type='GAME_REVERSAL'))))"""),
        values,
    )


async def append_reversal(c: AsyncConnection, values: dict[str, Any]) -> CursorResult[Any]:
    return await c.execute(
        text("""INSERT INTO mmr_ledger_entries
                (guild_id,id,season_id,user_id,lane,lane_revision,source_type,source_id,game_id,
                 opponent_user_id,mmr_before,mmr_after,delta,games_played_delta,wins_delta,
                 reverses_entry_id,reversed_source_type,actor_type,actor_id,request_id,created_at)
                VALUES (:guild,:entry,:season,:user_id,:lane,:next_revision,'GAME_REVERSAL',
                 :game,:game,:opponent_user_id,:current,:mmr_before,:reversal_delta,:reversal_played,
                 :reversal_wins,:id,'GAME_RESULT',:actor_type,:actor_id,:request_id,:now)"""),
        values,
    )


async def restore_lane_mmr(c: AsyncConnection, values: dict[str, Any]) -> CursorResult[Any]:
    return await c.execute(
        text("""UPDATE user_lane_mmr SET mmr=:mmr_before,
                games_played=games_played-:games_played_delta,wins=wins-:wins_delta,
                revision=:next_revision,updated_at=:now WHERE guild_id=:guild
                AND season_id=:season AND user_id=:user_id AND lane=:lane"""),
        values,
    )


async def void_confirmed_game(c: AsyncConnection, values: dict[str, Any]) -> CursorResult[Any]:
    return await c.execute(
        text(
            "UPDATE games SET status='VOIDED',mmr_status='REVERSED',voided_by=:actor_id, "
            "voided_by_type=:actor_type,voided_at=:now,void_reason=:void_reason "
            "WHERE guild_id=:guild AND id=:game AND status='CONFIRMED'"
        ),
        values,
    )


async def supersede_submitted_input(
    c: AsyncConnection, values: dict[str, Any]
) -> CursorResult[Any]:
    return await c.execute(
        text(
            "UPDATE game_inputs SET status='SUPERSEDED',updated_by=:actor_id,updated_at=:now "
            "WHERE guild_id=:guild AND id=:input AND status='SUBMITTED'"
        ),
        values,
    )
