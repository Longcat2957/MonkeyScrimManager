"""Recruitment projection SQL ported from the frozen Axum workflow repository."""

ACTIONABLE_SQL = """
WITH page AS (
             SELECT recruitment.guild_id,
                    recruitment.id,
                    recruitment.display_number,
                    recruitment.title,
                    recruitment.status,
                    recruitment.target_count,
                    recruitment.best_of,
                    recruitment.hard_fearless,
                    recruitment.revision,
                    recruitment.updated_at
             FROM recruitments AS recruitment
             JOIN guilds AS guild
               ON guild.guild_id = recruitment.guild_id
              AND guild.status = 'ACTIVE'
             JOIN guild_members AS viewer
               ON viewer.guild_id = recruitment.guild_id
              AND viewer.user_id = :user_id
              AND viewer.status = 'ACTIVE'
             WHERE recruitment.guild_id = :guild_id
               AND recruitment.status IN ('OPEN', 'CLOSED')
               AND recruitment.converted_at IS NULL
               AND NOT EXISTS (
                   SELECT 1
                   FROM series AS existing_series
                   WHERE existing_series.guild_id = recruitment.guild_id
                     AND existing_series.source_recruitment_id = recruitment.id
                     AND existing_series.status <> 'CANCELLED'
                     AND existing_series.deleted_at IS NULL
               )
               AND (
                   CAST(:cursor_time AS timestamptz) IS NULL
                   OR (recruitment.updated_at, recruitment.id)
                      < (:cursor_time, CAST(:cursor_id AS uuid))
               )
             ORDER BY recruitment.updated_at DESC, recruitment.id DESC
             LIMIT :fetch_limit
         )
         SELECT page.id,
                page.display_number,
                page.title,
                page.status,
                page.target_count,
                page.best_of,
                page.hard_fearless,
                page.revision,
                page.updated_at,
                joined.count AS participant_count,
                assigned.count AS assigned_count
         FROM page
         CROSS JOIN LATERAL (
             SELECT count(*) AS count
             FROM recruitment_participants AS participant
             WHERE participant.guild_id = page.guild_id
               AND participant.recruitment_id = page.id
               AND participant.status = 'JOINED'
         ) AS joined
         CROSS JOIN LATERAL (
             SELECT count(*) AS count
             FROM recruitment_entry_assignments AS assignment
             JOIN recruitment_participants AS participant
               ON participant.guild_id = assignment.guild_id
              AND participant.recruitment_id = assignment.recruitment_id
              AND participant.user_id = assignment.user_id
              AND participant.status = 'JOINED'
             WHERE assignment.guild_id = page.guild_id
               AND assignment.recruitment_id = page.id
         ) AS assigned
         ORDER BY page.updated_at DESC, page.id DESC
"""
