"""SQL copied from the frozen Rust auction recruitment workflow."""

SEASON = """
SELECT id
         FROM seasons
         WHERE guild_id = :p1 AND status = 'ACTIVE'
"""

COUNTER = """
UPDATE guild_counters
         SET next_value = next_value + 1
         WHERE guild_id = :p1 AND counter_key = 'AUCTION_RECRUITMENT'
         RETURNING next_value - 1
"""

INSERT_RECRUITMENT = """
INSERT INTO auction_recruitments (
             guild_id, id, season_id, display_number, title, format, target_count,
             semifinal_best_of, final_best_of, hard_fearless, status, revision, channel_id,
             message_id, created_by, created_at, updated_by, updated_at
         ) VALUES (:p1, :p2, :p3, :p4, :p5, :p6, :p7, :p8, :p9, :p10, 'OPEN', 0,
             NULL, NULL, :p11, :p12, :p11, :p12)
"""

INSERT_DRAFT = """
INSERT INTO auction_setup_drafts (
             guild_id, recruitment_id, revision, updated_by, updated_at
         ) VALUES (:p1, :p2, 0, :p3, :p4)
"""

INSERT_TEAM = """
INSERT INTO auction_setup_teams (
                 guild_id, recruitment_id, team_index, team_name, captain_user_id, initial_points
             ) VALUES (:p1, :p2, :p3, :p4, NULL, :p5)
"""

SNAPSHOT = """
SELECT recruitment.display_number,
                recruitment.title,
                recruitment.format,
                recruitment.target_count,
                recruitment.semifinal_best_of,
                recruitment.final_best_of,
                recruitment.hard_fearless,
                recruitment.status,
                recruitment.revision,
                recruitment.channel_id,
                recruitment.message_id,
                recruitment.created_by,
                recruitment.updated_at,
                count(participant.user_id) FILTER (WHERE participant.status = 'JOINED')
                    AS joined_participant_count
         FROM auction_recruitments AS recruitment
         LEFT JOIN auction_recruitment_participants AS participant
           ON participant.guild_id = recruitment.guild_id
          AND participant.recruitment_id = recruitment.id
         WHERE recruitment.guild_id = :p1 AND recruitment.id = :p2
         GROUP BY recruitment.id, recruitment.display_number, recruitment.title, recruitment.format,
                  recruitment.target_count, recruitment.semifinal_best_of,
                  recruitment.final_best_of, recruitment.hard_fearless, recruitment.status,
                  recruitment.revision,
                  recruitment.channel_id, recruitment.message_id, recruitment.created_by,
                  recruitment.updated_at
"""

PARTICIPANTS = """
SELECT participant.user_id AS id, member.display_name, participant.declared_lane
         FROM auction_recruitment_participants AS participant
         JOIN guild_members AS member
           ON member.guild_id = participant.guild_id
          AND member.user_id = participant.user_id
         WHERE participant.guild_id = :p1
           AND participant.recruitment_id = :p2
           AND participant.status = 'JOINED'
         ORDER BY participant.joined_at, participant.user_id
"""

LINKED_TOURNAMENT = """
SELECT id
             FROM auction_tournaments
             WHERE guild_id = :p1
               AND source_recruitment_id = :p2
               AND deleted_at IS NULL
             ORDER BY started_at DESC, id DESC
             LIMIT 1
"""

DRAFT_RECRUITMENT = """
SELECT season_id, format, status, closed_at, target_count, semifinal_best_of, final_best_of,
                hard_fearless, revision, created_by
         FROM auction_recruitments
         WHERE guild_id = :p1 AND id = :p2
"""

DRAFT_REVISION = """
SELECT revision
         FROM auction_setup_drafts
         WHERE guild_id = :p1 AND recruitment_id = :p2
"""

TEAMS = """
SELECT team_index, team_name, captain_user_id, initial_points
         FROM auction_setup_teams
         WHERE guild_id = :p1 AND recruitment_id = :p2
         ORDER BY team_index
"""

DRAFT_PARTICIPANTS = """
SELECT participant.user_id AS id, member.display_name, participant.declared_lane
         FROM auction_recruitment_participants AS participant
         JOIN guild_members AS member
           ON member.guild_id = participant.guild_id
          AND member.user_id = participant.user_id
         WHERE participant.guild_id = :p1
           AND participant.recruitment_id = :p2
           AND participant.status = 'JOINED'
         ORDER BY participant.joined_at, participant.user_id
"""

LOCK_RECRUITMENT = """
SELECT season_id, format, status, closed_at, target_count, semifinal_best_of, final_best_of,
                hard_fearless, revision, created_by
         FROM auction_recruitments
         WHERE guild_id = :p1 AND id = :p2
         FOR UPDATE
"""

LOCK_DRAFT = """
SELECT revision
         FROM auction_setup_drafts
         WHERE guild_id = :p1 AND recruitment_id = :p2
         FOR UPDATE
"""

LOCK_TEAMS = """
SELECT team_index, team_name, captain_user_id, initial_points
         FROM auction_setup_teams
         WHERE guild_id = :p1 AND recruitment_id = :p2
         ORDER BY team_index
         FOR UPDATE
"""

LOCK_PARTICIPANTS = """
SELECT user_id, declared_lane, mmr_at_join, snapshot_origin
         FROM auction_recruitment_participants
         WHERE guild_id = :p1 AND recruitment_id = :p2 AND status = 'JOINED'
         ORDER BY joined_at, user_id
         FOR UPDATE
"""

OPERATOR = """
SELECT EXISTS (
             SELECT 1
             FROM guild_members AS member
             JOIN guild_config AS config
               ON config.guild_id = member.guild_id
             JOIN guild_member_discord_roles AS role
               ON role.guild_id = member.guild_id
              AND role.user_id = member.user_id
              AND role.role_id = config.operator_role_id
             WHERE member.guild_id = :p1
               AND member.user_id = :p2
               AND member.status = 'ACTIVE'
         )
"""

PARTICIPANT_STATUS = """
SELECT status
         FROM auction_recruitment_participants
         WHERE guild_id = :p1 AND recruitment_id = :p2 AND user_id = :p3
         FOR UPDATE
"""

JOIN = """
INSERT INTO auction_recruitment_participants (
             guild_id, recruitment_id, user_id, status, declared_lane, mmr_at_join,
             snapshot_origin, joined_at, withdrawn_at
         ) VALUES (:p1, :p2, :p3, 'JOINED', :p4, :p5, 'CAPTURED', :p6, NULL)
         ON CONFLICT (guild_id, recruitment_id, user_id) DO UPDATE
         SET status = 'JOINED',
             declared_lane = EXCLUDED.declared_lane,
             mmr_at_join = EXCLUDED.mmr_at_join,
             snapshot_origin = 'CAPTURED',
             joined_at = EXCLUDED.joined_at,
             withdrawn_at = NULL
"""

LEAVE = """
UPDATE auction_recruitment_participants
         SET status = 'WITHDRAWN', withdrawn_at = :p1
         WHERE guild_id = :p2
           AND recruitment_id = :p3
           AND user_id = :p4
           AND status = 'JOINED'
"""

CLEAR_CAPTAIN = """
UPDATE auction_setup_teams
         SET captain_user_id = NULL
         WHERE guild_id = :p1 AND recruitment_id = :p2 AND captain_user_id = :p3
"""

BUMP_RECRUITMENT = """
UPDATE auction_recruitments
         SET revision = revision + 1, updated_by = :p1, updated_at = :p2
         WHERE guild_id = :p3 AND id = :p4
"""

BUMP_DRAFT = """
UPDATE auction_setup_drafts
         SET revision = revision + 1, updated_by = :p1, updated_at = :p2
         WHERE guild_id = :p3 AND recruitment_id = :p4
"""

CLOSE = """
UPDATE auction_recruitments
                 SET status = 'CLOSED',
                     revision = revision + 1,
                     updated_by = :p1,
                     updated_at = :p2,
                     closed_at = :p2
                 WHERE guild_id = :p3 AND id = :p4
"""

REOPEN = """
UPDATE auction_recruitments
                 SET status = 'OPEN',
                     revision = revision + 1,
                     updated_by = :p1,
                     updated_at = :p2,
                     closed_at = NULL
                 WHERE guild_id = :p3
                   AND id = :p4
                   AND revision = :p5
                   AND status = 'CLOSED'
"""

CANCEL = """
UPDATE auction_recruitments
                 SET status = 'CANCELLED',
                     revision = revision + 1,
                     updated_by = :p1,
                     updated_at = :p2,
                     cancelled_at = :p2
                 WHERE guild_id = :p3
                   AND id = :p4
                   AND revision = :p5
                   AND status IN ('OPEN', 'CLOSED')
"""

RESTORE = """
UPDATE auction_recruitments
                 SET status = :p1,
                     revision = revision + 1,
                     updated_by = :p2,
                     updated_at = :p3,
                     converted_at = NULL,
                     cancelled_at = NULL,
                     message_id = NULL
                 WHERE guild_id = :p4
                   AND id = :p5
                   AND revision = :p6
                   AND status = 'CANCELLED'
"""

UPDATE_TEAM = """
UPDATE auction_setup_teams
             SET team_name = :p1, captain_user_id = :p2, initial_points = :p3
             WHERE guild_id = :p4 AND recruitment_id = :p5 AND team_index = :p6
"""

UPDATE_DRAFT = """
UPDATE auction_setup_drafts
         SET revision = revision + 1, updated_by = :p1, updated_at = :p2
         WHERE guild_id = :p3 AND recruitment_id = :p4
         RETURNING revision
"""

LOCK_GUILD = """
SELECT status
         FROM guilds
         WHERE guild_id = :p1
         FOR UPDATE
"""

LOCK_CONFIG = """
SELECT operator_role_id
         FROM guild_config
         WHERE guild_id = :p1
         FOR UPDATE
"""

MAIN_LANE = """
SELECT main_lane
         FROM guild_members
         WHERE guild_id = :p1 AND user_id = :p2 AND status = 'ACTIVE'
         FOR UPDATE
"""

LIST = """
WITH page AS (
             SELECT recruitment.guild_id,
                    recruitment.id,
                    recruitment.display_number,
                    recruitment.title,
                    recruitment.format,
                    recruitment.target_count,
                    recruitment.semifinal_best_of,
                    recruitment.final_best_of,
                    recruitment.hard_fearless,
                    recruitment.status,
                    recruitment.revision,
                    recruitment.updated_at
             FROM auction_recruitments AS recruitment
             JOIN guilds AS guild
               ON guild.guild_id = recruitment.guild_id
              AND guild.status = 'ACTIVE'
             JOIN guild_members AS viewer
               ON viewer.guild_id = recruitment.guild_id
              AND viewer.user_id = :p1
              AND viewer.status = 'ACTIVE'
             WHERE recruitment.guild_id = :p2
               AND recruitment.status IN ('OPEN', 'CLOSED')
               AND recruitment.converted_at IS NULL
               AND NOT EXISTS (
                   SELECT 1
                   FROM auction_tournaments AS existing_tournament
                   WHERE existing_tournament.guild_id = recruitment.guild_id
                     AND existing_tournament.source_recruitment_id = recruitment.id
                     AND existing_tournament.status <> 'CANCELLED'
                     AND existing_tournament.deleted_at IS NULL
               )
               AND (
                   CAST(:p3 AS timestamptz) IS NULL
                   OR (recruitment.updated_at, recruitment.id) < (:p3, CAST(:p4 AS uuid))
               )
             ORDER BY recruitment.updated_at DESC, recruitment.id DESC
             LIMIT :p5
         )
         SELECT page.id,
                page.display_number,
                page.title,
                page.format,
                page.target_count,
                page.semifinal_best_of,
                page.final_best_of,
                page.hard_fearless,
                page.status,
                page.revision,
                page.updated_at,
                joined.count AS participant_count
         FROM page
         CROSS JOIN LATERAL (
             SELECT count(*) AS count
             FROM auction_recruitment_participants AS participant
             WHERE participant.guild_id = page.guild_id
               AND participant.recruitment_id = page.id
               AND participant.status = 'JOINED'
         ) AS joined
         ORDER BY page.updated_at DESC, page.id DESC
"""
