from fastapi import APIRouter, Request
from pydantic import ValidationError
from starlette.responses import Response

from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.paths import path
from monkeybot_api.api.requests import command_json, command_key
from monkeybot_api.api.sessions import resolve_session
from monkeybot_api.core.errors import success
from monkeybot_api.core.security import require_csrf
from monkeybot_api.schemas.auctions.bracket import COMMAND, invalid
from monkeybot_api.services.auctions import commands as workflows
from monkeybot_api.services.auctions import reads

BASE = "/api/guilds/{guild_id}/auction-tournaments/{tournament_id}/bracket"


router = APIRouter()


@router.api_route(BASE, methods=["GET", "HEAD"])
async def detail(
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
    result = await reads.bracket_detail(database, guild_id, identifier, principal["user_id"])
    return success(request, result)


@router.post(BASE + "/commands")
async def commands(
    request: Request,
    guild_id: str,
    tournament_id: str,
    *,
    settings: SettingsDep,
    database: DatabaseDep,
) -> Response:
    identifier = path(guild_id, tournament_id)
    principal = await resolve_session(request, database, settings)
    raw = await command_json(request)
    try:
        parsed = COMMAND.validate_python(raw).model_dump()
    except ValidationError:
        raise invalid() from None
    if parsed["expectedRevision"] < 0:
        raise invalid()
    # Preserve the frozen Serde field order and UUID normalization for stored request hashes.
    body = {
        key: parsed[key]
        for key in ("kind", "expectedRevision", "slotKey", "teamId")
        if key in parsed
    }
    require_csrf(request, settings, principal["csrf_secret_hash"])
    key = command_key(request, False)
    result = await workflows.bracket_command(
        database, guild_id, identifier, principal["user_id"], body, key, request.state.request_id
    )
    return success(request, result)
