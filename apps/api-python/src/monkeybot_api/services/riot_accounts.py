"""Self-service Riot account queries and mutations."""

import re
from typing import Any

from monkeybot_api.core.config import Settings
from monkeybot_api.core.errors import ApiError
from monkeybot_api.db.database import Database
from monkeybot_api.db.transactions import mutation
from monkeybot_api.repositories import riot_accounts as repository
from monkeybot_api.schemas.riot_accounts import RiotIdentity
from monkeybot_api.services import internal_guilds


def validate_puuid(puuid: str) -> None:
    if not re.fullmatch(r"[A-Za-z0-9_-]{1,256}", puuid, flags=re.ASCII):
        raise ApiError(400, "invalid_riot_account", "Riot 계정 식별자가 올바르지 않습니다.")


async def list_accounts(database: Database, user: str) -> dict[str, Any]:
    async with database.connection() as c:
        return await repository.list_accounts(c, user)


async def create(
    database: Database, settings: Settings, user: str, body: RiotIdentity
) -> dict[str, Any]:
    platform, region = internal_guilds.parse_platform(body.platform)
    game_name = internal_guilds.riot_component(body.gameName, 64)
    tag_line = internal_guilds.riot_component(body.tagLine, 16)
    account = await internal_guilds.resolve_account(settings, game_name, tag_line, platform, region)
    return await mutation(database, lambda c: repository.link(c, user, account))


async def set_main(database: Database, user: str, puuid: str) -> dict[str, Any]:
    return await mutation(database, lambda c: repository.set_main(c, user, puuid))


async def delete(database: Database, user: str, puuid: str) -> dict[str, Any]:
    return await mutation(database, lambda c: repository.delete(c, user, puuid))


async def refresh(database: Database, settings: Settings, user: str, puuid: str) -> dict[str, Any]:
    async with database.connection() as c:
        stored_platform = await repository.stored_platform(c, user, puuid)
    try:
        platform, region = internal_guilds.parse_platform(stored_platform)
    except ApiError:
        raise ApiError.from_status(500) from None
    account = await internal_guilds.resolve_account_by_puuid(settings, puuid, platform, region)
    return await mutation(database, lambda c: repository.refresh(c, user, account))
