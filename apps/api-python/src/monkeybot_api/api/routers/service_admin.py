"""HTTP boundaries for service-admin operational tools."""

from fastapi import APIRouter, Request
from starlette.responses import JSONResponse

from monkeybot_api.api.body import read_json
from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.sessions import resolve_session
from monkeybot_api.core.errors import ApiError, success
from monkeybot_api.core.security import require_csrf
from monkeybot_api.schemas.service_admin import ResetCommand
from monkeybot_api.services import service_admin as service

router = APIRouter()


@router.api_route("/api/admin/status", methods=["GET", "HEAD"])
async def status(request: Request, settings: SettingsDep, database: DatabaseDep) -> JSONResponse:
    principal = await resolve_session(request, database, settings)
    service.authorize(principal, settings)
    return success(request, await service.service_status(database))


@router.api_route("/api/admin/guilds/{guild_id}/record-reset", methods=["GET", "HEAD"])
async def reset_preview(
    request: Request, guild_id: str, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    principal = await resolve_session(request, database, settings)
    service.authorize(principal, settings, reset=True)
    service.validate_guild(guild_id)
    return success(request, await service.reset_preview(database, guild_id))


@router.post("/api/admin/guilds/{guild_id}/record-reset")
async def reset(
    request: Request, guild_id: str, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    principal = await resolve_session(request, database, settings)
    command = await read_json(request, ResetCommand)
    service.authorize(principal, settings, reset=True)
    require_csrf(request, settings, principal["csrf_secret_hash"])
    service.validate_guild(guild_id)
    if command.confirmation != f"RESET {guild_id}":
        raise ApiError(
            422, "reset_confirmation_mismatch", "확인 문구가 대상 서버와 일치하지 않습니다."
        )
    key = service.validate_key(request.headers.getlist("idempotency-key"))
    return success(
        request,
        await service.reset_records(
            database, guild_id, principal["user_id"], key, request.state.request_id
        ),
    )
