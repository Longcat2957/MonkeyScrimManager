from typing import Any, cast

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

GUILDS_SQL = """
SELECT guild.guild_id AS id,
       COALESCE(NULLIF(config.display_name, ''), guild.discord_name) AS display_name,
       guild.icon_hash,
       (SELECT count(*) FROM guild_members AS counted_member
        WHERE counted_member.guild_id = guild.guild_id
          AND counted_member.status = 'ACTIVE') AS member_count,
       EXISTS (SELECT 1 FROM guild_member_discord_roles AS member_role
               WHERE member_role.guild_id = guild.guild_id AND member_role.user_id = :user_id
                 AND member_role.role_id = config.operator_role_id) AS is_operator,
       COALESCE(config.is_externally_public, false) AS is_externally_public
FROM guild_members AS current_membership
JOIN guilds AS guild
  ON guild.guild_id = current_membership.guild_id AND guild.status = 'ACTIVE'
LEFT JOIN guild_config AS config ON config.guild_id = guild.guild_id
WHERE current_membership.user_id = :user_id AND current_membership.status = 'ACTIVE'
"""


async def list_guilds(connection: AsyncConnection, user_id: str) -> list[dict[str, Any]]:
    rows = (
        (
            await connection.execute(
                text(
                    GUILDS_SQL + " ORDER BY COALESCE(NULLIF(config.display_name, ''),"
                    " guild.discord_name), guild.guild_id"
                ),
                {"user_id": user_id},
            )
        )
        .mappings()
        .all()
    )
    return [dict(row) for row in rows]


async def get_guild(
    connection: AsyncConnection, user_id: str, guild_id: str
) -> dict[str, Any] | None:
    # Membership and season share one READ COMMITTED statement snapshot.
    row = (
        (
            await connection.execute(
                text(
                    """
                SELECT context.*, season.id AS season_id, season.name AS season_name,
                       season.started_at AS season_started_at
                FROM (
            """
                    + GUILDS_SQL
                    + """
                    AND guild.guild_id = :guild_id
                ) AS context
                LEFT JOIN seasons AS season
                  ON season.guild_id = context.id AND season.status = 'ACTIVE'
            """
                ),
                {"user_id": user_id, "guild_id": guild_id},
            )
        )
        .mappings()
        .one_or_none()
    )
    return dict(row) if row is not None else None


async def set_default_guild(connection: AsyncConnection, user_id: str, guild_id: str) -> str | None:
    return cast(
        str | None,
        await connection.scalar(
            text("""
                INSERT INTO user_guild_preferences (user_id, default_guild_id, updated_at)
                SELECT app_user.user_id, guild.guild_id, clock_timestamp()
                FROM users AS app_user
                JOIN guild_members AS membership
                  ON membership.user_id = app_user.user_id AND membership.guild_id = :guild_id
                 AND membership.status = 'ACTIVE'
                JOIN guilds AS guild
                  ON guild.guild_id = membership.guild_id AND guild.status = 'ACTIVE'
                WHERE app_user.user_id = :user_id AND app_user.deleted_at IS NULL
                ON CONFLICT (user_id) DO UPDATE
                SET default_guild_id = EXCLUDED.default_guild_id, updated_at = EXCLUDED.updated_at
                RETURNING default_guild_id
        """),
            {"user_id": user_id, "guild_id": guild_id},
        ),
    )
