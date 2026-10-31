from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError, hidden_guild
from monkeybot_api.schemas.bot import Actor


async def require_operator(
    connection: AsyncConnection, params: dict[str, Any], role: str, actor: Actor | None
) -> None:
    authorized = (
        role in actor.roleIds
        if actor is not None
        else await connection.scalar(
            text(
                "SELECT true FROM guild_member_discord_roles "
                "WHERE guild_id = :guild AND user_id = :actor_id AND role_id = :role FOR SHARE"
            ),
            {**params, "role": role},
        )
    )
    if not authorized:
        raise ApiError(403, "operator_required", "길드 운영자 권한이 필요합니다.")


async def require_guild_access(connection: AsyncConnection, user_id: str, guild_id: str) -> None:
    accessible = await connection.scalar(
        text("""
        SELECT EXISTS (
            SELECT 1 FROM guild_members membership
            JOIN guilds guild ON guild.guild_id = membership.guild_id AND guild.status = 'ACTIVE'
            WHERE membership.guild_id = :guild_id AND membership.user_id = :user_id
              AND membership.status = 'ACTIVE'
        )
    """),
        {"guild_id": guild_id, "user_id": user_id},
    )
    if not accessible:
        raise hidden_guild()
