from fastapi import APIRouter, Request
from starlette.responses import JSONResponse

from monkeybot_api.api.body import read_json
from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.requests import command_key
from monkeybot_api.api.sessions import resolve_session
from monkeybot_api.api.validation import guild_path
from monkeybot_api.core.errors import ApiError, success
from monkeybot_api.core.security import require_csrf
from monkeybot_api.schemas.guilds.seasons import RotateSeason
from monkeybot_api.services.guilds import seasons as service

router = APIRouter()


@router.api_route("/api/guilds/{guild_id}/seasons", methods=["GET", "HEAD"])
async def seasons(
    request: Request, guild_id: str, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    guild_path(guild_id)
    principal = await resolve_session(request, database, settings)
    return success(request, await service.list_seasons(database, guild_id, principal["user_id"]))


@router.post("/api/guilds/{guild_id}/seasons")
async def rotate_season(
    request: Request, guild_id: str, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    guild_path(guild_id)
    principal = await resolve_session(request, database, settings)
    body = await read_json(request, RotateSeason)
    require_csrf(request, settings, principal["csrf_secret_hash"])
    try:
        key = command_key(request, False)
    except ApiError as exc:
        raise ApiError(
            exc.status, exc.code, "Idempotency-Key가 올바르지 않습니다.", exc.details
        ) from None
    return success(
        request,
        await service.rotate_season(
            database, guild_id, principal["user_id"], request.state.request_id, key, body
        ),
    )
