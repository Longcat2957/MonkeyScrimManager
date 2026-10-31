from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError
from monkeybot_api.schemas.users import LANES

PRIVACY_COLUMNS = '''public_profile_enabled AS "publicProfile", show_riot_ids AS "showRiotIds",
                     allow_external_record_search AS "allowExternalRecordSearch"'''


async def preferences(connection: AsyncConnection, user_id: str) -> dict[str, list[str]]:
    rows = await connection.execute(
        text(
            "SELECT lane, position, champion_id FROM user_champion_preferences "
            "WHERE user_id = :user_id ORDER BY lane, position"
        ),
        {"user_id": user_id},
    )
    snapshot: dict[str, list[str]] = {lane: [] for lane in LANES}
    for lane, position, champion in rows:
        if lane not in snapshot or position != len(snapshot[lane]) + 1:
            raise ApiError.from_status(500)
        snapshot[lane].append(champion)
    return snapshot


async def public_profile(connection: AsyncConnection, discord_id: str) -> dict[str, Any] | None:
    row = (
        (
            await connection.execute(
                text("""
                SELECT identity.discord_id AS "discordId", app_user.display_name AS "displayName",
                       count(DISTINCT guild.guild_id) AS "activeGuildCount",
                       count(DISTINCT season.id) AS "seasonCount"
                FROM user_discord_identities identity
                JOIN users app_user ON app_user.user_id = identity.user_id
                  AND app_user.identity_kind = 'DISCORD'
                LEFT JOIN user_privacy_settings privacy ON privacy.user_id = app_user.user_id
                LEFT JOIN guild_members member ON member.user_id = app_user.user_id
                  AND member.status = 'ACTIVE'
                LEFT JOIN guilds guild ON guild.guild_id = member.guild_id
                  AND guild.status = 'ACTIVE'
                LEFT JOIN seasons season ON season.guild_id = guild.guild_id
                WHERE identity.discord_id = :discord_id AND identity.identity_kind = 'DISCORD'
                  AND app_user.deleted_at IS NULL AND COALESCE(privacy.public_profile_enabled, true)
                GROUP BY identity.discord_id, app_user.display_name
            """),
                {"discord_id": discord_id},
            )
        )
        .mappings()
        .one_or_none()
    )
    return dict(row) if row is not None else None


async def privacy(connection: AsyncConnection, user_id: str) -> dict[str, Any] | None:
    row = (
        (
            await connection.execute(
                text(
                    f"SELECT {PRIVACY_COLUMNS} FROM user_privacy_settings WHERE user_id = :user_id"
                ),
                {"user_id": user_id},
            )
        )
        .mappings()
        .one_or_none()
    )
    return dict(row) if row is not None else None


async def update_privacy(
    connection: AsyncConnection, user_id: str, settings: dict[str, Any]
) -> dict[str, Any]:
    row = (
        (
            await connection.execute(
                text(f"""
                INSERT INTO user_privacy_settings
                  (user_id, public_profile_enabled, show_riot_ids, allow_external_record_search,
                   updated_at)
                VALUES (:user_id, :publicProfile, :showRiotIds, :allowExternalRecordSearch,
                        clock_timestamp())
                ON CONFLICT (user_id) DO UPDATE
                SET public_profile_enabled = EXCLUDED.public_profile_enabled,
                    show_riot_ids = EXCLUDED.show_riot_ids,
                    allow_external_record_search = EXCLUDED.allow_external_record_search,
                    updated_at = EXCLUDED.updated_at
                RETURNING {PRIVACY_COLUMNS}
            """),
                {"user_id": user_id, **settings},
            )
        )
        .mappings()
        .one()
    )
    return dict(row)


async def replace_champions(
    connection: AsyncConnection, user_id: str, lane: str, champion_ids: list[str]
) -> None:
    # Serialize every lane mutation for this user, including the returned snapshot.
    locked = await connection.scalar(
        text(
            "SELECT user_id FROM users WHERE user_id = :user_id AND deleted_at IS NULL FOR UPDATE"
        ),
        {"user_id": user_id},
    )
    if locked is None:
        raise ApiError.from_status(500)
    params = {"user_id": user_id, "lane": lane}
    await connection.execute(
        text("DELETE FROM user_champion_preferences WHERE user_id = :user_id AND lane = :lane"),
        params,
    )
    await connection.execute(
        text("""
                INSERT INTO user_champion_preferences (user_id, lane, position, champion_id)
                SELECT :user_id, :lane, requested.position::smallint, requested.champion_id
                FROM unnest(CAST(:ids AS text[]))
                WITH ORDINALITY AS requested(champion_id, position)
        """),
        {**params, "ids": champion_ids},
    )
