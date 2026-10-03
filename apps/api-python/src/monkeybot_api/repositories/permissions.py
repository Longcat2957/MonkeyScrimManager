from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError
from monkeybot_api.db.queries import params
from monkeybot_api.repositories.guilds import queries as sql


async def access_failure(c: AsyncConnection, guild: str, actor: str) -> None:
    if await c.scalar(text(sql.MEMBER), params(guild, actor)):
        raise ApiError(403, "operator_required", "길드 운영자 권한이 필요합니다.")
    raise ApiError(404, "not_found", "요청한 길드를 찾을 수 없습니다.")


async def authorize(c: AsyncConnection, guild: str, actor: str) -> None:
    if not await c.scalar(text(sql.AUTHORIZE), params(guild, actor)):
        await access_failure(c, guild, actor)
