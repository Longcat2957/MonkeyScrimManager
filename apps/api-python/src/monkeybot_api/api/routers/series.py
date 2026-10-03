import re

from fastapi import APIRouter, Request
from pydantic import BaseModel
from starlette.responses import Response

from monkeybot_api.api.body import read_json
from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.paths import path_ids
from monkeybot_api.api.requests import command_key
from monkeybot_api.api.sessions import resolve_session
from monkeybot_api.core.config import Settings, is_snowflake
from monkeybot_api.core.errors import ApiError, success
from monkeybot_api.core.security import require_csrf
from monkeybot_api.db.database import Database
from monkeybot_api.schemas.common import Revision
from monkeybot_api.schemas.series import Early, PickBan, Submit, invalid
from monkeybot_api.services.series.commands import execute_command
from monkeybot_api.services.series.queries import list_series, read_series

router = APIRouter()


async def execute(
    request: Request,
    guild_id: str,
    series_id: str,
    action: str,
    *,
    settings: Settings,
    database: Database,
) -> Response:
    identifier = path_ids(guild_id, series_id)
    principal = await resolve_session(request, database, settings)
    models: dict[str, type[BaseModel]] = {
        "pickban": PickBan,
        "submit": Submit,
        "complete": Early,
    }
    model = models.get(action, Revision)
    parsed: BaseModel = await read_json(request, model)
    body = parsed.model_dump()
    if body["expectedRevision"] < 0:
        raise invalid(action)
    require_csrf(request, settings, principal["csrf_secret_hash"])
    key = command_key(request, False) if action != "pickban" else ""
    result = await execute_command(
        database,
        guild_id,
        identifier,
        principal["user_id"],
        request.state.request_id,
        request.method,
        action,
        key,
        body,
    )
    response = success(request, result)
    response.status_code = 201 if action == "submit" else 200
    return response


@router.patch("/api/guilds/{guild_id}/series/{series_id}/pickban")
async def pickban(
    request: Request, guild_id: str, series_id: str, *, settings: SettingsDep, database: DatabaseDep
) -> Response:
    return await execute(
        request, guild_id, series_id, "pickban", settings=settings, database=database
    )


@router.post("/api/guilds/{guild_id}/series/{series_id}/games")
async def submit(
    request: Request, guild_id: str, series_id: str, *, settings: SettingsDep, database: DatabaseDep
) -> Response:
    return await execute(
        request, guild_id, series_id, "submit", settings=settings, database=database
    )


@router.delete("/api/guilds/{guild_id}/series/{series_id}/games/last")
async def last(
    request: Request, guild_id: str, series_id: str, *, settings: SettingsDep, database: DatabaseDep
) -> Response:
    return await execute(request, guild_id, series_id, "last", settings=settings, database=database)


@router.post("/api/guilds/{guild_id}/series/{series_id}/complete")
async def early(
    request: Request, guild_id: str, series_id: str, *, settings: SettingsDep, database: DatabaseDep
) -> Response:
    return await execute(
        request, guild_id, series_id, "complete", settings=settings, database=database
    )


@router.post("/api/guilds/{guild_id}/series/{series_id}/revert")
async def revert(
    request: Request, guild_id: str, series_id: str, *, settings: SettingsDep, database: DatabaseDep
) -> Response:
    return await execute(
        request, guild_id, series_id, "revert", settings=settings, database=database
    )


@router.delete("/api/guilds/{guild_id}/series/{series_id}")
async def delete(
    request: Request, guild_id: str, series_id: str, *, settings: SettingsDep, database: DatabaseDep
) -> Response:
    return await execute(
        request, guild_id, series_id, "delete", settings=settings, database=database
    )


async def read(
    request: Request,
    guild_id: str,
    series_id: str,
    result: bool,
    *,
    settings: Settings,
    database: Database,
) -> Response:
    identifier = path_ids(guild_id, series_id)
    principal = await resolve_session(request, database, settings)
    return success(
        request, await read_series(database, guild_id, identifier, principal["user_id"], result)
    )


@router.api_route("/api/guilds/{guild_id}/series/{series_id}", methods=["GET", "HEAD"])
async def detail(
    request: Request, guild_id: str, series_id: str, *, settings: SettingsDep, database: DatabaseDep
) -> Response:
    return await read(request, guild_id, series_id, False, settings=settings, database=database)


@router.api_route("/api/guilds/{guild_id}/series/{series_id}/result", methods=["GET", "HEAD"])
async def result(
    request: Request, guild_id: str, series_id: str, *, settings: SettingsDep, database: DatabaseDep
) -> Response:
    return await read(request, guild_id, series_id, True, settings=settings, database=database)


@router.api_route("/api/guilds/{guild_id}/series", methods=["GET", "HEAD"])
async def listing(
    request: Request, guild_id: str, *, settings: SettingsDep, database: DatabaseDep
) -> Response:
    if not is_snowflake(guild_id):
        raise ApiError.from_status(400)
    pairs = list(request.query_params.multi_items())
    q = dict(pairs)
    if (
        len(pairs) != len(q)
        or set(q) - {"state", "seasonId", "cursor", "limit"}
        or q.get("state") not in {"active", "completed"}
        or (
            "limit" in q and (not re.fullmatch(r"\+?[0-9]+", q["limit"]) or int(q["limit"]) > 65535)
        )
    ):
        raise ApiError.from_status(400)
    principal = await resolve_session(request, database, settings)
    return success(request, await list_series(database, guild_id, principal["user_id"], q))
