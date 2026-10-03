"""Pending workflow rows in the legacy priority and activity order."""

from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection


async def fetch_operations(
    connection: AsyncConnection, guild: str, actor: str
) -> list[dict[str, Any]]:
    rows = await connection.execute(
        text("""
        WITH viewer AS (
            SELECT membership.guild_id
            FROM guild_members AS membership
            JOIN guilds AS guild
              ON guild.guild_id = membership.guild_id
             AND guild.status = 'ACTIVE'
            WHERE membership.guild_id = :guild_id
              AND membership.user_id = :user_id
              AND membership.status = 'ACTIVE'
        ),
        operation_rows AS (
            SELECT recruitment.id AS entity_id,
                   'ENTRY'::text AS source_kind,
                   recruitment.title,
                   recruitment.updated_at,
                   joined.count AS first_count,
                   assigned.count AS second_count,
                   recruitment.target_count::bigint AS third_count,
                   NULL::text AS source_status,
                   NULL::text AS auxiliary_status,
                   NULL::uuid AS auxiliary_id,
                   0::smallint AS priority_rank
            FROM recruitments AS recruitment
            JOIN viewer ON viewer.guild_id = recruitment.guild_id
            CROSS JOIN LATERAL (
                SELECT count(*) AS count
                FROM recruitment_participants AS participant
                WHERE participant.guild_id = recruitment.guild_id
                  AND participant.recruitment_id = recruitment.id
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
                WHERE assignment.guild_id = recruitment.guild_id
                  AND assignment.recruitment_id = recruitment.id
            ) AS assigned
            WHERE recruitment.guild_id = :guild_id
              AND recruitment.status = 'CLOSED'
              AND recruitment.converted_at IS NULL
              AND NOT EXISTS (
                  SELECT 1
                  FROM series AS existing_series
                  WHERE existing_series.guild_id = recruitment.guild_id
                    AND existing_series.source_recruitment_id = recruitment.id
                    AND existing_series.status <> 'CANCELLED'
                    AND existing_series.deleted_at IS NULL
              )
        
            UNION ALL
        
            SELECT scrim.id,
                   'PICK_BAN'::text,
                   scrim.title,
                   scrim.updated_at,
                   score.confirmed_games,
                   scrim.best_of::bigint,
                   0::bigint,
                   scrim.status,
                   NULL::text,
                   NULL::uuid,
                   0::smallint
            FROM series AS scrim
            JOIN viewer ON viewer.guild_id = scrim.guild_id
            CROSS JOIN LATERAL (
                SELECT count(*) AS confirmed_games
                FROM games AS game
                WHERE game.guild_id = scrim.guild_id
                  AND game.series_id = scrim.id
                  AND game.status = 'CONFIRMED'
            ) AS score
            WHERE scrim.guild_id = :guild_id
              AND scrim.status = 'IN_PROGRESS'
              AND scrim.deleted_at IS NULL
        
            UNION ALL
        
            SELECT recruitment.id,
                   'AUCTION_RECRUITMENT'::text,
                   recruitment.title,
                   recruitment.updated_at,
                   joined.count,
                   CASE recruitment.format WHEN 'TEN_PERSON' THEN 2 ELSE 4 END::bigint,
                   recruitment.target_count::bigint,
                   recruitment.status,
                   NULL::text,
                   NULL::uuid,
                   1::smallint
            FROM auction_recruitments AS recruitment
            JOIN viewer ON viewer.guild_id = recruitment.guild_id
            CROSS JOIN LATERAL (
                SELECT count(*) AS count
                FROM auction_recruitment_participants AS participant
                WHERE participant.guild_id = recruitment.guild_id
                  AND participant.recruitment_id = recruitment.id
                  AND participant.status = 'JOINED'
            ) AS joined
            WHERE recruitment.guild_id = :guild_id
              AND recruitment.status = 'CLOSED'
              AND recruitment.converted_at IS NULL
              AND NOT EXISTS (
                  SELECT 1
                  FROM auction_tournaments AS existing_tournament
                  WHERE existing_tournament.guild_id = recruitment.guild_id
                    AND existing_tournament.source_recruitment_id = recruitment.id
                    AND existing_tournament.status <> 'CANCELLED'
                    AND existing_tournament.deleted_at IS NULL
              )
        
            UNION ALL
        
            SELECT tournament.id,
                   'AUCTION_TOURNAMENT'::text,
                   tournament.title,
                   tournament.updated_at,
                   members.count,
                   teams.count,
                   matches.count,
                   tournament.status,
                   next_match.status,
                   next_match.id,
                   1::smallint
            FROM auction_tournaments AS tournament
            JOIN viewer ON viewer.guild_id = tournament.guild_id
            CROSS JOIN LATERAL (
                SELECT count(*) AS count
                FROM auction_team_members AS member
                WHERE member.guild_id = tournament.guild_id
                  AND member.tournament_id = tournament.id
                  AND member.released_at IS NULL
            ) AS members
            CROSS JOIN LATERAL (
                SELECT count(*) AS count
                FROM auction_teams AS team
                WHERE team.guild_id = tournament.guild_id
                  AND team.tournament_id = tournament.id
            ) AS teams
            CROSS JOIN LATERAL (
                SELECT count(*) AS count
                FROM auction_matches AS auction_match
                WHERE auction_match.guild_id = tournament.guild_id
                  AND auction_match.tournament_id = tournament.id
                  AND auction_match.deleted_at IS NULL
                  AND auction_match.status NOT IN ('COMPLETED', 'CANCELLED')
            ) AS matches
            LEFT JOIN LATERAL (
                SELECT auction_match.id,
                       auction_match.status
                FROM auction_matches AS auction_match
                WHERE auction_match.guild_id = tournament.guild_id
                  AND auction_match.tournament_id = tournament.id
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
            WHERE tournament.guild_id = :guild_id
              AND tournament.status IN ('BIDDING', 'BRACKET_SETUP', 'IN_GAME')
              AND tournament.deleted_at IS NULL
        )
        SELECT entity_id,
               source_kind,
               title,
               updated_at,
               first_count,
               second_count,
               third_count,
               source_status,
               auxiliary_status,
               auxiliary_id
        FROM operation_rows
        ORDER BY priority_rank, updated_at DESC, source_kind, entity_id DESC
        LIMIT 100
        """),
        {"guild_id": guild, "user_id": actor},
    )
    return [dict(row) for row in rows.mappings()]
