"""Write only terminal state; confirmed games, lineups, and MMR stay intact."""

from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection


async def match_score(c: AsyncConnection, p: dict[str, Any]) -> tuple[int, int]:
    result = await c.execute(
        text("""
            SELECT count(*) FILTER (
                       WHERE (side_preset='DEFAULT' AND winning_side='BLUE')
                          OR (side_preset='SWAPPED' AND winning_side='RED')
                   ) AS left_score,
                   count(*) FILTER (
                       WHERE (side_preset='DEFAULT' AND winning_side='RED')
                          OR (side_preset='SWAPPED' AND winning_side='BLUE')
                   ) AS right_score
            FROM games
            WHERE guild_id=:guild AND auction_match_id=:match AND status='CONFIRMED'
        """),
        p,
    )
    row = result.one()
    return row.left_score, row.right_score


async def finish_match(c: AsyncConnection, p: dict[str, Any], *, completed: bool) -> int:
    if completed:
        query = """
            UPDATE auction_matches SET revision=revision+1, updated_at=:now, updated_by=:actor_id
            WHERE guild_id=:guild AND id=:match RETURNING revision
        """
    else:
        query = """
            UPDATE auction_matches
            SET status='COMPLETED', ended_early=true, winner_team_id=:winner,
                ended_at=:now, updated_at=:now, updated_by=:actor_id, revision=revision+1
            WHERE guild_id=:guild AND id=:match RETURNING revision
        """
    return int((await c.execute(text(query), p)).scalar_one())


async def finish_tournament(c: AsyncConnection, p: dict[str, Any]) -> int:
    result = await c.execute(
        text("""
            UPDATE auction_tournaments
            SET status='COMPLETED', ended_early=true, champion_team_id=:champion,
                ended_at=:now, updated_at=:now, updated_by=:actor_id, revision=revision+1
            WHERE guild_id=:guild AND id=:tournament RETURNING revision
        """),
        p,
    )
    return int(result.scalar_one())
