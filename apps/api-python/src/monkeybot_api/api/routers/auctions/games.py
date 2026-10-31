from fastapi import APIRouter, Request
from starlette.responses import Response

from monkeybot_api.api.body import read_json
from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.paths import path
from monkeybot_api.api.requests import command_key
from monkeybot_api.api.sessions import resolve_session
from monkeybot_api.core.config import Settings
from monkeybot_api.core.errors import success
from monkeybot_api.core.security import require_csrf
from monkeybot_api.db.database import Database
from monkeybot_api.schemas.auctions.games import Submit, validate
from monkeybot_api.schemas.common import Revision
from monkeybot_api.services.auctions import commands as workflows

BASE = "/api/guilds/{guild_id}/auction-matches/{match_id}"


router = APIRouter()


async def execute(
    request: Request,
    guild_id: str,
    match_id: str,
    action: str,
    *,
    settings: Settings,
    database: Database,
) -> Response:
    identifier = path(guild_id, match_id)
    principal = await resolve_session(request, database, settings)
    parsed = await read_json(request, Submit if action == "submit" else Revision)
    body = parsed.model_dump()
    # Frozen Rust handlers validate a negative revision before checking CSRF.
    if body["expectedRevision"] < 0:
        validate(body, action)
    require_csrf(request, settings, principal["csrf_secret_hash"])
    key = command_key(request, False)
    validate(body, action)
    result = await workflows.game(
        database,
        guild_id,
        identifier,
        principal["user_id"],
        action,
        body,
        key,
        request.method,
        request.state.request_id,
    )
    response = success(request, result)
    response.status_code = 201 if action == "submit" else 200
    return response


@router.post(BASE + "/games")
async def submit(
    request: Request, guild_id: str, match_id: str, *, settings: SettingsDep, database: DatabaseDep
) -> Response:
    return await execute(
        request, guild_id, match_id, "submit", settings=settings, database=database
    )


@router.delete(BASE + "/games/last")
async def last(
    request: Request, guild_id: str, match_id: str, *, settings: SettingsDep, database: DatabaseDep
) -> Response:
    return await execute(request, guild_id, match_id, "last", settings=settings, database=database)


@router.post(BASE + "/complete")
async def complete(
    request: Request, guild_id: str, match_id: str, *, settings: SettingsDep, database: DatabaseDep
) -> Response:
    return await execute(
        request, guild_id, match_id, "complete", settings=settings, database=database
    )
