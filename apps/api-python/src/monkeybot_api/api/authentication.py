import hmac
from typing import Any

from fastapi import Request

from monkeybot_api.api.sessions import resolve_session
from monkeybot_api.core.config import Settings
from monkeybot_api.core.errors import ApiError
from monkeybot_api.db.database import Database


def require_bot(request: Request, settings: Settings) -> None:
    token = settings.bot_service_token
    if token is None or not token.get_secret_value().strip():
        raise ApiError(503, "bot_service_unavailable", "Bot 인증 서비스를 사용할 수 없습니다.")
    headers = request.headers.getlist("authorization")
    if (
        len(headers) != 1
        or not headers[0].startswith("Bearer ")
        or not hmac.compare_digest(headers[0][7:].encode(), token.get_secret_value().encode())
    ):
        raise ApiError.from_status(401)


async def command_principal(
    request: Request, settings: Settings, database: Database
) -> tuple[bool, dict[str, Any] | None]:
    bot = "authorization" in request.headers
    principal = None
    if bot:
        require_bot(request, settings)
    else:
        principal = await resolve_session(request, database, settings)
    return bot, principal
