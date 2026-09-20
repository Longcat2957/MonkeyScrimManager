"""SQL projections ported from the frozen Axum series implementation."""

TEAMS = """
SELECT participant.team,
                participant.user_id AS id,
                participant.lane,
                participant.mmr_at_lock AS mmr,
                member.display_name,
                solo_rank.tier AS solo_rank_tier,
                solo_rank.division AS solo_rank_division
         FROM series_participants AS participant
         JOIN guild_members AS member
           ON member.guild_id = participant.guild_id
          AND member.user_id = participant.user_id
         LEFT JOIN LATERAL (
             SELECT riot.platform, riot.puuid
             FROM riot_accounts AS riot
             WHERE riot.user_id = participant.user_id
               AND riot.is_main = true
               AND riot.origin = 'RIOT'
             ORDER BY riot.updated_at DESC
             LIMIT 1
         ) AS account ON TRUE
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
         WHERE participant.guild_id = :p1 AND participant.series_id = :p2
         ORDER BY CASE participant.team
                      WHEN 'TEAM_1' THEN 1
                      WHEN 'TEAM_2' THEN 2
                  END,
                  CASE participant.lane
                      WHEN 'TOP' THEN 1
                      WHEN 'JUNGLE' THEN 2
                      WHEN 'MID' THEN 3
                      WHEN 'BOTTOM' THEN 4
                      WHEN 'SUPPORT' THEN 5
                  END
"""

CHAMPIONS = """
SELECT preference.user_id, preference.champion_id
         FROM user_champion_preferences AS preference
         JOIN series_participants AS participant
           ON participant.user_id = preference.user_id
          AND participant.guild_id = :p1
          AND participant.series_id = :p2
          AND participant.lane = preference.lane
         ORDER BY preference.user_id,
                  preference.position
"""

LIST = """
WITH page AS (
             SELECT scrim.guild_id,
                    scrim.id,
                    scrim.display_number,
                    scrim.title,
                    scrim.best_of,
                    scrim.hard_fearless,
                    scrim.status,
                    scrim.winning_team,
                    scrim.terminal_reason,
                    scrim.updated_at,
                    scrim.ended_at
             FROM series AS scrim
             JOIN guilds AS guild
               ON guild.guild_id = scrim.guild_id
              AND guild.status = 'ACTIVE'
             JOIN guild_members AS viewer
               ON viewer.guild_id = scrim.guild_id
              AND viewer.user_id = :p1
              AND viewer.status = 'ACTIVE'
             WHERE scrim.guild_id = :p2
               AND (
                   (:p3 = 'active' AND scrim.status = 'IN_PROGRESS')
                   OR (:p3 = 'completed' AND scrim.status = 'COMPLETED')
               )
               AND scrim.deleted_at IS NULL
               AND (
                   CAST(:p4 AS timestamptz) IS NULL
                   OR (scrim.updated_at, scrim.id) < (:p4, CAST(:p5 AS uuid))
               )
             ORDER BY scrim.updated_at DESC, scrim.id DESC
             LIMIT :p6
         )
         SELECT page.id,
                page.display_number,
                page.title,
                page.best_of,
                page.hard_fearless,
                page.status,
                page.winning_team,
                page.terminal_reason,
                page.updated_at,
                page.ended_at,
                score.team_1_score,
                score.team_2_score
         FROM page
         CROSS JOIN LATERAL (
             SELECT count(*) FILTER (
                        WHERE (game.side_preset = 'DEFAULT' AND game.winning_side = 'BLUE')
                           OR (game.side_preset = 'SWAPPED' AND game.winning_side = 'RED')
                    ) AS team_1_score,
                    count(*) FILTER (
                        WHERE (game.side_preset = 'DEFAULT' AND game.winning_side = 'RED')
                           OR (game.side_preset = 'SWAPPED' AND game.winning_side = 'BLUE')
                    ) AS team_2_score
             FROM games AS game
             WHERE game.guild_id = page.guild_id
               AND game.series_id = page.id
               AND game.status = 'CONFIRMED'
         ) AS score
         ORDER BY page.updated_at DESC, page.id DESC
"""
