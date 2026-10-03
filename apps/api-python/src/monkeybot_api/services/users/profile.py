import re
from typing import Any

from monkeybot_api.core.config import is_snowflake
from monkeybot_api.core.errors import ApiError
from monkeybot_api.db.database import Database
from monkeybot_api.repositories import users as repository
from monkeybot_api.schemas.users import LANES, PrivacyInput


def validate_champions(ids: list[str]) -> None:
    if len(ids) > 10:
        raise ApiError(
            422,
            "too_many_champion_preferences",
            "한 포지션에는 선호 챔피언을 최대 10개까지 저장할 수 있습니다.",
            {"max": 10},
        )
    seen: set[str] = set()
    for champion in ids:
        if not re.fullmatch(r"[A-Za-z0-9]{1,64}", champion):
            raise ApiError(422, "invalid_champion_id", "챔피언 ID 형식이 올바르지 않습니다.")
        if champion in seen:
            raise ApiError(
                422,
                "duplicate_champion_id",
                "같은 챔피언을 한 포지션에 중복해서 저장할 수 없습니다.",
            )
        seen.add(champion)


async def public_profile(database: Database, discord_id: str) -> dict[str, Any]:
    if not is_snowflake(discord_id):
        raise ApiError(400, "invalid_discord_id", "Discord 사용자 ID 형식이 올바르지 않습니다.")
    async with database.connection() as connection:
        row = await repository.public_profile(connection, discord_id)
    if row is None:
        raise ApiError(404, "not_found", "요청한 사용자 프로필을 찾을 수 없습니다.")
    if not row["displayName"].strip():
        raise ApiError.from_status(500)
    return row


async def privacy(database: Database, user_id: str) -> dict[str, Any]:
    async with database.connection() as connection:
        row = await repository.privacy(connection, user_id)
    return {
        "settings": row
        if row is not None
        else {
            "publicProfile": True,
            "showRiotIds": True,
            "allowExternalRecordSearch": False,
        }
    }


async def update_privacy(database: Database, user_id: str, command: PrivacyInput) -> dict[str, Any]:
    async with database.connection(transaction=True) as connection:
        row = await repository.update_privacy(connection, user_id, command.model_dump())
    return {"settings": row}


async def champions(database: Database, user_id: str) -> dict[str, Any]:
    async with database.connection() as connection:
        snapshot = await repository.preferences(connection, user_id)
    return {"preferences": snapshot}


async def update_champions(
    database: Database, user_id: str, lane: str, champion_ids: list[str]
) -> dict[str, Any]:
    if lane not in LANES:
        raise ApiError(400, "invalid_lane", "지원하지 않는 포지션입니다.", {"lane": lane})
    validate_champions(champion_ids)
    async with database.connection(transaction=True) as connection:
        await repository.replace_champions(connection, user_id, lane, champion_ids)
        snapshot = await repository.preferences(connection, user_id)
    return {"preferences": snapshot}
