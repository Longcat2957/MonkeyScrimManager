"""Static SQL from the Axum tournament creation and draft workflow."""

EXISTING = """
SELECT EXISTS (
             SELECT 1
             FROM auction_tournaments
             WHERE guild_id = :p1
               AND source_recruitment_id = :p2
               AND status <> 'CANCELLED'
               AND deleted_at IS NULL
         )
"""

CREATE = """
INSERT INTO auction_tournaments (
             guild_id, id, season_id, source_recruitment_id, display_number, title, format,
             semifinal_best_of, final_best_of, hard_fearless, status, revision,
             bracket_locked_at, bracket_locked_by, champion_team_id, started_at, ended_at,
             created_by, updated_by, updated_at, end_card_channel_id,
             end_card_message_id, deleted_at
         ) VALUES (
             :p1, :p2, :p3, :p4, :p5, :p6, :p7,
             :p8, :p9, :p10, 'BIDDING', 0,
             NULL, NULL, NULL, :p11, NULL,
             :p12, :p12, :p11, NULL, NULL, NULL
         )
"""

CREATE_TEAM = """
INSERT INTO auction_teams (
                 guild_id, tournament_id, id, team_index, team_name, initial_points,
                 current_points, point_revision, created_at
             ) VALUES (:p1, :p2, :p3, :p4, :p5, :p6, :p6, 0, :p7)
"""

CAPTAIN = """
INSERT INTO auction_team_members (
                 guild_id, tournament_id, team_id, id, user_id, roster_slot, acquired_via,
                 declared_lane, mmr_at_acquisition, snapshot_origin, acquisition_action_id,
                 acquisition_action_kind, joined_at, released_at
             ) VALUES (:p1, :p2, :p3, :p4, :p5, 1, 'CAPTAIN', :p6, :p7, :p8, NULL, NULL, :p9, NULL)
"""

INITIAL_LEDGER = """
INSERT INTO auction_point_ledger (
                 guild_id, tournament_id, id, team_id, point_revision, reason,
                 source_action_id, source_action_kind, points_before, points_after, delta,
                 reverses_entry_id, reversed_reason, actor_type, actor_id, created_at
             ) VALUES (:p1, :p2, :p3, :p4, 0, 'INITIAL', NULL, NULL, 0, :p5, :p5,
                       NULL, NULL, 'USER', :p6, :p7)
"""

CREATE_CANDIDATE = """
INSERT INTO auction_bid_candidates (
                 guild_id, tournament_id, user_id, declared_lane, mmr_at_start, snapshot_origin,
                 queue_position, state, draw_count, updated_at
             ) VALUES (:p1, :p2, :p3, :p4, :p5, :p6, :p7, 'QUEUED', 0, :p8)
"""

CREATE_SLOT = """
INSERT INTO auction_bracket_slots (
                 guild_id, tournament_id, slot_key, team_id, updated_by, updated_at
             ) VALUES (:p1, :p2, :p3, NULL, NULL, :p4)
"""

CONVERT = """
UPDATE auction_recruitments
         SET status = 'CONVERTED', revision = revision + 1, updated_by = :p1, updated_at = :p2,
             converted_at = :p2
         WHERE guild_id = :p3 AND id = :p4 AND status = 'CLOSED'
"""

TOURNAMENT = """
SELECT id, season_id, source_recruitment_id, display_number, title, format,
                semifinal_best_of, final_best_of, hard_fearless, status, revision,
                bracket_locked_at
         FROM auction_tournaments
         WHERE guild_id = :p1 AND id = :p2 AND deleted_at IS NULL
"""

TEAMS = """
SELECT id, team_index, team_name, initial_points, current_points, point_revision
         FROM auction_teams
         WHERE guild_id = :p1 AND tournament_id = :p2
         ORDER BY team_index
"""

MEMBERS = """
SELECT member.team_id, member.id, member.user_id, guild_member.display_name,
                member.declared_lane, member.mmr_at_acquisition, member.snapshot_origin,
                member.acquired_via, member.roster_slot
         FROM auction_team_members AS member
         JOIN guild_members AS guild_member
           ON guild_member.guild_id = member.guild_id
          AND guild_member.user_id = member.user_id
         WHERE member.guild_id = :p1
           AND member.tournament_id = :p2
           AND member.released_at IS NULL
         ORDER BY member.team_id, member.roster_slot
"""

CANDIDATES = """
SELECT candidate.user_id, member.display_name, candidate.declared_lane,
                candidate.mmr_at_start, candidate.snapshot_origin, candidate.queue_position,
                candidate.state, candidate.draw_count
         FROM auction_bid_candidates AS candidate
         JOIN guild_members AS member
           ON member.guild_id = candidate.guild_id
          AND member.user_id = candidate.user_id
         WHERE candidate.guild_id = :p1 AND candidate.tournament_id = :p2
         ORDER BY candidate.queue_position, candidate.user_id
"""

ACTIVE_LOTS = """
SELECT target_user_id
         FROM auction_lots
         WHERE guild_id = :p1 AND tournament_id = :p2 AND status = 'ACTIVE'
         ORDER BY draw_sequence, id
"""

PROFILE = """
SELECT app_user.avatar_hash,
                member.main_lane,
                account.profile_icon_id,
                cached_riot.snapshot AS riot_snapshot,
                cached_riot.refreshed_at AS riot_refreshed_at,
                solo_rank.tier AS solo_rank_tier,
                solo_rank.division AS solo_rank_division
         FROM guild_members AS member
         JOIN users AS app_user
           ON app_user.user_id = member.user_id
          AND app_user.deleted_at IS NULL
         LEFT JOIN LATERAL (
             SELECT riot.platform, riot.puuid, riot.profile_icon_id
             FROM riot_accounts AS riot
             WHERE riot.user_id = member.user_id
               AND riot.is_main = true
               AND riot.origin = 'RIOT'
             ORDER BY riot.updated_at DESC
             LIMIT 1
         ) AS account ON TRUE
         LEFT JOIN LATERAL (
             SELECT records.snapshot, records.refreshed_at
             FROM riot_records_snapshots AS records
             WHERE records.platform = account.platform
               AND records.puuid = account.puuid
             LIMIT 1
         ) AS cached_riot ON TRUE
         LEFT JOIN LATERAL (
             SELECT rank_entry.value ->> 'tier' AS tier,
                    rank_entry.value ->> 'rank' AS division
             FROM riot_records_snapshots AS records
             CROSS JOIN LATERAL jsonb_array_elements(
                 CASE
                     WHEN jsonb_typeof(records.snapshot -> 'ranks') = 'array'
                         THEN records.snapshot -> 'ranks'
                     ELSE '[]'::jsonb
                 END
             ) AS rank_entry(value)
             WHERE records.platform = account.platform
               AND records.puuid = account.puuid
               AND rank_entry.value ->> 'queueType' = 'RANKED_SOLO_5x5'
             LIMIT 1
         ) AS solo_rank ON TRUE
         WHERE member.guild_id = :p1 AND member.user_id = :p2
"""

LANE_MMR = """
SELECT lane, mmr, games_played, wins
         FROM user_lane_mmr
         WHERE guild_id = :p1 AND season_id = :p2 AND user_id = :p3
"""

CHAMPIONS = """
SELECT champion_id
             FROM user_champion_preferences
             WHERE user_id = :p1 AND lane = :p2
             ORDER BY position
             LIMIT 10
"""

ARTIFACTS = """
SELECT EXISTS (
             SELECT 1
             FROM auction_matches
             WHERE guild_id = :p1
               AND tournament_id = :p2
               AND deleted_at IS NULL
               AND status <> 'CANCELLED'
             UNION ALL
             SELECT 1
             FROM auction_match_lineups AS lineup
             JOIN auction_matches AS auction_match
               ON auction_match.guild_id = lineup.guild_id
              AND auction_match.id = lineup.match_id
             WHERE lineup.guild_id = :p1
               AND lineup.tournament_id = :p2
               AND auction_match.deleted_at IS NULL
               AND auction_match.status <> 'CANCELLED'
             UNION ALL
             SELECT 1
             FROM games AS game
             JOIN auction_matches AS match
               ON match.guild_id = game.guild_id
              AND match.id = game.auction_match_id
             WHERE game.guild_id = :p1
               AND match.tournament_id = :p2
               AND match.deleted_at IS NULL
               AND match.status <> 'CANCELLED'
         )
"""

RECENT_ACTION = """
SELECT action.kind
         FROM auction_actions AS action
         WHERE action.guild_id = :p1
           AND action.tournament_id = :p2
           AND action.kind <> 'REVERT_LAST'
           AND NOT EXISTS (
               SELECT 1
               FROM auction_actions AS reversal
               WHERE reversal.guild_id = action.guild_id
                 AND reversal.tournament_id = action.tournament_id
                 AND reversal.kind = 'REVERT_LAST'
                 AND reversal.reverses_action_id = action.id
           )
         ORDER BY action.sequence DESC
         LIMIT 1
"""

REVERTIBLE = """
SELECT EXISTS (
             SELECT 1
             FROM (
                 SELECT kind
                 FROM auction_actions
                 WHERE guild_id = :p1 AND tournament_id = :p2
                 ORDER BY sequence DESC, id DESC
                 LIMIT 1
             ) AS latest
             WHERE latest.kind IN (
                 'DRAW_NEXT',
                 'MARK_UNSOLD',
                 'AWARD_BID',
                 'ASSIGN_CURRENT',
                 'ASSIGN_UNSOLD',
                 'RESTORE_UNSOLD'
             )
         )
"""

LOCK_TOURNAMENT = """
SELECT source_recruitment_id, display_number, title, format, final_best_of,
                hard_fearless, status, revision, bracket_locked_at
         FROM auction_tournaments
         WHERE guild_id = :p1 AND id = :p2 AND deleted_at IS NULL
         FOR UPDATE
"""

LOCK_TEAMS = """
SELECT id, team_index, team_name, initial_points, current_points, point_revision
         FROM auction_teams
         WHERE guild_id = :p1 AND tournament_id = :p2
         ORDER BY team_index
         FOR UPDATE
"""

LOCK_MEMBERS = """
SELECT id, team_id, user_id, roster_slot, acquired_via
         FROM auction_team_members
         WHERE guild_id = :p1 AND tournament_id = :p2 AND released_at IS NULL
         ORDER BY team_id, roster_slot, user_id
         FOR UPDATE
"""

LOCK_CANDIDATES = """
SELECT user_id, declared_lane, mmr_at_start, snapshot_origin, queue_position, state
         FROM auction_bid_candidates
         WHERE guild_id = :p1 AND tournament_id = :p2
         ORDER BY queue_position, user_id
         FOR UPDATE
"""

LOCK_LOTS = """
SELECT id, draw_sequence, target_user_id, status
         FROM auction_lots
         WHERE guild_id = :p1 AND tournament_id = :p2
         ORDER BY draw_sequence, id
         FOR UPDATE
"""

ACTIONS = """
SELECT id, sequence, kind, target_user_id, team_id, lot_id, points, reverses_action_id
         FROM auction_actions
         WHERE guild_id = :p1 AND tournament_id = :p2
         ORDER BY sequence, id
"""

LEDGER = """
SELECT id, team_id, point_revision, reason, source_action_id, points_before, points_after
         FROM auction_point_ledger
         WHERE guild_id = :p1 AND tournament_id = :p2
         ORDER BY team_id, point_revision, id
"""

ACTION = """
INSERT INTO auction_actions (
             guild_id, tournament_id, id, sequence, kind, target_user_id, team_id, lot_id,
             points, reverses_action_id, reversed_action_kind, actor_type, actor_id, created_at
         ) VALUES (:p1, :p2, :p3, :p4, :p5, :p6, :p7, :p8, :p9, :p10, :p11, 'USER', :p12, :p13)
"""

RANDOMIZE = """
SELECT requested.user_id
         FROM unnest(CAST(:p1 AS text[])) AS requested(user_id)
         ORDER BY random(), requested.user_id
"""

RESET_REVERSALS = """
DELETE FROM auction_point_ledger
         WHERE guild_id = :p1 AND tournament_id = :p2 AND reason = 'REVERSAL'
"""

RESET_DEBITS = """
DELETE FROM auction_point_ledger
         WHERE guild_id = :p1 AND tournament_id = :p2 AND reason = 'BID_DEBIT'
"""

RESET_MEMBERS = """
DELETE FROM auction_team_members
         WHERE guild_id = :p1 AND tournament_id = :p2 AND acquired_via <> 'CAPTAIN'
"""

RESET_REVERSAL_ACTIONS = """
DELETE FROM auction_actions
         WHERE guild_id = :p1 AND tournament_id = :p2 AND kind = 'REVERT_LAST'
"""

RESET_ACTIONS = """
DELETE FROM auction_actions
         WHERE guild_id = :p1 AND tournament_id = :p2
"""

RESET_LOTS = """
DELETE FROM auction_lots
         WHERE guild_id = :p1 AND tournament_id = :p2
"""

RESET_POINTS = """
UPDATE auction_teams
         SET current_points = initial_points, point_revision = 0
         WHERE guild_id = :p1 AND tournament_id = :p2
"""

QUEUE_OFFSET = """
UPDATE auction_bid_candidates
         SET queue_position = queue_position + :p3
         WHERE guild_id = :p1
           AND tournament_id = :p2
           AND user_id = ANY(CAST(:p4 AS text[]))
"""

QUEUE_RESET = """
WITH assignment AS (
             SELECT requested.user_id, requested.queue_position
             FROM unnest(CAST(:p3 AS text[]), CAST(:p4 AS bigint[])) AS requested(
                 user_id, queue_position)
         )
         UPDATE auction_bid_candidates AS candidate
         SET queue_position = assignment.queue_position,
             state = 'QUEUED',
             draw_count = 0,
             updated_at = :p5
         FROM assignment
         WHERE candidate.guild_id = :p1
           AND candidate.tournament_id = :p2
           AND candidate.user_id = assignment.user_id
"""

QUEUE_ASSIGN = """
WITH assignment AS (
             SELECT requested.user_id, requested.queue_position
             FROM unnest(CAST(:p3 AS text[]), CAST(:p4 AS bigint[])) AS requested(
                 user_id, queue_position)
         )
         UPDATE auction_bid_candidates AS candidate
         SET queue_position = assignment.queue_position,
             updated_at = :p5
         FROM assignment
         WHERE candidate.guild_id = :p1
           AND candidate.tournament_id = :p2
           AND candidate.user_id = assignment.user_id
"""

LOT = """
INSERT INTO auction_lots (
             guild_id, tournament_id, id, draw_sequence, target_user_id, status,
             opened_by_type, opened_by_id, opened_at, closed_by_type, closed_by_id, closed_at
         ) VALUES (:p1, :p2, :p3, :p4, :p5, 'ACTIVE', 'USER', :p6, :p7, NULL, NULL, NULL)
"""

CLEAR_SLOTS = """
UPDATE auction_bracket_slots
         SET team_id = NULL, updated_by = :p1, updated_at = :p2
         WHERE guild_id = :p3 AND tournament_id = :p4 AND team_id IS NOT NULL
"""

TRANSITION = """
UPDATE auction_bid_candidates
         SET state = :p1, updated_at = :p2
         WHERE guild_id = :p3 AND tournament_id = :p4 AND user_id = :p5 AND state = :p6
"""

DRAW = """
UPDATE auction_bid_candidates
         SET state = 'ACTIVE', draw_count = draw_count + 1, updated_at = :p1
         WHERE guild_id = :p2 AND tournament_id = :p3 AND user_id = :p4 AND state = 'QUEUED'
"""

CLOSE_LOT = """
UPDATE auction_lots
         SET status = :p1, closed_by_type = 'USER', closed_by_id = :p2, closed_at = :p3
         WHERE guild_id = :p4 AND tournament_id = :p5 AND id = :p6 AND status = 'ACTIVE'
"""

REVERT_LOT = """
UPDATE auction_lots
         SET status = 'REVERTED'
         WHERE guild_id = :p1 AND tournament_id = :p2 AND id = :p3 AND status <> 'ACTIVE'
"""

MEMBER = """
INSERT INTO auction_team_members (
             guild_id, tournament_id, team_id, id, user_id, roster_slot, acquired_via,
             declared_lane, mmr_at_acquisition, snapshot_origin, acquisition_action_id,
             acquisition_action_kind, joined_at, released_at
         ) VALUES (:p1, :p2, :p3, :p4, :p5, :p6, :p7, :p8, :p9, :p10, :p11, :p12, :p13, NULL)
"""

RELEASE_MEMBER_READ = """
SELECT id, team_id, user_id, roster_slot, acquisition_action_id, acquired_via
         FROM auction_team_members
         WHERE guild_id = :p1
           AND tournament_id = :p2
           AND acquisition_action_id = :p3
           AND user_id = :p4
           AND released_at IS NULL
         FOR UPDATE
"""

RELEASE_MEMBER = """
UPDATE auction_team_members
         SET released_at = :p1
         WHERE guild_id = :p2 AND tournament_id = :p3 AND id = :p4 AND released_at IS NULL
"""

BUMP = """
UPDATE auction_tournaments
         SET status = :p1, revision = revision + 1, updated_by = :p2, updated_at = :p3
         WHERE guild_id = :p4 AND id = :p5 AND deleted_at IS NULL
         RETURNING revision
"""

POINTS = """
UPDATE auction_teams
         SET current_points = :p1, point_revision = :p2
         WHERE guild_id = :p3
           AND tournament_id = :p4
           AND id = :p5
           AND current_points = :p6
           AND point_revision = :p7
"""

DEBIT = """
INSERT INTO auction_point_ledger (
             guild_id, tournament_id, id, team_id, point_revision, reason,
             source_action_id, source_action_kind, points_before, points_after, delta,
             reverses_entry_id, reversed_reason, actor_type, actor_id, created_at
         ) VALUES (:p1, :p2, :p3, :p4, :p5, 'BID_DEBIT', :p6, 'AWARD_BID', :p7, :p8, :p9,
                   NULL, NULL, 'USER', :p10, :p11)
"""

REVERSAL = """
INSERT INTO auction_point_ledger (
             guild_id, tournament_id, id, team_id, point_revision, reason,
             source_action_id, source_action_kind, points_before, points_after, delta,
             reverses_entry_id, reversed_reason, actor_type, actor_id, created_at
         ) VALUES (:p1, :p2, :p3, :p4, :p5, 'REVERSAL', :p6, 'REVERT_LAST', :p7, :p8, :p9,
                   :p10, 'BID_DEBIT', 'USER', :p11, :p12)
"""

COUNTER = """
UPDATE guild_counters
         SET next_value = next_value + 1
         WHERE guild_id = :p1 AND counter_key = 'AUCTION_TOURNAMENT'
         RETURNING next_value - 1
"""

CHAMPION_RECORDS = """
WITH eligible AS (
    SELECT game.id,
           game.played_at,
           participant.lane,
           participant.champion_id,
           (
               (participant.team = 'TEAM_1' AND (
                   (game.side_preset = 'DEFAULT' AND game.winning_side = 'BLUE') OR
                   (game.side_preset = 'SWAPPED' AND game.winning_side = 'RED')
               )) OR
               (participant.team = 'TEAM_2' AND (
                   (game.side_preset = 'DEFAULT' AND game.winning_side = 'RED') OR
                   (game.side_preset = 'SWAPPED' AND game.winning_side = 'BLUE')
               ))
           ) AS won
    FROM games AS game
    JOIN game_participants AS participant
      ON participant.guild_id = game.guild_id
     AND participant.game_id = game.id
     AND participant.user_id = :p3
    WHERE game.guild_id = :p1
      AND game.season_id = :p2
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
)
         SELECT champion_id,
                count(*) AS games,
                count(*) FILTER (WHERE won) AS wins
         FROM eligible
         WHERE champion_id IS NOT NULL
         GROUP BY champion_id
         ORDER BY count(*) DESC, count(*) FILTER (WHERE won) DESC, champion_id
         LIMIT 3
"""

RECENT_GAMES = """
WITH eligible AS (
    SELECT game.id,
           game.played_at,
           participant.lane,
           participant.champion_id,
           (
               (participant.team = 'TEAM_1' AND (
                   (game.side_preset = 'DEFAULT' AND game.winning_side = 'BLUE') OR
                   (game.side_preset = 'SWAPPED' AND game.winning_side = 'RED')
               )) OR
               (participant.team = 'TEAM_2' AND (
                   (game.side_preset = 'DEFAULT' AND game.winning_side = 'RED') OR
                   (game.side_preset = 'SWAPPED' AND game.winning_side = 'BLUE')
               ))
           ) AS won
    FROM games AS game
    JOIN game_participants AS participant
      ON participant.guild_id = game.guild_id
     AND participant.game_id = game.id
     AND participant.user_id = :p3
    WHERE game.guild_id = :p1
      AND game.season_id = :p2
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
)
         SELECT id, played_at, lane, champion_id, won
         FROM eligible
         ORDER BY played_at DESC, id DESC
         LIMIT 8
"""

LIST = """
WITH page AS (
             SELECT tournament.guild_id,
                    tournament.id,
                    tournament.display_number,
                    tournament.title,
                    tournament.format,
                    tournament.status,
                    tournament.revision,
                    tournament.champion_team_id,
                    tournament.updated_at,
                    tournament.ended_at
             FROM auction_tournaments AS tournament
             JOIN guilds AS guild
               ON guild.guild_id = tournament.guild_id
              AND guild.status = 'ACTIVE'
             WHERE tournament.guild_id = :p2
               AND (
                   CAST(:p1 AS text) IS NULL
                   OR EXISTS (
                       SELECT 1
                       FROM guild_members AS viewer
                       WHERE viewer.guild_id = tournament.guild_id
                         AND viewer.user_id = :p1
                         AND viewer.status = 'ACTIVE'
                   )
               )
               AND (
                   (:p3 = 'active' AND tournament.status IN ('BIDDING', 'BRACKET_SETUP', 'IN_GAME'))
                   OR (:p3 = 'completed' AND tournament.status IN ('COMPLETED', 'CANCELLED'))
               )
               AND tournament.deleted_at IS NULL
               AND (
                   CAST(:p4 AS uuid) IS NULL OR tournament.season_id = :p4
               )
               AND (
                   CAST(:p5 AS timestamptz) IS NULL
                   OR (tournament.updated_at, tournament.id) < (:p5, CAST(:p6 AS uuid))
               )
             ORDER BY tournament.updated_at DESC, tournament.id DESC
             LIMIT :p7
         )
         SELECT page.id,
                page.display_number,
                page.title,
                page.format,
                page.status,
                page.revision,
                page.updated_at,
                page.ended_at,
                member_count.active_count AS active_member_count,
                member_count.bid_count AS bid_member_count,
                match_count.count AS incomplete_match_count,
                page.champion_team_id,
                champion.team_name AS champion_team_name,
                next_match.id AS next_match_id,
                next_match.match_key AS next_match_key,
                next_match.status AS next_match_status,
                next_match.confirmed_game_count AS next_match_confirmed_game_count
         FROM page
         CROSS JOIN LATERAL (
             SELECT count(*) FILTER (WHERE member.released_at IS NULL) AS active_count,
                    count(*) FILTER (
                        WHERE member.released_at IS NULL AND member.acquired_via = 'BID'
                    ) AS bid_count
             FROM auction_team_members AS member
             WHERE member.guild_id = page.guild_id
               AND member.tournament_id = page.id
         ) AS member_count
         CROSS JOIN LATERAL (
             SELECT count(*) AS count
             FROM auction_matches AS auction_match
             WHERE auction_match.guild_id = page.guild_id
               AND auction_match.tournament_id = page.id
               AND auction_match.deleted_at IS NULL
               AND auction_match.status NOT IN ('COMPLETED', 'CANCELLED')
         ) AS match_count
         LEFT JOIN auction_teams AS champion
           ON champion.guild_id = page.guild_id
          AND champion.tournament_id = page.id
          AND champion.id = page.champion_team_id
         LEFT JOIN LATERAL (
             SELECT auction_match.id,
                    auction_match.match_key,
                    auction_match.status,
                    (
                        SELECT count(*)
                        FROM games AS game
                        WHERE game.guild_id = auction_match.guild_id
                          AND game.auction_match_id = auction_match.id
                          AND game.status = 'CONFIRMED'
                    ) AS confirmed_game_count
             FROM auction_matches AS auction_match
             WHERE auction_match.guild_id = page.guild_id
               AND auction_match.tournament_id = page.id
               AND auction_match.deleted_at IS NULL
               AND auction_match.status NOT IN ('COMPLETED', 'CANCELLED')
             ORDER BY CASE auction_match.status
                          WHEN 'IN_PROGRESS' THEN 0
                          WHEN 'READY' THEN 1
                          ELSE 2
                      END,
                      CASE auction_match.match_key
                          WHEN 'SEMIFINAL_A' THEN 0
                          WHEN 'SEMIFINAL_B' THEN 1
                          ELSE 2
                      END,
                      auction_match.id
             LIMIT 1
         ) AS next_match ON TRUE
         ORDER BY page.updated_at DESC, page.id DESC
"""
