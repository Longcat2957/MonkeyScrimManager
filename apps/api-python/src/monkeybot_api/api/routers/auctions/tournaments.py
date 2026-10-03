import re
from typing import Any

from fastapi import APIRouter, Request
from pydantic import ValidationError
from starlette.responses import Response

from monkeybot_api.api.authentication import command_principal
from monkeybot_api.api.body import read_json
from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.paths import path
from monkeybot_api.api.requests import command_json, command_key
from monkeybot_api.api.sessions import resolve_session
from monkeybot_api.core.config import Settings
from monkeybot_api.core.errors import ApiError, success
from monkeybot_api.core.security import require_csrf
from monkeybot_api.db.database import Database
from monkeybot_api.schemas.auctions.tournaments import COMMAND, Create, invalid
from monkeybot_api.services.auctions import commands as workflows
from monkeybot_api.services.auctions import reads

BASE = "/api/guilds/{guild_id}/auction-tournaments"


router = APIRouter()


async def write(
    request: Request, guild: str, identifier: str | None, *, settings: Settings, database: Database
) -> Response:
    record = path(guild, identifier)
    creating = identifier is None
    principal = await resolve_session(request, database, settings)
    body: dict[str, Any]
    if creating:
        parsed = await read_json(request, Create)
        # Serde serializes sourceRecruitmentId before expectedRevision.
        body = {
            "sourceRecruitmentId": parsed.sourceRecruitmentId,
            "expectedRevision": parsed.expectedRevision,
        }
    else:
        raw = await command_json(request)
        try:
            parsed_command = COMMAND.validate_python(raw).model_dump()
        except ValidationError:
            raise invalid() from None
        body = {
            key: parsed_command[key]
            for key in ("kind", "expectedRevision", "memberId", "teamId", "points")
            if key in parsed_command
        }
    if body["expectedRevision"] < 0:
        raise invalid(creating)
    require_csrf(request, settings, principal["csrf_secret_hash"])
    key = command_key(request, False)
    result = await workflows.tournament(
        database, guild, record, principal["user_id"], body, creating, key, request.state.request_id
    )
    response = success(request, result)
    response.status_code = 201 if creating else 200
    return response


@router.post(BASE)
async def create(
    request: Request, guild_id: str, *, settings: SettingsDep, database: DatabaseDep
) -> Response:
    return await write(request, guild_id, None, settings=settings, database=database)


@router.post(BASE + "/{tournament_id}/commands")
async def commands(
    request: Request,
    guild_id: str,
    tournament_id: str,
    *,
    settings: SettingsDep,
    database: DatabaseDep,
) -> Response:
    return await write(request, guild_id, tournament_id, settings=settings, database=database)


@router.api_route(BASE + "/{tournament_id}", methods=["GET", "HEAD"])
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
    result = await reads.tournament_detail(database, guild_id, identifier, principal["user_id"])
    return success(request, result)


@router.api_route(BASE, methods=["GET", "HEAD"])
async def listing(
    request: Request, guild_id: str, *, settings: SettingsDep, database: DatabaseDep
) -> Response:
    path(guild_id)
    pairs = list(request.query_params.multi_items())
    q = dict(pairs)
    if (
        len(q) != len(pairs)
        or set(q) - {"state", "seasonId", "cursor", "limit"}
        or q.get("state") not in {"active", "completed"}
    ):
        raise ApiError.from_status(400)
    if "limit" in q and (not re.fullmatch(r"\+?[0-9]+", q["limit"]) or int(q["limit"]) > 65535):
        raise ApiError.from_status(400)
    bot, principal = await command_principal(request, settings, database)
    viewer = None if bot else principal["user_id"] if principal else None
    result = await reads.tournament_list(database, guild_id, viewer, q)
    return success(request, result)
