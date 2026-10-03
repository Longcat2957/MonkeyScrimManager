import hashlib
import logging
import re
from datetime import UTC, datetime, timedelta
from typing import Any

from sqlalchemy.exc import SQLAlchemyError

from monkeybot_api.core.config import Settings, is_snowflake
from monkeybot_api.core.errors import ApiError
from monkeybot_api.db.database import Database
from monkeybot_api.repositories import auth as repository

logger = logging.getLogger(__name__)


async def authenticate_session(
    database: Database, settings: Settings, token: str | None, request_id: str
) -> dict[str, Any]:
    if not settings.auth_configured or database.engine is None:
        raise ApiError(
            503, "authentication_unavailable", "인증 서비스를 일시적으로 사용할 수 없습니다."
        )
    if token is None:
        raise ApiError.from_status(401)
    try:
        row = await repository.find_session(database, hashlib.sha256(token.encode()).digest())
    except (OSError, TimeoutError) as error:
        logger.error(
            "session_database_transport_failed",
            extra={"request_id": request_id, "failure_type": type(error).__name__},
        )
        raise ApiError(
            503, "database_unavailable", "데이터베이스를 일시적으로 사용할 수 없습니다."
        ) from error
    if row is None:
        raise ApiError.from_status(401)
    if not is_snowflake(row["discord_id"]) or (
        row["default_guild_id"] is not None and not is_snowflake(row["default_guild_id"])
    ):
        raise ApiError.from_status(500)
    if row["last_seen_at"] < datetime.now(UTC) - timedelta(minutes=5):
        try:
            await repository.touch_session(database, row["session_id"])
        except (SQLAlchemyError, OSError, TimeoutError) as error:
            logger.warning(
                "session_touch_failed",
                extra={
                    "request_id": request_id,
                    "failure_type": type(error).__name__,
                },
            )
    return {**dict(row), "token_hash": hashlib.sha256(token.encode()).digest()}


def describe_user(row: dict[str, Any], settings: Settings) -> dict[str, Any]:
    avatar = row["avatar_hash"]
    avatar_url = None
    if avatar and re.fullmatch(r"[0-9a-fA-F_]{1,128}", avatar):
        avatar_url = (
            f"https://cdn.discordapp.com/avatars/{row['discord_id']}/{avatar}.webp?size=128"
        )
    return {
        "discordId": row["discord_id"],
        "displayName": row["display_name"],
        "avatarUrl": avatar_url,
        "defaultGuildId": row["default_guild_id"],
        "serviceAdmin": row["discord_id"] in settings.admin_ids,
    }
