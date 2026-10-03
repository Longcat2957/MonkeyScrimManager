"""SQL ported from the current Axum entry draft projection."""

RECRUITMENT = """
SELECT recruitment.id,
                recruitment.season_id,
                recruitment.display_number,
                recruitment.title,
                recruitment.status,
                recruitment.target_count,
                recruitment.best_of,
                recruitment.hard_fearless,
                recruitment.revision,
                recruitment.updated_at,
                recruitment.converted_at,
                recruitment.created_by,
                draft.revision AS draft_revision,
                draft.updated_at AS draft_updated_at,
                draft.updated_by AS draft_updated_by,
                editor.display_name AS draft_updated_by_display_name
         FROM recruitments AS recruitment
         JOIN recruitment_entry_drafts AS draft
           ON draft.guild_id = recruitment.guild_id
          AND draft.recruitment_id = recruitment.id
         LEFT JOIN guild_members AS editor
           ON editor.guild_id = draft.guild_id
          AND editor.user_id = draft.updated_by
         WHERE recruitment.guild_id = :guild AND recruitment.id = :id
"""

PARTICIPANTS = """
SELECT participant.user_id AS id,
                member.display_name,
                member.main_lane,
                COALESCE(mmr.mmr, 1500::double precision) AS mmr,
                COALESCE(mmr.games_played, 0) AS games,
                COALESCE(mmr.wins, 0) AS wins,
                solo_rank.tier AS solo_rank_tier,
                solo_rank.division AS solo_rank_division,
                account.profile_icon_id
         FROM recruitment_participants AS participant
         JOIN guild_members AS member
           ON member.guild_id = participant.guild_id
          AND member.user_id = participant.user_id
         JOIN recruitments AS recruitment
           ON recruitment.guild_id = participant.guild_id
          AND recruitment.id = participant.recruitment_id
         LEFT JOIN recruitment_entry_assignments AS assignment
           ON assignment.guild_id = participant.guild_id
          AND assignment.recruitment_id = participant.recruitment_id
          AND assignment.user_id = participant.user_id
         LEFT JOIN LATERAL (
             SELECT state.mmr, state.games_played, state.wins
             FROM user_lane_mmr AS state
             WHERE state.guild_id = participant.guild_id
               AND state.season_id = recruitment.season_id
               AND state.user_id = participant.user_id
               AND state.lane = COALESCE(
                   assignment.lane,
                   member.main_lane,
                   (
                       SELECT preferred.lane
                       FROM recruitment_preferred_lanes AS preferred
                       WHERE preferred.guild_id = participant.guild_id
                         AND preferred.recruitment_id = participant.recruitment_id
                         AND preferred.user_id = participant.user_id
                       ORDER BY preferred.priority
                       LIMIT 1
                   )
               )
             LIMIT 1
         ) AS mmr ON TRUE
         LEFT JOIN LATERAL (
             SELECT riot.platform, riot.puuid, riot.profile_icon_id
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
         WHERE participant.guild_id = :guild
           AND participant.recruitment_id = :id
           AND participant.status = 'JOINED'
         ORDER BY participant.joined_at, participant.user_id
"""

LANES = """
SELECT preferred.user_id, preferred.lane
         FROM recruitment_preferred_lanes AS preferred
         JOIN recruitment_participants AS participant
           ON participant.guild_id = preferred.guild_id
          AND participant.recruitment_id = preferred.recruitment_id
          AND participant.user_id = preferred.user_id
          AND participant.status = 'JOINED'
         WHERE preferred.guild_id = :guild AND preferred.recruitment_id = :id
         ORDER BY preferred.user_id, preferred.priority
"""

CHAMPIONS = """
SELECT preference.user_id, preference.champion_id
         FROM user_champion_preferences AS preference
         JOIN recruitment_participants AS participant
           ON participant.user_id = preference.user_id
          AND participant.guild_id = :guild
          AND participant.recruitment_id = :id
          AND participant.status = 'JOINED'
         WHERE preference.user_id IN (
             SELECT joined.user_id
             FROM recruitment_participants AS joined
             WHERE joined.guild_id = :guild
               AND joined.recruitment_id = :id
               AND joined.status = 'JOINED'
         )
         ORDER BY preference.user_id,
                  CASE preference.lane
                      WHEN 'TOP' THEN 1
                      WHEN 'JUNGLE' THEN 2
                      WHEN 'MID' THEN 3
                      WHEN 'BOTTOM' THEN 4
                      WHEN 'SUPPORT' THEN 5
                  END,
                  preference.position
"""

ASSIGNMENTS = """
SELECT assignment.user_id AS participant_id, assignment.team, assignment.lane
         FROM recruitment_entry_assignments AS assignment
         JOIN recruitment_participants AS participant
           ON participant.guild_id = assignment.guild_id
          AND participant.recruitment_id = assignment.recruitment_id
          AND participant.user_id = assignment.user_id
          AND participant.status = 'JOINED'
         WHERE assignment.guild_id = :guild AND assignment.recruitment_id = :id
         ORDER BY assignment.team,
                  CASE assignment.lane
                      WHEN 'TOP' THEN 1
                      WHEN 'JUNGLE' THEN 2
                      WHEN 'MID' THEN 3
                      WHEN 'BOTTOM' THEN 4
                      WHEN 'SUPPORT' THEN 5
                  END
"""

HEAD_TO_HEAD = """
SELECT ledger.lane,
                ledger.user_id AS user_a_id,
                ledger.opponent_user_id AS user_b_id,
                COUNT(*) AS plays,
                SUM(ledger.wins_delta)::bigint AS user_a_wins,
                (COUNT(*) - SUM(ledger.wins_delta))::bigint AS user_b_wins
         FROM mmr_ledger_entries AS ledger
         JOIN games AS game
           ON game.guild_id = ledger.guild_id
          AND game.id = ledger.game_id
          AND game.status = 'CONFIRMED'
          AND game.series_id IS NOT NULL
         JOIN recruitment_participants AS user_a
           ON user_a.guild_id = ledger.guild_id
          AND user_a.recruitment_id = :id
          AND user_a.user_id = ledger.user_id
          AND user_a.status = 'JOINED'
         JOIN recruitment_participants AS user_b
           ON user_b.guild_id = ledger.guild_id
          AND user_b.recruitment_id = :id
          AND user_b.user_id = ledger.opponent_user_id
          AND user_b.status = 'JOINED'
         WHERE ledger.guild_id = :guild
           AND ledger.season_id = :season
           AND ledger.source_type = 'GAME_RESULT'
           AND ledger.user_id < ledger.opponent_user_id
         GROUP BY ledger.lane, ledger.user_id, ledger.opponent_user_id
         ORDER BY CASE ledger.lane
                      WHEN 'TOP' THEN 1
                      WHEN 'JUNGLE' THEN 2
                      WHEN 'MID' THEN 3
                      WHEN 'BOTTOM' THEN 4
                      WHEN 'SUPPORT' THEN 5
                  END,
                  ledger.user_id,
                  ledger.opponent_user_id
"""
