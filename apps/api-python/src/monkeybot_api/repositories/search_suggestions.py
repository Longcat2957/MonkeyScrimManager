"""Frozen prefix queries with literal matching, stable ranking, and privacy gates."""

from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection


async def public_riot_accounts(
    connection: AsyncConnection, platform: str, prefix: str, limit: int
) -> list[dict[str, Any]]:
    result = await connection.execute(
        text("""
        WITH known_riot_ids AS (
             SELECT account.platform,
                    account.game_name,
                    account.tag_line,
                    account.normalized_game_name,
                    account.normalized_tag_line,
                    account.profile_icon_id,
                    account.updated_at AS known_at,
                    1 AS source_priority
             FROM riot_accounts AS account
             WHERE account.platform = :platform
               AND account.origin = 'RIOT'
               AND (
                   starts_with(account.normalized_game_name, :prefix)
                   OR starts_with(
                       account.normalized_game_name || '#' || account.normalized_tag_line,
                       :prefix
                   )
               )

             UNION ALL

             SELECT snapshot.platform,
                    snapshot.game_name,
                    snapshot.tag_line,
                    snapshot.normalized_game_name,
                    snapshot.normalized_tag_line,
                    CASE
                        WHEN jsonb_typeof(snapshot.snapshot -> 'profileIconId') = 'number'
                        THEN CASE
                            WHEN (snapshot.snapshot ->> 'profileIconId')::numeric
                                 BETWEEN 0 AND 2147483647
                            THEN (snapshot.snapshot ->> 'profileIconId')::numeric::integer
                        END
                    END AS profile_icon_id,
                    snapshot.updated_at AS known_at,
                    0 AS source_priority
             FROM riot_records_snapshots AS snapshot
             WHERE snapshot.platform = :platform
               AND (
                   starts_with(snapshot.normalized_game_name, :prefix)
                   OR starts_with(
                       snapshot.normalized_game_name || '#' || snapshot.normalized_tag_line,
                       :prefix
                   )
               )
         ),
         canonical_riot_ids AS (
             SELECT DISTINCT ON (
                        known.platform,
                        known.normalized_game_name,
                        known.normalized_tag_line
                    )
                    known.platform,
                    known.game_name,
                    known.tag_line,
                    known.normalized_game_name,
                    known.normalized_tag_line,
                    known.profile_icon_id
             FROM known_riot_ids AS known
             ORDER BY known.platform,
                      known.normalized_game_name,
                      known.normalized_tag_line,
                      known.known_at DESC,
                      known.source_priority
         )
         SELECT known.platform,
                known.game_name,
                known.tag_line,
                known.profile_icon_id
         FROM canonical_riot_ids AS known
         ORDER BY known.normalized_game_name,
                  known.normalized_tag_line,
                  known.game_name,
                  known.tag_line
         LIMIT :limit
        """),
        {"platform": platform, "prefix": prefix, "limit": limit},
    )
    return [dict(row) for row in result.mappings()]


async def guild_members(
    connection: AsyncConnection, viewer_id: str, guild_id: str, prefix: str, limit: int
) -> list[dict[str, Any]]:
    result = await connection.execute(
        text("""
        WITH viewer AS (
             SELECT EXISTS (
                 SELECT 1
                 FROM guild_members AS membership
                 JOIN guilds AS guild
                   ON guild.guild_id = membership.guild_id
                  AND guild.status = 'ACTIVE'
                 WHERE membership.guild_id = :guild_id
                   AND membership.user_id = :viewer_id
                   AND membership.status = 'ACTIVE'
             ) AS has_access
         )
         SELECT viewer.has_access,
                suggestion.display_name,
                suggestion.game_name,
                suggestion.tag_line,
                suggestion.profile_icon_id
         FROM viewer
         LEFT JOIN LATERAL (
             SELECT member.display_name,
                    CASE WHEN privacy.user_id IS NOT NULL THEN account.game_name END AS game_name,
                    CASE WHEN privacy.user_id IS NOT NULL THEN account.tag_line END AS tag_line,
                    CASE
                        WHEN privacy.user_id IS NOT NULL THEN account.profile_icon_id
                    END AS profile_icon_id
             FROM guild_members AS member
             JOIN users AS app_user
               ON app_user.user_id = member.user_id
              AND app_user.deleted_at IS NULL
             LEFT JOIN riot_accounts AS account
               ON account.user_id = member.user_id
              AND account.is_main
             LEFT JOIN user_privacy_settings AS privacy
               ON privacy.user_id = member.user_id
              AND privacy.show_riot_ids
             WHERE viewer.has_access
               AND member.guild_id = :guild_id
               AND member.status = 'ACTIVE'
               AND starts_with(lower(member.display_name), lower(:prefix))
             ORDER BY lower(member.display_name), member.display_name, member.user_id
             LIMIT :limit
         ) AS suggestion ON true
        """),
        {"viewer_id": viewer_id, "guild_id": guild_id, "prefix": prefix, "limit": limit},
    )
    return [dict(row) for row in result.mappings()]
