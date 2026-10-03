from fastapi import APIRouter, Request
from pydantic import ValidationError
from starlette.responses import Response

from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.paths import path
from monkeybot_api.api.requests import command_json, command_key
from monkeybot_api.api.sessions import resolve_session
from monkeybot_api.core.config import Settings
from monkeybot_api.core.errors import ApiError, success
from monkeybot_api.core.security import require_csrf
from monkeybot_api.db.database import Database
from monkeybot_api.schemas.auctions.lineups import Assignment, error
from monkeybot_api.schemas.common import Revision
from monkeybot_api.services.auctions import commands as workflows
from monkeybot_api.services.auctions import reads

BASE = "/api/guilds/{guild_id}/auction-matches/{match_id}"


router = APIRouter()


@router.api_route(BASE, methods=["GET", "HEAD"])
async def detail(
    request: Request, guild_id: str, match_id: str, *, settings: SettingsDep, database: DatabaseDep
) -> Response:
    identifier = path(guild_id, match_id)
    assert identifier is not None
    principal = await resolve_session(request, database, settings)
    result = await reads.match_detail(
        database, guild_id, identifier, principal["user_id"], request.state.request_id
    )
    return success(request, result)


async def write(
    request: Request,
    guild_id: str,
    match_id: str,
    kind: str,
    *,
    settings: Settings,
    database: Database,
) -> Response:
    identifier = path(guild_id, match_id)
    principal = await resolve_session(request, database, settings)
    raw = await command_json(request)
    try:
        parsed = (Assignment if kind == "ASSIGN" else Revision).model_validate(raw).model_dump()
    except ValidationError:
        raise ApiError.from_status(422) from None
    if parsed["expectedRevision"] < 0:
        raise error("invalid_auction_match_lineup_request")
    require_csrf(request, settings, principal["csrf_secret_hash"])
    key = command_key(request, False)
    result = await workflows.lineup(
        database,
        guild_id,
        identifier,
        principal["user_id"],
        kind,
        parsed,
        key,
        request.state.request_id,
    )
    return success(request, result)


@router.patch(BASE + "/lineup")
async def assign(
    request: Request, guild_id: str, match_id: str, *, settings: SettingsDep, database: DatabaseDep
) -> Response:
    return await write(request, guild_id, match_id, "ASSIGN", settings=settings, database=database)


@router.post(BASE + "/lineup/lock")
async def lock(
    request: Request, guild_id: str, match_id: str, *, settings: SettingsDep, database: DatabaseDep
) -> Response:
    return await write(request, guild_id, match_id, "LOCK", settings=settings, database=database)


@router.post(BASE + "/lineup/unlock")
async def unlock(
    request: Request, guild_id: str, match_id: str, *, settings: SettingsDep, database: DatabaseDep
) -> Response:
    return await write(request, guild_id, match_id, "UNLOCK", settings=settings, database=database)
