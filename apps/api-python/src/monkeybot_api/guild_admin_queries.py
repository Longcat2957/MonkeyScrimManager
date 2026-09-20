"""Queries preserved from Rust guild_admin_settings and guild_seasons."""

CATALOG_LOCK = """
SELECT true
         FROM guilds AS guild
         JOIN guild_config AS config ON config.guild_id = guild.guild_id
         WHERE guild.guild_id = :p1 AND guild.status = 'ACTIVE'
         FOR SHARE OF guild
         FOR UPDATE OF config
"""

CLOCK = """
SELECT clock_timestamp()
"""

DELETE_ROLES = """
DELETE FROM guild_discord_roles WHERE guild_id = :p1
"""

DELETE_CHANNELS = """
DELETE FROM guild_discord_channels WHERE guild_id = :p1
"""

CATALOG_STATE = """
INSERT INTO guild_discord_catalog_state (guild_id, synced_at)
         VALUES (:p1, :p2)
         ON CONFLICT (guild_id) DO UPDATE SET synced_at = EXCLUDED.synced_at
"""

SETTINGS_UPDATE = """
UPDATE guild_config
         SET display_name = :p1,
             operator_role_id = :p2,
             recruitment_channel_id = :p3,
             entry_channel_id = :p4,
             result_channel_id = :p5,
             log_channel_id = :p6,
             is_externally_public = :p7,
             revision = :p8,
             updated_by = :p9,
             updated_at = :p10
         WHERE guild_id = :p11 AND revision = :p12
"""

AUTHORIZE = """
SELECT true
         FROM guilds AS guild
         JOIN guild_members AS actor
           ON actor.guild_id = guild.guild_id
          AND actor.user_id = :p2
          AND actor.status = 'ACTIVE'
         JOIN guild_config AS config ON config.guild_id = guild.guild_id
         JOIN guild_member_discord_roles AS actor_role
           ON actor_role.guild_id = actor.guild_id
          AND actor_role.user_id = actor.user_id
          AND actor_role.role_id = config.operator_role_id
         WHERE guild.guild_id = :p1 AND guild.status = 'ACTIVE'
         FOR SHARE OF guild, actor, config, actor_role
"""

SETTINGS_LOCK = """
SELECT config.display_name,
                config.operator_role_id,
                config.recruitment_channel_id,
                config.entry_channel_id,
                config.result_channel_id,
                config.log_channel_id,
                config.is_externally_public,
                config.revision,
                config.updated_at,
                config.updated_by
         FROM guilds AS guild
         JOIN guild_members AS actor
           ON actor.guild_id = guild.guild_id
          AND actor.user_id = :p2
          AND actor.status = 'ACTIVE'
         JOIN guild_config AS config ON config.guild_id = guild.guild_id
         JOIN guild_member_discord_roles AS actor_role
           ON actor_role.guild_id = actor.guild_id
          AND actor_role.user_id = actor.user_id
          AND actor_role.role_id = config.operator_role_id
         WHERE guild.guild_id = :p1 AND guild.status = 'ACTIVE'
         FOR SHARE OF guild, actor, actor_role
         FOR UPDATE OF config
"""

MEMBER = """
SELECT true
         FROM guilds AS guild
         JOIN guild_members AS actor
           ON actor.guild_id = guild.guild_id
          AND actor.user_id = :p2
          AND actor.status = 'ACTIVE'
         WHERE guild.guild_id = :p1 AND guild.status = 'ACTIVE'
"""

SETTINGS_GET = """
SELECT config.display_name,
                config.operator_role_id,
                config.recruitment_channel_id,
                config.entry_channel_id,
                config.result_channel_id,
                config.log_channel_id,
                config.is_externally_public,
                config.revision,
                config.updated_at,
                config.updated_by
         FROM guilds AS guild
         JOIN guild_config AS config ON config.guild_id = guild.guild_id
         WHERE guild.guild_id = :p1 AND guild.status = 'ACTIVE'
"""

CATALOG_TIME = """
SELECT synced_at FROM guild_discord_catalog_state WHERE guild_id = :p1
"""

ROLES = """
SELECT role_id AS id, name
         FROM guild_discord_roles
         WHERE guild_id = :p1 AND NOT managed
         ORDER BY position DESC, lower(name), role_id
"""

CHANNELS = """
SELECT channel_id AS id, name, parent_name
         FROM guild_discord_channels
         WHERE guild_id = :p1
         ORDER BY position, lower(name), channel_id
"""

ROLE_EXISTS = """
SELECT true
         FROM guild_discord_roles
         WHERE guild_id = :p1 AND role_id = :p2 AND NOT managed
"""

CHANNEL_EXISTS = """
SELECT true FROM guild_discord_channels WHERE guild_id = :p1 AND channel_id = :p2
"""

ROTATION_LOCK = """
SELECT pg_advisory_xact_lock(
             hashtextextended(
                 jsonb_build_array(
                     'monkeybot:season-rotation:v1',
                     CAST(:p1 AS text)
                 )::text,
                 0
             )
         )
"""

SEASONS = """
SELECT season.id,
                season.name,
                season.status,
                season.started_at,
                season.ended_at,
                creator.display_name AS created_by,
                COALESCE(participants.participant_count, 0) AS participant_count,
                COALESCE(general.general_series_count, 0) AS general_series_count,
                COALESCE(auction.auction_tournament_count, 0) AS auction_tournament_count,
                COALESCE(auction.auction_ten_count, 0) AS auction_ten_count,
                COALESCE(auction.auction_twenty_count, 0) AS auction_twenty_count,
                COALESCE(game_summary.confirmed_game_count, 0) AS confirmed_game_count,
                game_summary.last_game_at
         FROM seasons AS season
         LEFT JOIN guild_members AS creator
           ON creator.guild_id = season.guild_id
          AND creator.user_id = season.created_by
         LEFT JOIN LATERAL (
             SELECT COUNT(*) AS general_series_count
             FROM series AS normal_series
             WHERE normal_series.guild_id = season.guild_id
               AND normal_series.season_id = season.id
               AND normal_series.status = 'COMPLETED'
               AND normal_series.deleted_at IS NULL
         ) AS general ON true
         LEFT JOIN LATERAL (
             SELECT COUNT(*) AS auction_tournament_count,
                    COUNT(*) FILTER (WHERE tournament.format = 'TEN_PERSON') AS auction_ten_count,
                    COUNT(*) FILTER (WHERE tournament.format = 'TWENTY_PERSON')
                        AS auction_twenty_count
             FROM auction_tournaments AS tournament
             WHERE tournament.guild_id = season.guild_id
               AND tournament.season_id = season.id
               AND tournament.status = 'COMPLETED'
               AND tournament.deleted_at IS NULL
         ) AS auction ON true
         LEFT JOIN LATERAL (
             SELECT COUNT(*) AS confirmed_game_count,
                    MAX(game.confirmed_at) AS last_game_at
             FROM games AS game
             WHERE game.guild_id = season.guild_id
               AND game.season_id = season.id
               AND game.status = 'CONFIRMED'
         ) AS game_summary ON true
         LEFT JOIN LATERAL (
             SELECT COUNT(DISTINCT participant.user_id) AS participant_count
             FROM games AS participant_game
             JOIN game_participants AS participant
               ON participant.guild_id = participant_game.guild_id
              AND participant.game_id = participant_game.id
             WHERE participant_game.guild_id = season.guild_id
               AND participant_game.season_id = season.id
               AND participant_game.status = 'CONFIRMED'
         ) AS participants ON true
         WHERE season.guild_id = :p1
         ORDER BY CASE season.status WHEN 'ACTIVE' THEN 0 ELSE 1 END,
                  season.ended_at DESC NULLS FIRST,
                  season.id DESC
"""

ACTIVE_SEASON = """
SELECT id, name, started_at
         FROM seasons
         WHERE guild_id = :p1 AND status = 'ACTIVE'
         FOR UPDATE
"""
