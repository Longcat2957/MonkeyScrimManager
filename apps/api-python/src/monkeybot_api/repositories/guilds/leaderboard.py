"""Active guild access and leaderboard rows, including parent-filtered champion counts."""

from typing import Any
from uuid import UUID

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection


async def begin_snapshot(connection: AsyncConnection) -> None:
    await connection.execute(text("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY"))


async def load_access(
    connection: AsyncConnection, guild_id: str, viewer_id: str
) -> dict[str, Any] | None:
    result = await connection.execute(
        text("""
SELECT season.id AS season_id, season.name AS season_name
         FROM guild_members AS viewer
         JOIN guilds AS guild
           ON guild.guild_id = viewer.guild_id
          AND guild.status = 'ACTIVE'
         LEFT JOIN seasons AS season
           ON season.guild_id = viewer.guild_id
          AND season.status = 'ACTIVE'
         WHERE viewer.guild_id = :guild_id
           AND viewer.user_id = :viewer_id
           AND viewer.status = 'ACTIVE'
        """),
        {"guild_id": guild_id, "viewer_id": viewer_id},
    )
    row = result.mappings().one_or_none()
    return dict(row) if row is not None else None


async def load_rows(
    connection: AsyncConnection, guild_id: str, season_id: UUID
) -> list[dict[str, Any]]:
    result = await connection.execute(
        text("""
WITH champion_counts AS (
             SELECT participant.user_id,
                    participant.lane,
                    participant.champion_id,
                    count(*) AS games
             FROM games AS game
             JOIN game_participants AS participant
               ON participant.guild_id = game.guild_id
              AND participant.game_id = game.id
             WHERE game.guild_id = :guild_id
               AND game.season_id = :season_id
               AND game.status = 'CONFIRMED'
               AND participant.champion_id IS NOT NULL
               AND (
                   (game.series_id IS NOT NULL AND EXISTS (
                       SELECT 1
                       FROM series AS parent_series
                       WHERE parent_series.guild_id = game.guild_id
                         AND parent_series.id = game.series_id
                         AND parent_series.status <> 'CANCELLED'
                         AND parent_series.deleted_at IS NULL
                   ))
                   OR
                   (game.auction_match_id IS NOT NULL AND EXISTS (
                       SELECT 1
                       FROM auction_matches AS parent_match
                       JOIN auction_tournaments AS parent_tournament
                         ON parent_tournament.guild_id = parent_match.guild_id
                        AND parent_tournament.id = parent_match.tournament_id
                       WHERE parent_match.guild_id = game.guild_id
                         AND parent_match.id = game.auction_match_id
                         AND parent_match.status <> 'CANCELLED'
                         AND parent_match.deleted_at IS NULL
                         AND parent_tournament.status <> 'CANCELLED'
                         AND parent_tournament.deleted_at IS NULL
                   ))
               )
             GROUP BY participant.user_id, participant.lane, participant.champion_id
         ),
         top_champions AS (
             SELECT DISTINCT ON (user_id, lane)
                    user_id, lane, champion_id
             FROM champion_counts
             ORDER BY user_id, lane, games DESC, champion_id
         )
         SELECT member.user_id,
                member.display_name AS name,
                member.main_lane,
                CASE
                    WHEN privacy.show_riot_ids IS TRUE
                    THEN main_account.game_name || '#' || main_account.tag_line
                    ELSE NULL
                END AS tag,
                CASE
                    WHEN privacy.show_riot_ids IS TRUE
                    THEN main_account.profile_icon_id
                    ELSE NULL
                END AS profile_icon_id,
                lane_mmr.lane,
                lane_mmr.mmr,
                lane_mmr.games_played AS games,
                lane_mmr.wins,
                top_champion.champion_id
         FROM user_lane_mmr AS lane_mmr
         JOIN guild_members AS member
           ON member.guild_id = lane_mmr.guild_id
          AND member.user_id = lane_mmr.user_id
          AND member.status = 'ACTIVE'
         LEFT JOIN user_privacy_settings AS privacy
           ON privacy.user_id = member.user_id
         LEFT JOIN riot_accounts AS main_account
           ON main_account.user_id = member.user_id
          AND main_account.is_main IS TRUE
         LEFT JOIN top_champions AS top_champion
           ON top_champion.user_id = lane_mmr.user_id
          AND top_champion.lane = lane_mmr.lane
         WHERE lane_mmr.guild_id = :guild_id
           AND lane_mmr.season_id = :season_id
           AND lane_mmr.games_played >= 1
         ORDER BY member.user_id,
                  CASE lane_mmr.lane
                      WHEN 'TOP' THEN 1
                      WHEN 'JUNGLE' THEN 2
                      WHEN 'MID' THEN 3
                      WHEN 'BOTTOM' THEN 4
                      WHEN 'SUPPORT' THEN 5
                  END
        """),
        {"guild_id": guild_id, "season_id": season_id},
    )
    return [dict(row) for row in result.mappings()]
