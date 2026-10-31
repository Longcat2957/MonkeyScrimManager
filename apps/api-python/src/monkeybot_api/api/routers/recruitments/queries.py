import re
import uuid

from fastapi import APIRouter, Request
from starlette.responses import Response

from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.sessions import resolve_session
from monkeybot_api.core.config import is_snowflake
from monkeybot_api.core.errors import ApiError, success
from monkeybot_api.core.pagination import decode_cursor
from monkeybot_api.services.recruitments.queries import get_recruitment, list_recruitments


def actionable_query(request: Request) -> dict[str, str]:
    pairs = list(request.query_params.multi_items())
    query = dict(pairs)
    if len(query) != len(pairs) or set(query) - {"state", "cursor", "limit"}:
        raise ApiError.from_status(400)
    if query.get("state") != "actionable":
        raise ApiError.from_status(400)
    if "limit" in query and (
        not re.fullmatch(r"\+?[0-9]+", query["limit"]) or int(query["limit"]) > 65535
    ):
        raise ApiError.from_status(400)
    return query


router = APIRouter()


@router.api_route("/api/guilds/{guild_id}/recruitments", methods=["GET", "HEAD"])
async def recruitments(
    request: Request, guild_id: str, *, settings: SettingsDep, database: DatabaseDep
) -> Response:
    if not is_snowflake(guild_id):
        raise ApiError.from_status(400)
    query = actionable_query(request)
    principal = await resolve_session(request, database, settings)
    limit = int(query.get("limit", "50"))
    if not 1 <= limit <= 100:
        raise ApiError(400, "invalid_limit", "limit은 1 이상 100 이하이어야 합니다.")
    cursor_time, cursor_id = decode_cursor(query["cursor"]) if "cursor" in query else (None, None)
    return success(
        request,
        await list_recruitments(
            database,
            guild_id,
            principal["user_id"],
            limit,
            cursor_time,
            cursor_id,
        ),
    )


@router.api_route("/api/guilds/{guild_id}/recruitments/{recruitment_id}", methods=["GET", "HEAD"])
async def recruitment(
    request: Request,
    guild_id: str,
    recruitment_id: str,
    *,
    settings: SettingsDep,
    database: DatabaseDep,
) -> Response:
    if not is_snowflake(guild_id):
        raise ApiError.from_status(400)
    try:
        record_id = uuid.UUID(recruitment_id)
    except ValueError:
        raise ApiError.from_status(400) from None
    principal = await resolve_session(request, database, settings)
    return success(
        request,
        await get_recruitment(
            database,
            guild_id,
            principal["user_id"],
            record_id,
        ),
    )
