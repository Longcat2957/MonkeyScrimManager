"""Frozen record projection queries, scoped to the same read-only snapshot."""

SEASONS = """
SELECT id, name, status, started_at, ended_at
FROM seasons
WHERE guild_id = :guild_id
ORDER BY (status = 'ACTIVE') DESC, started_at DESC, id DESC
"""

SUMMARY = """
SELECT count(*) AS games,
       count(*) FILTER (WHERE
           (participant.team = 'TEAM_1' AND (
               (game.side_preset = 'DEFAULT' AND game.winning_side = 'BLUE') OR
               (game.side_preset = 'SWAPPED' AND game.winning_side = 'RED')
           )) OR
           (participant.team = 'TEAM_2' AND (
               (game.side_preset = 'DEFAULT' AND game.winning_side = 'RED') OR
               (game.side_preset = 'SWAPPED' AND game.winning_side = 'BLUE')
           ))
       ) AS wins
FROM games AS game
JOIN game_participants AS participant
  ON participant.guild_id = game.guild_id
 AND participant.game_id = game.id
 AND participant.user_id = :user_id
WHERE game.guild_id = :guild_id
  AND game.season_id = :season_id
  AND game.status = 'CONFIRMED'
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
"""

LANE_MMR = """
SELECT lane, mmr, games_played, wins
FROM user_lane_mmr
WHERE guild_id = :guild_id AND season_id = :season_id AND user_id = :user_id
"""

CHAMPIONS = """
SELECT participant.champion_id,
       participant.lane,
       count(*) AS games,
       count(*) FILTER (WHERE
           (participant.team = 'TEAM_1' AND (
               (game.side_preset = 'DEFAULT' AND game.winning_side = 'BLUE') OR
               (game.side_preset = 'SWAPPED' AND game.winning_side = 'RED')
           )) OR
           (participant.team = 'TEAM_2' AND (
               (game.side_preset = 'DEFAULT' AND game.winning_side = 'RED') OR
               (game.side_preset = 'SWAPPED' AND game.winning_side = 'BLUE')
           ))
       ) AS wins,
       count(*) FILTER (WHERE NOT (
           (participant.team = 'TEAM_1' AND (
               (game.side_preset = 'DEFAULT' AND game.winning_side = 'BLUE') OR
               (game.side_preset = 'SWAPPED' AND game.winning_side = 'RED')
           )) OR
           (participant.team = 'TEAM_2' AND (
               (game.side_preset = 'DEFAULT' AND game.winning_side = 'RED') OR
               (game.side_preset = 'SWAPPED' AND game.winning_side = 'BLUE')
           ))
       )) AS losses
FROM games AS game
JOIN game_participants AS participant
  ON participant.guild_id = game.guild_id
 AND participant.game_id = game.id
 AND participant.user_id = :user_id
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
GROUP BY participant.champion_id, participant.lane
ORDER BY count(*) DESC,
         count(*) FILTER (WHERE
           (participant.team = 'TEAM_1' AND (
               (game.side_preset = 'DEFAULT' AND game.winning_side = 'BLUE') OR
               (game.side_preset = 'SWAPPED' AND game.winning_side = 'RED')
           )) OR
           (participant.team = 'TEAM_2' AND (
               (game.side_preset = 'DEFAULT' AND game.winning_side = 'RED') OR
               (game.side_preset = 'SWAPPED' AND game.winning_side = 'BLUE')
           ))
         ) DESC,
         participant.champion_id,
         CASE participant.lane
             WHEN 'TOP' THEN 1 WHEN 'JUNGLE' THEN 2 WHEN 'MID' THEN 3
             WHEN 'BOTTOM' THEN 4 WHEN 'SUPPORT' THEN 5
         END
"""

HEADS = """
WITH candidates AS (
    SELECT 'SERIES'::text AS kind,
           'REGULAR'::text AS scope,
           scrim.id,
           scrim.title,
           max(game.played_at) AS played_at,
           CASE
               WHEN scrim.winning_team = locked.team THEN 'WIN'
               WHEN scrim.winning_team IS NULL THEN 'DRAW'
               ELSE 'LOSS'
           END::text AS result,
           NULL::uuid AS team_id,
           NULL::text AS team_name,
           NULL::text AS opponent_name,
           NULL::text AS stage,
           NULL::integer AS spent_points,
           NULL::smallint AS placement
    FROM series AS scrim
    JOIN series_participants AS locked
      ON locked.guild_id = scrim.guild_id
     AND locked.series_id = scrim.id
     AND locked.user_id = :user_id
    JOIN games AS game
      ON game.guild_id = scrim.guild_id
     AND game.season_id = scrim.season_id
     AND game.series_id = scrim.id
     AND game.status = 'CONFIRMED'
    JOIN game_participants AS target
      ON target.guild_id = game.guild_id
     AND target.game_id = game.id
     AND target.user_id = :user_id
     AND target.team = locked.team
    WHERE scrim.guild_id = :guild_id
      AND scrim.season_id = :season_id
      AND scrim.status = 'COMPLETED'
      AND scrim.deleted_at IS NULL
    GROUP BY scrim.id, scrim.title, scrim.winning_team, locked.team

    UNION ALL

    SELECT 'SERIES'::text AS kind,
           'AUCTION'::text AS scope,
           match.id,
           tournament.title,
           max(game.played_at) AS played_at,
           CASE WHEN match.winner_team_id IS NULL THEN 'DRAW'
                WHEN match.winner_team_id = target_team.id
                THEN 'WIN' ELSE 'LOSS' END::text AS result,
           target_team.id AS team_id,
           target_team.team_name AS team_name,
           opponent_team.team_name AS opponent_name,
           match.match_key AS stage,
           NULL::integer AS spent_points,
           NULL::smallint AS placement
    FROM auction_tournaments AS tournament
    JOIN auction_matches AS match
      ON match.guild_id = tournament.guild_id
     AND match.tournament_id = tournament.id
     AND match.season_id = tournament.season_id
     AND match.status = 'COMPLETED'
     AND match.deleted_at IS NULL
    JOIN games AS game
      ON game.guild_id = match.guild_id
     AND game.season_id = match.season_id
     AND game.auction_match_id = match.id
     AND game.status = 'CONFIRMED'
    JOIN game_participants AS target
      ON target.guild_id = game.guild_id
     AND target.game_id = game.id
     AND target.user_id = :user_id
    JOIN auction_teams AS target_team
      ON target_team.guild_id = match.guild_id
     AND target_team.tournament_id = match.tournament_id
     AND target_team.id = CASE target.team
           WHEN 'TEAM_1' THEN match.left_team_id ELSE match.right_team_id END
    LEFT JOIN auction_teams AS opponent_team
      ON opponent_team.guild_id = match.guild_id
     AND opponent_team.tournament_id = match.tournament_id
     AND opponent_team.id = CASE target.team
           WHEN 'TEAM_1' THEN match.right_team_id ELSE match.left_team_id END
    WHERE tournament.guild_id = :guild_id
      AND tournament.season_id = :season_id
      AND tournament.format = 'TEN_PERSON'
      AND tournament.status = 'COMPLETED'
      AND tournament.deleted_at IS NULL
      AND match.match_key = 'FINAL'
    GROUP BY match.id, tournament.title, match.winner_team_id,
             target_team.id, target_team.team_name,
             opponent_team.team_name, match.match_key

    UNION ALL

    SELECT 'TOURNAMENT'::text AS kind,
           'TOURNAMENT'::text AS scope,
           tournament.id,
           tournament.title,
           max(game.played_at) AS played_at,
           NULL::text AS result,
           target_team.id AS team_id,
           target_team.team_name AS team_name,
           NULL::text AS opponent_name,
           NULL::text AS stage,
           target_team.initial_points - target_team.current_points AS spent_points,
           CASE
               WHEN tournament.champion_team_id = target_team.id THEN 1::smallint
               WHEN bool_or(match.match_key = 'FINAL'
                            AND match.winner_team_id <> target_team.id) THEN 2::smallint
               WHEN bool_or(match.match_key IN ('SEMIFINAL_A', 'SEMIFINAL_B')
                            AND match.winner_team_id <> target_team.id) THEN 3::smallint
               ELSE NULL::smallint
           END AS placement
    FROM auction_tournaments AS tournament
    JOIN auction_matches AS match
      ON match.guild_id = tournament.guild_id
     AND match.tournament_id = tournament.id
     AND match.season_id = tournament.season_id
     AND match.status = 'COMPLETED'
     AND match.deleted_at IS NULL
    JOIN games AS game
      ON game.guild_id = match.guild_id
     AND game.season_id = match.season_id
     AND game.auction_match_id = match.id
     AND game.status = 'CONFIRMED'
    JOIN game_participants AS target
      ON target.guild_id = game.guild_id
     AND target.game_id = game.id
     AND target.user_id = :user_id
    JOIN auction_teams AS target_team
      ON target_team.guild_id = match.guild_id
     AND target_team.tournament_id = match.tournament_id
     AND target_team.id = CASE target.team
           WHEN 'TEAM_1' THEN match.left_team_id ELSE match.right_team_id END
    WHERE tournament.guild_id = :guild_id
      AND tournament.season_id = :season_id
      AND tournament.format = 'TWENTY_PERSON'
      AND tournament.status = 'COMPLETED'
      AND tournament.deleted_at IS NULL
    GROUP BY tournament.id, tournament.title, tournament.champion_team_id,
             target_team.id, target_team.team_name,
             target_team.initial_points, target_team.current_points
)
SELECT kind, scope, id, title, played_at, result, team_id, team_name,
       opponent_name, stage, spent_points, placement
FROM candidates
WHERE CAST(:cursor_played_at AS timestamptz) IS NULL
   OR (played_at, id, kind) <
       (:cursor_played_at, CAST(:cursor_id AS uuid), CAST(:cursor_kind AS text))
ORDER BY played_at DESC, id DESC, kind DESC
LIMIT :page_size
"""

GAMES = """
WITH requested AS (
    SELECT request.scope, request.id
    FROM unnest(CAST(:scopes AS text[]), CAST(:ids AS uuid[])) AS request(scope, id)
)
SELECT requested.id AS top_id,
       game.id,
       COALESCE(game.series_id, game.auction_match_id) AS parent_id,
       game.game_number,
       game.played_at,
       game.winning_side,
       game.side_preset,
       target.team AS target_team,
       target.lane AS target_lane,
       target.champion_id AS target_champion_id,
       ledger.mmr_before,
       ledger.mmr_after
FROM requested
JOIN games AS game
  ON game.guild_id = :guild_id
 AND game.status = 'CONFIRMED'
LEFT JOIN auction_matches AS auction_match
  ON auction_match.guild_id = game.guild_id
 AND auction_match.id = game.auction_match_id
JOIN game_participants AS target
  ON target.guild_id = game.guild_id
 AND target.game_id = game.id
 AND target.user_id = :user_id
LEFT JOIN LATERAL (
    SELECT entry.mmr_before, entry.mmr_after
    FROM mmr_ledger_entries AS entry
    WHERE entry.guild_id = game.guild_id
      AND entry.game_id = game.id
      AND entry.user_id = :user_id
      AND entry.lane = target.lane
      AND entry.source_type = 'GAME_RESULT'
) AS ledger ON true
WHERE (requested.scope = 'REGULAR' AND game.series_id = requested.id)
   OR (requested.scope = 'AUCTION' AND game.auction_match_id = requested.id)
   OR (requested.scope = 'TOURNAMENT'
       AND auction_match.tournament_id = requested.id)
ORDER BY requested.id, game.played_at, game.game_number, game.id
"""

LINEUPS = """
SELECT participant.game_id,
       game.side_preset,
       participant.team,
       member.display_name,
       participant.lane,
       participant.champion_id,
       participant.user_id = :user_id AS is_target
FROM game_participants AS participant
JOIN games AS game
  ON game.guild_id = participant.guild_id
 AND game.id = participant.game_id
JOIN guild_members AS member
  ON member.guild_id = participant.guild_id
 AND member.user_id = participant.user_id
WHERE participant.guild_id = :guild_id
  AND participant.game_id = ANY(CAST(:game_ids AS uuid[]))
ORDER BY participant.game_id,
         participant.team,
         CASE participant.lane
             WHEN 'TOP' THEN 1 WHEN 'JUNGLE' THEN 2 WHEN 'MID' THEN 3
             WHEN 'BOTTOM' THEN 4 WHEN 'SUPPORT' THEN 5
         END
"""

BANS = """
SELECT game_id, side, slot, champion_id
FROM game_bans
WHERE guild_id = :guild_id AND game_id = ANY(CAST(:game_ids AS uuid[]))
ORDER BY game_id, side, slot
"""

SERIES_META = """
SELECT tournament.id AS tournament_id,
       match.id,
       tournament.title,
       match.match_key AS stage,
       opponent_team.team_name AS opponent_name,
       max(game.played_at) AS played_at,
       CASE WHEN match.winner_team_id IS NULL THEN 'DRAW'
            WHEN match.winner_team_id = target_team.id
            THEN 'WIN' ELSE 'LOSS' END::text AS result
FROM auction_tournaments AS tournament
JOIN auction_matches AS match
  ON match.guild_id = tournament.guild_id
 AND match.tournament_id = tournament.id
 AND match.status = 'COMPLETED'
 AND match.deleted_at IS NULL
JOIN games AS game
  ON game.guild_id = match.guild_id
 AND game.auction_match_id = match.id
 AND game.status = 'CONFIRMED'
JOIN game_participants AS target
  ON target.guild_id = game.guild_id
 AND target.game_id = game.id
 AND target.user_id = :user_id
JOIN auction_teams AS target_team
  ON target_team.guild_id = match.guild_id
 AND target_team.tournament_id = match.tournament_id
 AND target_team.id = CASE target.team
       WHEN 'TEAM_1' THEN match.left_team_id ELSE match.right_team_id END
LEFT JOIN auction_teams AS opponent_team
  ON opponent_team.guild_id = match.guild_id
 AND opponent_team.tournament_id = match.tournament_id
 AND opponent_team.id = CASE target.team
       WHEN 'TEAM_1' THEN match.right_team_id ELSE match.left_team_id END
WHERE tournament.guild_id = :guild_id
  AND tournament.id = ANY(CAST(:tournament_ids AS uuid[]))
GROUP BY tournament.id, match.id, tournament.title, match.match_key,
         opponent_team.team_name, match.winner_team_id, target_team.id
ORDER BY tournament.id, max(game.played_at) DESC, match.id DESC
"""

ROSTERS = """
WITH requested AS (
    SELECT request.tournament_id, request.team_id
    FROM unnest(CAST(:tournament_ids AS uuid[]), CAST(:team_ids AS uuid[]))
         AS request(tournament_id, team_id)
)
SELECT requested.tournament_id,
       member.display_name,
       COALESCE(roster.declared_lane, recent_participation.lane) AS lane,
       roster.user_id = :user_id AS is_target
FROM requested
JOIN auction_team_members AS roster
  ON roster.guild_id = :guild_id
 AND roster.tournament_id = requested.tournament_id
 AND roster.team_id = requested.team_id
 AND roster.released_at IS NULL
JOIN guild_members AS member
  ON member.guild_id = roster.guild_id
 AND member.user_id = roster.user_id
LEFT JOIN LATERAL (
    SELECT participant.lane
    FROM auction_matches AS match
    JOIN games AS game
      ON game.guild_id = match.guild_id
     AND game.auction_match_id = match.id
     AND game.status = 'CONFIRMED'
    JOIN game_participants AS participant
      ON participant.guild_id = game.guild_id
     AND participant.game_id = game.id
     AND participant.user_id = roster.user_id
    WHERE match.guild_id = roster.guild_id
      AND match.tournament_id = roster.tournament_id
      AND CASE participant.team
            WHEN 'TEAM_1' THEN match.left_team_id ELSE match.right_team_id END
          = roster.team_id
    ORDER BY game.played_at DESC, game.id DESC
    LIMIT 1
) AS recent_participation ON true
ORDER BY requested.tournament_id, roster.roster_slot
"""
