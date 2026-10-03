"""Exact member resolution and privacy-filtered Riot identity queries."""

from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection


async def member_rows(
    connection: AsyncConnection,
    guild: str,
    viewer: str,
    query: str,
    is_id: bool,
    normalized: str,
) -> list[dict[str, Any]]:
    rows = await connection.execute(
        text("""
        WITH viewer AS (
            SELECT EXISTS (
                SELECT 1
                FROM guild_members AS membership
                JOIN guilds AS guild
                  ON guild.guild_id = membership.guild_id
                 AND guild.status = 'ACTIVE'
                WHERE membership.guild_id = :guild
                  AND membership.user_id = :viewer
                  AND membership.status = 'ACTIVE'
            ) AS has_access
        ),
        candidates AS (
            SELECT member.user_id,
                   member.display_name,
                   app_user.avatar_hash,
                   member.main_lane,
                   COALESCE(profile_privacy.public_profile_enabled, TRUE) AS public_profile_enabled,
                   NOT :is_id
                   AND lower(member.display_name) <> lower(:query)
                   AND EXISTS (
                       SELECT 1
                       FROM riot_accounts AS matched_account
                       JOIN user_privacy_settings AS matched_privacy
                         ON matched_privacy.user_id = matched_account.user_id
                        AND matched_privacy.show_riot_ids IS TRUE
                       WHERE matched_account.user_id = member.user_id
                         AND matched_account.normalized_game_name
                             || '#'
                             || matched_account.normalized_tag_line = :normalized
                   ) AS matched_by_riot_id
            FROM guild_members AS member
            JOIN users AS app_user
              ON app_user.user_id = member.user_id
             AND app_user.deleted_at IS NULL
            LEFT JOIN user_privacy_settings AS profile_privacy
              ON profile_privacy.user_id = member.user_id
            WHERE (SELECT has_access FROM viewer)
              AND member.guild_id = :guild
              AND member.status = 'ACTIVE'
              AND (
                  (:is_id AND member.user_id = :query)
                  OR (
                      NOT :is_id
                      AND (
                          lower(member.display_name) = lower(:query)
                          OR EXISTS (
                              SELECT 1
                              FROM riot_accounts AS account
                              JOIN user_privacy_settings AS privacy
                                ON privacy.user_id = account.user_id
                               AND privacy.show_riot_ids IS TRUE
                              WHERE account.user_id = member.user_id
                                AND account.normalized_game_name
                                    || '#'
                                    || account.normalized_tag_line = :normalized
                          )
                      )
                  )
              )
            ORDER BY lower(member.display_name), member.display_name, member.user_id
            LIMIT 2
        )
        SELECT viewer.has_access,
               candidate.user_id,
               candidate.display_name,
               candidate.avatar_hash,
               candidate.main_lane,
               candidate.public_profile_enabled,
               candidate.matched_by_riot_id
        FROM viewer
        LEFT JOIN candidates AS candidate ON true
        """),
        {
            "guild": guild,
            "viewer": viewer,
            "query": query,
            "is_id": is_id,
            "normalized": normalized,
        },
    )
    return [dict(row) for row in rows.mappings()]


async def visible_accounts(connection: AsyncConnection, user: str) -> list[dict[str, Any]]:
    rows = await connection.execute(
        text("""
        SELECT account.platform,
               account.game_name,
               account.tag_line,
               account.profile_icon_id,
               account.is_main
        FROM riot_accounts AS account
        JOIN user_privacy_settings AS privacy
          ON privacy.user_id = account.user_id
         AND privacy.show_riot_ids IS TRUE
        WHERE account.user_id = :user
        ORDER BY account.is_main DESC, account.created_at, account.puuid
        """),
        {"user": user},
    )
    return [dict(row) for row in rows.mappings()]
