from fastapi import APIRouter, Request
from starlette.responses import Response

from monkeybot_api.api.body import read_json
from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.paths import path
from monkeybot_api.api.requests import command_key
from monkeybot_api.api.sessions import resolve_session
from monkeybot_api.core.errors import success
from monkeybot_api.core.legacy_json import parse_float, parse_int
from monkeybot_api.core.security import require_csrf
from monkeybot_api.core.text import trim
from monkeybot_api.schemas.auctions.finish import Finish, error
from monkeybot_api.schemas.bot import invalid_bot
from monkeybot_api.services.auctions import finish as service

BASE = "/api/guilds/{guild_id}/auction-tournaments/{tournament_id}"
router = APIRouter()


@router.api_route(BASE + "/finish-preview", methods=["GET", "HEAD"])
async def preview(
    request: Request,
    guild_id: str,
    tournament_id: str,
    *,
    settings: SettingsDep,
    database: DatabaseDep,
) -> Response:
    identifier = path(guild_id, tournament_id)
    assert identifier is not None
    principal = await resolve_session(request, database, settings)
    return success(
        request, await service.preview(database, guild_id, identifier, principal["user_id"])
    )


@router.post(BASE + "/finish")
async def finish(
    request: Request,
    guild_id: str,
    tournament_id: str,
    *,
    settings: SettingsDep,
    database: DatabaseDep,
) -> Response:
    identifier = path(guild_id, tournament_id)
    assert identifier is not None
    principal = await resolve_session(request, database, settings)
    body = await read_json(request, Finish, parse_float=parse_float, parse_int=parse_int)
    if body.expectedRevision < 0:
        raise error("invalid_auction_tournament_finish_request")
    if not trim(body.reason) or len(body.reason) > 240:
        raise invalid_bot("reason")
    require_csrf(request, settings, principal["csrf_secret_hash"])
    key = command_key(request, False)
    result = await service.finish(
        database,
        guild_id,
        identifier,
        principal["user_id"],
        body.expectedRevision,
        body.reason,
        key,
        request.state.request_id,
    )
    return success(request, result)
