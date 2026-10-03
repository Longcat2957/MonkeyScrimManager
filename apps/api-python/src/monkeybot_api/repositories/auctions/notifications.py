"""SQL operations for auction notifications workflows."""

from typing import Any

from sqlalchemy import text
from sqlalchemy.engine import CursorResult
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.repositories.series.snapshots import rows


async def result_channel(c: AsyncConnection, values: dict[str, Any]) -> Any:
    return await c.scalar(
        text("SELECT result_channel_id FROM guild_config WHERE guild_id=:guild"), values
    )


async def completed_tournament(c: AsyncConnection, *values: Any) -> list[dict[str, Any]]:
    return await rows(
        c,
        """SELECT t.*,team.team_name AS champion_name
        FROM auction_tournaments t LEFT JOIN auction_teams team ON team.guild_id=t.guild_id
        AND team.tournament_id=t.id AND team.id=t.champion_team_id
        WHERE t.guild_id=:guild AND t.id=:tournament AND t.status='COMPLETED'
        AND t.deleted_at IS NULL""",
        *values,
    )


async def completed_matches(c: AsyncConnection, *values: Any) -> list[dict[str, Any]]:
    return await rows(
        c,
        """SELECT m.*,lt.team_name AS left_name,rt.team_name AS right_name,
        wt.team_name AS winner_name FROM auction_matches m
        LEFT JOIN auction_teams lt ON lt.guild_id=m.guild_id AND lt.tournament_id=m.tournament_id
            AND lt.id=m.left_team_id
        LEFT JOIN auction_teams rt ON rt.guild_id=m.guild_id AND rt.tournament_id=m.tournament_id
            AND rt.id=m.right_team_id
        LEFT JOIN auction_teams wt ON wt.guild_id=m.guild_id AND wt.tournament_id=m.tournament_id
            AND wt.id=m.winner_team_id
        WHERE m.guild_id=:guild AND m.tournament_id=:tournament AND m.status='COMPLETED'
            AND m.deleted_at IS NULL ORDER BY CASE m.match_key WHEN 'SEMIFINAL_A' THEN 1
            WHEN 'SEMIFINAL_B' THEN 2 WHEN 'FINAL' THEN 3 ELSE 4 END""",
        *values,
    )


async def confirmed_games(c: AsyncConnection, *values: Any) -> list[dict[str, Any]]:
    return await rows(
        c,
        "SELECT * FROM games WHERE guild_id=:guild "
        "AND auction_match_id=:match AND status='CONFIRMED' ORDER BY game_number",
        *values,
    )


async def lock_result_notifications(c: AsyncConnection, *values: Any) -> list[dict[str, Any]]:
    return await rows(
        c,
        """SELECT *,
        (delivered_at IS NULL AND failed_at IS NULL AND lease_until>=clock_timestamp()) AS active
        FROM bot_notification_outbox WHERE guild_id=:guild
        AND notification_type='AUCTION_TOURNAMENT_RESULT_CREATE'
        AND payload->>'tournamentId'=:target FOR UPDATE""",
        *values,
    )


async def cancel_pending_notification(
    c: AsyncConnection, values: dict[str, Any]
) -> CursorResult[Any]:
    return await c.execute(
        text("""UPDATE bot_notification_outbox SET failed_at=:now,
                failure_code='RESULT_REVERTED',
                last_error='auction tournament result was reverted before delivery',
                lease_owner=NULL,lease_token=NULL,lease_until=NULL
                WHERE guild_id=:guild AND id=:card AND delivered_at IS NULL
                AND failed_at IS NULL"""),
        values,
    )
