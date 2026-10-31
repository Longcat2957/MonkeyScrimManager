from typing import Any

from fastapi import Request

from monkeybot_api.core.config import Settings
from monkeybot_api.core.security import cookie_value
from monkeybot_api.db.database import Database
from monkeybot_api.services.auth import session as service


def session_cookie(request: Request) -> str | None:
    return cookie_value(request, "monkeybot_session")


async def resolve_session(
    request: Request, database: Database, settings: Settings
) -> dict[str, Any]:
    return await service.authenticate_session(
        database, settings, session_cookie(request), request.state.request_id
    )


async def current_user(request: Request, database: Database, settings: Settings) -> dict[str, Any]:
    row = await resolve_session(request, database, settings)
    return service.describe_user(row, settings)
