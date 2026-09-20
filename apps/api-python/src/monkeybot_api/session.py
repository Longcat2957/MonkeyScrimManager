import hashlib
import logging
import re
from datetime import UTC, datetime, timedelta
from typing import Any

from fastapi import Request
from sqlalchemy import text
from sqlalchemy.exc import SQLAlchemyError

from monkeybot_api.auth import cookie_value
from monkeybot_api.config import Settings, is_snowflake
from monkeybot_api.database import Database
from monkeybot_api.errors import ApiError

logger = logging.getLogger(__name__)

# Ported from AuthRepository::resolve_session. Keep tenant/default-membership predicates intact.
SESSION_SQL = text("""
SELECT session.id AS session_id, session.user_id, identity.discord_id, session.csrf_secret_hash,
       users.display_name, users.avatar_hash,
       CASE WHEN default_member.user_id IS NOT NULL AND default_guild.status = 'ACTIVE'
            THEN preferences.default_guild_id ELSE NULL END AS default_guild_id,
       session.last_seen_at
FROM user_sessions AS session
JOIN users ON users.user_id = session.user_id AND users.identity_kind = 'DISCORD'
JOIN user_discord_identities AS identity ON identity.user_id = session.user_id
LEFT JOIN user_guild_preferences AS preferences ON preferences.user_id = session.user_id
LEFT JOIN guild_members AS default_member
  ON default_member.guild_id = preferences.default_guild_id
 AND default_member.user_id = session.user_id AND default_member.status = 'ACTIVE'
LEFT JOIN guilds AS default_guild ON default_guild.guild_id = preferences.default_guild_id
WHERE session.token_hash = :token_hash AND session.revoked_at IS NULL
  AND session.expires_at > CURRENT_TIMESTAMP AND users.deleted_at IS NULL
""")


def session_cookie(request: Request) -> str | None:
    return cookie_value(request, "monkeybot_session")


async def resolve_session(
    request: Request, database: Database, settings: Settings
) -> dict[str, Any]:
    if not settings.auth_configured or database.engine is None:
        raise ApiError(
            503, "authentication_unavailable", "인증 서비스를 일시적으로 사용할 수 없습니다."
        )
    token = session_cookie(request)
    if token is None:
        raise ApiError.from_status(401)
    try:
        async with database.engine.connect() as connection:
            result = await connection.execute(
                SESSION_SQL, {"token_hash": hashlib.sha256(token.encode()).digest()}
            )
            row = result.mappings().one_or_none()
    except (OSError, TimeoutError) as error:
        logger.error(
            "session_database_transport_failed",
            extra={"request_id": request.state.request_id, "failure_type": type(error).__name__},
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
            async with database.engine.begin() as connection:
                await connection.execute(
                    text("""
                        UPDATE user_sessions SET last_seen_at = CURRENT_TIMESTAMP
                        WHERE id = :session_id AND revoked_at IS NULL
                          AND expires_at > CURRENT_TIMESTAMP
                          AND last_seen_at < CURRENT_TIMESTAMP - INTERVAL '5 minutes'
                    """),
                    {"session_id": row["session_id"]},
                )
        except (SQLAlchemyError, OSError, TimeoutError) as error:
            logger.warning(
                "session_touch_failed",
                extra={
                    "request_id": request.state.request_id,
                    "failure_type": type(error).__name__,
                },
            )
    return {**dict(row), "token_hash": hashlib.sha256(token.encode()).digest()}


async def current_user(request: Request, database: Database, settings: Settings) -> dict[str, Any]:
    row = await resolve_session(request, database, settings)
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
