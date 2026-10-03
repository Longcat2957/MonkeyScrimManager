from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.schemas.guilds.audit_logs import AuditLogCursor


async def fetch_page(
    connection: AsyncConnection, guild: str, cursor: AuditLogCursor | None, limit: int
) -> list[dict[str, Any]]:
    result = await connection.execute(
        text("""
        SELECT audit.id,
               audit.principal_type,
               audit.principal_id,
               CASE WHEN audit.principal_type = 'USER'
                    THEN COALESCE(principal.display_name, principal_user.display_name)
                    ELSE NULL
               END AS principal_display_name,
               audit.action,
               audit.target_type,
               audit.target_id,
               CASE audit.target_type
                   WHEN 'GUILD' THEN config.display_name
                   WHEN 'GUILD_CONFIG' THEN config.display_name
                   WHEN 'GUILD_MEMBER' THEN target_member.display_name
                   WHEN 'GUILD_MEMBER_MMR' THEN target_member.display_name
                   WHEN 'SEASON' THEN target_season.name
                   WHEN 'RECRUITMENT' THEN target_recruitment.title
                   ELSE NULL
               END AS target_display_name,
               audit.checkpoint_id,
               audit.request_id,
               audit.before_summary,
               audit.after_summary,
               audit.result,
               audit.note,
               audit.created_at
        FROM admin_audit_log AS audit
        JOIN guild_config AS config ON config.guild_id = audit.guild_id
        LEFT JOIN guild_members AS principal
          ON principal.guild_id = audit.guild_id
         AND principal.user_id = audit.principal_id
         AND audit.principal_type = 'USER'
        LEFT JOIN users AS principal_user
          ON principal_user.user_id = audit.principal_id
         AND audit.principal_type = 'USER'
        LEFT JOIN guild_members AS target_member
          ON target_member.guild_id = audit.guild_id
         AND target_member.user_id = audit.target_id
         AND audit.target_type IN ('GUILD_MEMBER', 'GUILD_MEMBER_MMR')
        LEFT JOIN seasons AS target_season
          ON target_season.guild_id = audit.guild_id
         AND target_season.id::text = audit.target_id
         AND audit.target_type = 'SEASON'
        LEFT JOIN recruitments AS target_recruitment
          ON target_recruitment.guild_id = audit.guild_id
         AND target_recruitment.id::text = audit.target_id
         AND audit.target_type = 'RECRUITMENT'
        WHERE audit.guild_id = :guild
          AND (
              CAST(:cursor_time AS timestamptz) IS NULL
              OR (audit.created_at, audit.id) < (:cursor_time, CAST(:cursor_id AS uuid))
          )
        ORDER BY audit.created_at DESC, audit.id DESC
        LIMIT :limit
        """),
        {
            "guild": guild,
            "cursor_time": cursor.database_timestamp() if cursor else None,
            "cursor_id": cursor.id if cursor else None,
            "limit": limit + 1,
        },
    )
    return [dict(row) for row in result.mappings()]
