from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError
from monkeybot_api.schemas.bot import Actor


async def sync_actor(connection: AsyncConnection, actor: Actor, params: dict[str, Any]) -> None:
    values = {**params, **actor.model_dump()}
    persisted = await connection.scalar(
        text("""
        INSERT INTO users (user_id, identity_kind, display_name, avatar_hash,
                           created_at, updated_at, deleted_at)
        VALUES (:actor_id, 'DISCORD', :globalDisplayName, :avatarHash, :now, :now, NULL)
        ON CONFLICT (user_id) DO UPDATE
        SET display_name = EXCLUDED.display_name, avatar_hash = EXCLUDED.avatar_hash,
            updated_at = EXCLUDED.updated_at, deleted_at = NULL
        WHERE users.identity_kind = 'DISCORD' RETURNING user_id
    """),
        values,
    )
    identity = (
        await connection.scalar(
            text("""
        INSERT INTO user_discord_identities (user_id, discord_id, created_at, updated_at)
        VALUES (:actor_id, :actor_id, :now, :now)
        ON CONFLICT (discord_id) DO UPDATE SET updated_at = EXCLUDED.updated_at
        WHERE user_discord_identities.user_id = EXCLUDED.user_id RETURNING user_id
    """),
            values,
        )
        if persisted is not None
        else None
    )
    if identity != persisted or persisted is None:
        raise ApiError(
            409,
            "user_identity_conflict",
            "이 Discord 계정 식별자는 다른 사용자 유형에서 이미 사용 중입니다.",
        )
    await connection.execute(
        text("""
        INSERT INTO guild_members
            (guild_id, user_id, display_name, status, main_lane, first_seen_at, registered_at,
             discord_joined_at, last_active_at, last_synced_at, inactivated_at)
        VALUES (:guild, :actor_id, :displayName, 'ACTIVE', NULL, :now, :now, NULL, :now, :now, NULL)
        ON CONFLICT (guild_id, user_id) DO UPDATE
        SET display_name = EXCLUDED.display_name,
            registered_at = COALESCE(guild_members.registered_at, EXCLUDED.registered_at),
            last_active_at = EXCLUDED.last_active_at, last_synced_at = EXCLUDED.last_synced_at
    """),
        values,
    )
    await connection.execute(
        text(
            "DELETE FROM guild_member_discord_roles WHERE guild_id = :guild AND user_id = :actor_id"
        ),
        values,
    )
    for role in dict.fromkeys(actor.roleIds):
        await connection.execute(
            text("""
            INSERT INTO guild_member_discord_roles (guild_id, user_id, role_id, last_synced_at)
            VALUES (:guild, :actor_id, :role, :now)
        """),
            {**values, "role": role},
        )
