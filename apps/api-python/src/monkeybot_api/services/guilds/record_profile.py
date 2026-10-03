"""Resolve a member without exposing private account links or tenant existence."""

import logging
import re
from typing import Any

from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.config import is_snowflake
from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.riot_identity import normalize_riot_id
from monkeybot_api.db.database import Database
from monkeybot_api.repositories.guilds import record_profile as repository
from monkeybot_api.repositories.users import preferences
from monkeybot_api.schemas.common import user_id
from monkeybot_api.schemas.guilds.record_profile import ProfileData, StoredMember

logger = logging.getLogger(__name__)


def hidden_member() -> ApiError:
    return ApiError(404, "not_found", "요청한 길드 멤버를 찾을 수 없습니다.")


async def resolve_member(
    connection: AsyncConnection, guild: str, viewer: str, query: str
) -> dict[str, Any]:
    try:
        user_id(query)
        is_id = True
    except ValueError:
        is_id = False
    rows = await repository.member_rows(
        connection, guild, viewer, query, is_id, normalize_riot_id(query)
    )
    if not rows:
        raise ValueError("member query returned no access projection")
    if not rows[0]["has_access"]:
        raise hidden_member()
    candidates = [
        StoredMember.model_validate(row).model_dump() for row in rows if row["user_id"] is not None
    ]
    if not candidates:
        raise hidden_member()
    if len(candidates) > 1:
        raise ApiError(409, "ambiguous_guild_member", "같은 이름의 길드 멤버가 여러 명입니다.")
    return candidates[0]


async def load_identity(connection: AsyncConnection, member: dict[str, Any]) -> dict[str, Any]:
    try:
        champions = await preferences(connection, member["user_id"])
    except ApiError as error:
        if error.status != 500:
            raise
        raise ValueError("stored champion preferences are not contiguous") from error
    accounts = await repository.visible_accounts(connection, member["user_id"])
    # Profile reads use READ COMMITTED, so publication can be revoked after resolution.
    if member["matched_by_riot_id"] and not accounts:
        raise hidden_member()
    avatar = member["avatar_hash"]
    return ProfileData.model_validate(
        {
            "member": {
                "displayName": member["display_name"],
                "avatarUrl": (
                    f"https://cdn.discordapp.com/avatars/{member['user_id']}/{avatar}.webp?size=128"
                    if avatar is not None and re.fullmatch(r"[0-9a-fA-F_]{1,128}", avatar)
                    else None
                ),
                "mainLane": member["main_lane"],
                "profileHref": (
                    f"/users/{member['user_id']}"
                    if member["public_profile_enabled"] and is_snowflake(member["user_id"])
                    else None
                ),
            },
            "championPreferences": champions,
            "riotAccounts": [
                {
                    "platform": account["platform"],
                    "gameName": account["game_name"],
                    "tagLine": account["tag_line"],
                    "profileIconId": account["profile_icon_id"],
                    "isMain": account["is_main"],
                }
                for account in accounts
            ],
        }
    ).model_dump()


async def get_profile(
    database: Database, guild: str, viewer: str, query: str, request_id: str
) -> dict[str, Any]:
    try:
        async with database.connection() as connection:
            member = await resolve_member(connection, guild, viewer, query)
            return await load_identity(connection, member)
    except ValueError:
        logger.exception("guild_record_profile_invariant_failed", extra={"request_id": request_id})
        raise ApiError.from_status(500) from None
