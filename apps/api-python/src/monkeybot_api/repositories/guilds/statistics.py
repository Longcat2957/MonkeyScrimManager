"""Season-scoped statistics queries; all values are bound within one read snapshot."""

from typing import Any
from uuid import UUID

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

VALID_GAMES = """
WITH valid_games AS (
    SELECT game.id,
           game.side_preset,
           game.winning_side,
           game.duration_seconds,
           game.series_id IS NOT NULL AS is_regular,
           CASE
               WHEN (game.side_preset = 'DEFAULT' AND game.winning_side = 'BLUE')
                 OR (game.side_preset = 'SWAPPED' AND game.winning_side = 'RED')
               THEN 'TEAM_1'
               ELSE 'TEAM_2'
           END AS winning_team
      FROM games AS game
 LEFT JOIN series AS parent_series
        ON parent_series.guild_id = game.guild_id
       AND parent_series.id = game.series_id
 LEFT JOIN auction_matches AS auction_match
        ON auction_match.guild_id = game.guild_id
       AND auction_match.id = game.auction_match_id
 LEFT JOIN auction_tournaments AS auction_tournament
        ON auction_tournament.guild_id = game.guild_id
       AND auction_tournament.id = auction_match.tournament_id
     WHERE game.guild_id = :guild
       AND game.season_id = :season
       AND game.status = 'CONFIRMED'
       AND (
           (game.series_id IS NOT NULL
            AND parent_series.status <> 'CANCELLED'
            AND parent_series.deleted_at IS NULL)
           OR
           (game.auction_match_id IS NOT NULL
            AND auction_match.status <> 'CANCELLED'
            AND auction_match.deleted_at IS NULL
            AND auction_tournament.status <> 'CANCELLED'
            AND auction_tournament.deleted_at IS NULL)
       )
       AND (
           :scope = 'ALL'
           OR (:scope = 'REGULAR' AND game.series_id IS NOT NULL)
           OR (:scope = 'AUCTION' AND game.auction_match_id IS NOT NULL)
       )
)
"""

LIST_SEASONS = """SELECT id, name, status
         FROM seasons
         WHERE guild_id = :guild
         ORDER BY (status = 'ACTIVE') DESC, started_at DESC, id DESC"""

OVERVIEW = (
    VALID_GAMES
    + """
         SELECT count(*) AS games,
                count(*) FILTER (WHERE game.is_regular) AS regular_games,
                count(*) FILTER (WHERE NOT game.is_regular) AS auction_games,
                (SELECT count(DISTINCT participant.user_id)
                   FROM valid_games AS player_game
                   JOIN game_participants AS participant
                     ON participant.guild_id = :guild
                    AND participant.game_id = player_game.id) AS unique_players,
                count(*) FILTER (WHERE game.winning_side = 'BLUE') AS blue_wins,
                count(*) FILTER (WHERE game.winning_side = 'RED') AS red_wins,
                AVG(game.duration_seconds)::double precision AS average_duration_seconds,
                count(*) FILTER (WHERE game.duration_seconds IS NOT NULL) AS duration_games,
                count(*) FILTER (WHERE EXISTS (
                    SELECT 1 FROM game_participant_stats AS stat
                    WHERE stat.guild_id = :guild AND stat.game_id = game.id
                )) AS participant_stat_games,
                count(*) FILTER (WHERE EXISTS (
                    SELECT 1 FROM game_participants AS participant
                    WHERE participant.guild_id = :guild
                      AND participant.game_id = game.id
                      AND participant.champion_id IS NOT NULL
                )) AS champion_pick_games,
                count(*) FILTER (WHERE EXISTS (
                    SELECT 1 FROM game_bans AS ban
                    WHERE ban.guild_id = :guild AND ban.game_id = game.id
                )) AS champion_ban_games
           FROM valid_games AS game"""
)

PLAYERS = (
    VALID_GAMES
    + """
         SELECT participant.user_id,
                member.display_name AS name,
                count(*) AS games,
                count(*) FILTER (WHERE participant.team = game.winning_team) AS wins
           FROM valid_games AS game
           JOIN game_participants AS participant
             ON participant.guild_id = :guild
            AND participant.game_id = game.id
           JOIN guild_members AS member
             ON member.guild_id = participant.guild_id
            AND member.user_id = participant.user_id
          GROUP BY participant.user_id, member.display_name"""
)

DUOS = (
    VALID_GAMES
    + """
         SELECT user_a.user_id AS user_a_id,
                member_a.display_name AS user_a_name,
                user_b.user_id AS user_b_id,
                member_b.display_name AS user_b_name,
                count(*) AS games,
                count(*) FILTER (WHERE user_a.team = game.winning_team) AS wins
           FROM valid_games AS game
           JOIN game_participants AS user_a
             ON user_a.guild_id = :guild
            AND user_a.game_id = game.id
           JOIN game_participants AS user_b
             ON user_b.guild_id = user_a.guild_id
            AND user_b.game_id = user_a.game_id
            AND user_b.team = user_a.team
            AND user_a.user_id < user_b.user_id
           JOIN guild_members AS member_a
             ON member_a.guild_id = user_a.guild_id
            AND member_a.user_id = user_a.user_id
           JOIN guild_members AS member_b
             ON member_b.guild_id = user_b.guild_id
            AND member_b.user_id = user_b.user_id
          GROUP BY user_a.user_id, member_a.display_name, user_b.user_id, member_b.display_name
          ORDER BY games DESC, wins DESC, user_a.user_id, user_b.user_id
          LIMIT 10"""
)

CHAMPION_PICKS = (
    VALID_GAMES
    + """
         SELECT participant.champion_id,
                participant.lane,
                count(*) AS games,
                count(*) FILTER (WHERE participant.team = game.winning_team) AS wins
           FROM valid_games AS game
           JOIN game_participants AS participant
             ON participant.guild_id = :guild
            AND participant.game_id = game.id
          WHERE participant.champion_id IS NOT NULL
          GROUP BY participant.champion_id, participant.lane
          ORDER BY games DESC, wins DESC, participant.champion_id, participant.lane
          LIMIT 10"""
)

CHAMPION_BANS = (
    VALID_GAMES
    + """
         SELECT ban.champion_id, count(*) AS bans
           FROM valid_games AS game
           JOIN game_bans AS ban
             ON ban.guild_id = :guild
            AND ban.game_id = game.id
          GROUP BY ban.champion_id
          ORDER BY bans DESC, ban.champion_id
          LIMIT 10"""
)

COMPLETED_TOURNAMENTS = """SELECT count(*)
         FROM auction_tournaments
         WHERE guild_id = :guild
           AND season_id = :season
           AND status = 'COMPLETED'
           AND deleted_at IS NULL"""

CAPTAINS = """WITH captain_teams AS (
             SELECT tournament.id AS tournament_id,
                    team.id AS team_id,
                    captain.user_id,
                    member.display_name AS name,
                    tournament.champion_team_id,
                    (team.initial_points - team.current_points)::bigint AS spent_points
             FROM auction_tournaments AS tournament
             JOIN auction_teams AS team
               ON team.guild_id = tournament.guild_id
              AND team.tournament_id = tournament.id
             JOIN auction_team_members AS captain
               ON captain.guild_id = team.guild_id
              AND captain.tournament_id = team.tournament_id
              AND captain.team_id = team.id
              AND captain.acquired_via = 'CAPTAIN'
              AND captain.released_at IS NULL
             JOIN guild_members AS member
               ON member.guild_id = captain.guild_id
              AND member.user_id = captain.user_id
             WHERE tournament.guild_id = :guild
               AND tournament.season_id = :season
               AND tournament.status = 'COMPLETED'
               AND tournament.deleted_at IS NULL
         ),
         team_stats AS (
             SELECT user_id,
                    name,
                    count(*)::bigint AS teams_captained,
                    count(*) FILTER (WHERE champion_team_id = team_id)::bigint AS titles,
                    sum(spent_points)::bigint AS total_spent_points
             FROM captain_teams
             GROUP BY user_id, name
         ),
         match_stats AS (
             SELECT captain.user_id,
                    count(*)::bigint AS matches,
                    count(*) FILTER (WHERE game_match.winner_team_id = captain.team_id)
                        ::bigint AS match_wins,
                    count(*) FILTER (WHERE game_match.match_key = 'FINAL')
                        ::bigint AS final_appearances
             FROM captain_teams AS captain
             JOIN auction_matches AS game_match
               ON game_match.guild_id = :guild
              AND game_match.tournament_id = captain.tournament_id
              AND (game_match.left_team_id = captain.team_id
                   OR game_match.right_team_id = captain.team_id)
             WHERE game_match.status = 'COMPLETED'
               AND game_match.deleted_at IS NULL
             GROUP BY captain.user_id
         ),
         game_stats AS (
             SELECT captain.user_id,
                    count(*)::bigint AS games,
                    count(*) FILTER (WHERE
                        (game_match.left_team_id = captain.team_id AND (
                            (game.side_preset = 'DEFAULT' AND game.winning_side = 'BLUE') OR
                            (game.side_preset = 'SWAPPED' AND game.winning_side = 'RED')
                        ))
                        OR
                        (game_match.right_team_id = captain.team_id AND (
                            (game.side_preset = 'DEFAULT' AND game.winning_side = 'RED') OR
                            (game.side_preset = 'SWAPPED' AND game.winning_side = 'BLUE')
                        ))
                    )::bigint AS game_wins
             FROM captain_teams AS captain
             JOIN auction_matches AS game_match
               ON game_match.guild_id = :guild
              AND game_match.tournament_id = captain.tournament_id
              AND (game_match.left_team_id = captain.team_id
                   OR game_match.right_team_id = captain.team_id)
             JOIN games AS game
               ON game.guild_id = game_match.guild_id
              AND game.auction_match_id = game_match.id
              AND game.status = 'CONFIRMED'
             WHERE game_match.status = 'COMPLETED'
               AND game_match.deleted_at IS NULL
             GROUP BY captain.user_id
         )
         SELECT team_stats.user_id,
                team_stats.name,
                team_stats.teams_captained,
                team_stats.titles,
                coalesce(match_stats.final_appearances, 0)::bigint AS final_appearances,
                coalesce(match_stats.matches, 0)::bigint AS matches,
                coalesce(match_stats.match_wins, 0)::bigint AS match_wins,
                coalesce(game_stats.games, 0)::bigint AS games,
                coalesce(game_stats.game_wins, 0)::bigint AS game_wins,
                team_stats.total_spent_points
         FROM team_stats
         LEFT JOIN match_stats ON match_stats.user_id = team_stats.user_id
         LEFT JOIN game_stats ON game_stats.user_id = team_stats.user_id
         ORDER BY team_stats.titles DESC,
                  coalesce(game_stats.game_wins, 0) DESC,
                  coalesce(game_stats.games, 0) DESC,
                  team_stats.user_id
         LIMIT 10"""

TOP_BIDS = """WITH bid_members AS (
             SELECT tournament.id AS tournament_id,
                    tournament.title AS tournament_title,
                    tournament.champion_team_id,
                    team.id AS team_id,
                    team.team_name,
                    bidder.user_id,
                    member.display_name AS name,
                    action.points
             FROM auction_tournaments AS tournament
             JOIN auction_teams AS team
               ON team.guild_id = tournament.guild_id
              AND team.tournament_id = tournament.id
             JOIN auction_team_members AS bidder
               ON bidder.guild_id = team.guild_id
              AND bidder.tournament_id = team.tournament_id
              AND bidder.team_id = team.id
              AND bidder.acquired_via = 'BID'
              AND bidder.released_at IS NULL
             JOIN auction_actions AS action
               ON action.guild_id = bidder.guild_id
              AND action.tournament_id = bidder.tournament_id
              AND action.id = bidder.acquisition_action_id
              AND action.kind = 'AWARD_BID'
             JOIN guild_members AS member
               ON member.guild_id = bidder.guild_id
              AND member.user_id = bidder.user_id
             WHERE tournament.guild_id = :guild
               AND tournament.season_id = :season
               AND tournament.status = 'COMPLETED'
               AND tournament.deleted_at IS NULL
         ),
         bid_game_stats AS (
             SELECT bid.tournament_id,
                    bid.team_id,
                    bid.user_id,
                    count(*)::bigint AS games,
                    count(*) FILTER (WHERE participant.team = CASE
                        WHEN (game.side_preset = 'DEFAULT' AND game.winning_side = 'BLUE')
                          OR (game.side_preset = 'SWAPPED' AND game.winning_side = 'RED')
                        THEN 'TEAM_1'
                        ELSE 'TEAM_2'
                    END)::bigint AS wins
             FROM bid_members AS bid
             JOIN auction_matches AS game_match
               ON game_match.guild_id = :guild
              AND game_match.tournament_id = bid.tournament_id
              AND game_match.status = 'COMPLETED'
              AND game_match.deleted_at IS NULL
             JOIN games AS game
               ON game.guild_id = game_match.guild_id
              AND game.auction_match_id = game_match.id
              AND game.status = 'CONFIRMED'
             JOIN game_participants AS participant
               ON participant.guild_id = game.guild_id
              AND participant.game_id = game.id
              AND participant.user_id = bid.user_id
             GROUP BY bid.tournament_id, bid.team_id, bid.user_id
         )
         SELECT bid.tournament_id,
                bid.tournament_title,
                bid.team_name,
                bid.user_id,
                bid.name,
                bid.points,
                coalesce(stats.games, 0)::bigint AS games,
                coalesce(stats.wins, 0)::bigint AS wins,
                (bid.team_id = bid.champion_team_id) AS won_tournament
         FROM bid_members AS bid
         LEFT JOIN bid_game_stats AS stats
           ON stats.tournament_id = bid.tournament_id
          AND stats.team_id = bid.team_id
          AND stats.user_id = bid.user_id
         ORDER BY bid.points DESC, coalesce(stats.games, 0) DESC, bid.tournament_id, bid.user_id
         LIMIT 10"""


async def begin_snapshot(connection: AsyncConnection) -> None:
    await connection.execute(text("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY"))


async def has_access(connection: AsyncConnection, guild: str, actor: str) -> bool:
    return bool(
        await connection.scalar(
            text("""
        SELECT EXISTS (
            SELECT 1 FROM guild_members AS member
            JOIN guilds AS guild ON guild.guild_id=member.guild_id AND guild.status='ACTIVE'
            WHERE member.guild_id=:guild AND member.user_id=:actor AND member.status='ACTIVE'
        )
    """),
            {"guild": guild, "actor": actor},
        )
    )


async def list_seasons(connection: AsyncConnection, guild: str) -> list[dict[str, Any]]:
    result = await connection.execute(text(LIST_SEASONS), {"guild": guild})
    return [dict(row) for row in result.mappings()]


async def overview(
    connection: AsyncConnection, guild: str, season: UUID, scope: str
) -> dict[str, Any]:
    result = await connection.execute(
        text(OVERVIEW), {"guild": guild, "season": season, "scope": scope}
    )
    return dict(result.mappings().one())


async def players(
    connection: AsyncConnection, guild: str, season: UUID, scope: str
) -> list[dict[str, Any]]:
    result = await connection.execute(
        text(PLAYERS), {"guild": guild, "season": season, "scope": scope}
    )
    return [dict(row) for row in result.mappings()]


async def duos(
    connection: AsyncConnection, guild: str, season: UUID, scope: str
) -> list[dict[str, Any]]:
    result = await connection.execute(
        text(DUOS), {"guild": guild, "season": season, "scope": scope}
    )
    return [dict(row) for row in result.mappings()]


async def champion_picks(
    connection: AsyncConnection, guild: str, season: UUID, scope: str
) -> list[dict[str, Any]]:
    result = await connection.execute(
        text(CHAMPION_PICKS), {"guild": guild, "season": season, "scope": scope}
    )
    return [dict(row) for row in result.mappings()]


async def champion_bans(
    connection: AsyncConnection, guild: str, season: UUID, scope: str
) -> list[dict[str, Any]]:
    result = await connection.execute(
        text(CHAMPION_BANS), {"guild": guild, "season": season, "scope": scope}
    )
    return [dict(row) for row in result.mappings()]


async def completed_tournaments(connection: AsyncConnection, guild: str, season: UUID) -> int:
    result = await connection.execute(
        text(COMPLETED_TOURNAMENTS), {"guild": guild, "season": season}
    )
    return int(result.scalar_one())


async def captains(connection: AsyncConnection, guild: str, season: UUID) -> list[dict[str, Any]]:
    result = await connection.execute(text(CAPTAINS), {"guild": guild, "season": season})
    return [dict(row) for row in result.mappings()]


async def top_bids(connection: AsyncConnection, guild: str, season: UUID) -> list[dict[str, Any]]:
    result = await connection.execute(text(TOP_BIDS), {"guild": guild, "season": season})
    return [dict(row) for row in result.mappings()]
