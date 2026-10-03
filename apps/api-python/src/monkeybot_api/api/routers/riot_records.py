"""Public Riot records reads and authenticated refresh."""

import re

from fastapi import APIRouter, Request
from starlette.responses import JSONResponse

from monkeybot_api.api.body import read_json
from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.sessions import resolve_session
from monkeybot_api.core.errors import success
from monkeybot_api.core.security import require_csrf
from monkeybot_api.schemas.riot_records import RecordsRequest
from monkeybot_api.services import riot_records as service

router = APIRouter()


def _query(request: Request) -> tuple[str, str, str]:
    values = request.query_params
    game_name = values.get("gameName")
    tag_line = values.get("tagLine")
    platform = values.get("platform")
    if (
        game_name is None
        or tag_line is None
        or platform is None
        or set(values) - {"gameName", "tagLine", "platform", "offset", "count"}
        or len(values.multi_items()) != len(set(values))
    ):
        raise service.invalid_request()
    return game_name, tag_line, platform


@router.api_route("/api/riot/records", methods=["GET", "HEAD"])
async def records(request: Request, settings: SettingsDep, database: DatabaseDep) -> JSONResponse:
    game_name, tag_line, platform = _query(request)
    if "offset" in request.query_params or "count" in request.query_params:
        raise service.invalid_request()
    return success(
        request, await service.records(database, settings, game_name, tag_line, platform)
    )


@router.api_route("/api/riot/records/matches", methods=["GET", "HEAD"])
async def matches(request: Request, settings: SettingsDep, database: DatabaseDep) -> JSONResponse:
    game_name, tag_line, platform = _query(request)
    raw_offset = request.query_params.get("offset", "0")
    raw_count = request.query_params.get("count", "10")
    if not re.fullmatch(r"\+?[0-9]+", raw_offset) or not re.fullmatch(r"\+?[0-9]+", raw_count):
        raise service.invalid_request() from None
    try:
        offset = int(raw_offset)
        count = int(raw_count)
    except ValueError:
        raise service.invalid_request() from None
    return success(
        request,
        await service.recent_matches(
            database, settings, game_name, tag_line, platform, offset, count
        ),
    )


@router.post("/api/riot/records/refresh")
async def refresh(request: Request, settings: SettingsDep, database: DatabaseDep) -> JSONResponse:
    principal = await resolve_session(request, database, settings)
    body = await read_json(request, RecordsRequest)
    require_csrf(request, settings, principal["csrf_secret_hash"])
    return success(
        request,
        await service.records(
            database, settings, body.gameName, body.tagLine, body.platform, refresh=True
        ),
    )
