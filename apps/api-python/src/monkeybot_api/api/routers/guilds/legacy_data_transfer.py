"""Legacy member and season MMR seed transfer endpoints."""

from fastapi import APIRouter, Request
from starlette.responses import JSONResponse

from monkeybot_api.api.body import read_json
from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.paths import path_ids
from monkeybot_api.api.requests import command_key
from monkeybot_api.api.sessions import resolve_session
from monkeybot_api.api.validation import guild_path
from monkeybot_api.core.errors import ApiError, success
from monkeybot_api.core.legacy_json import parse_float, parse_int
from monkeybot_api.core.security import require_csrf
from monkeybot_api.schemas.guilds.legacy_data_transfer import ApplyCommand, PreviewRequest
from monkeybot_api.services.guilds import legacy_data_transfer as service

router = APIRouter()


@router.post("/api/guilds/{guild_id}/data-export")
async def export(
    request: Request, guild_id: str, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    guild_path(guild_id)
    principal = await resolve_session(request, database, settings)
    require_csrf(request, settings, principal["csrf_secret_hash"])
    return success(
        request,
        await service.export_archive(
            database, guild_id, principal["user_id"], request.state.request_id
        ),
    )


@router.post("/api/guilds/{guild_id}/data-imports/preview")
async def preview(
    request: Request, guild_id: str, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    guild_path(guild_id)
    principal = await resolve_session(request, database, settings)
    body = await read_json(request, PreviewRequest, parse_float=parse_float, parse_int=parse_int)
    require_csrf(request, settings, principal["csrf_secret_hash"])
    return success(
        request,
        await service.preview_archive(
            database, guild_id, principal["user_id"], request.state.request_id, body.archive
        ),
    )


@router.post("/api/guilds/{guild_id}/data-imports/{import_id}/apply")
async def apply(
    request: Request,
    guild_id: str,
    import_id: str,
    settings: SettingsDep,
    database: DatabaseDep,
) -> JSONResponse:
    identifier = path_ids(guild_id, import_id)
    principal = await resolve_session(request, database, settings)
    body = await read_json(request, ApplyCommand, parse_float=parse_float, parse_int=parse_int)
    require_csrf(request, settings, principal["csrf_secret_hash"])
    try:
        key = command_key(request, False)
    except ApiError as exc:
        raise ApiError(
            exc.status, exc.code, "Idempotency-Key가 올바르지 않습니다.", exc.details
        ) from None
    return success(
        request,
        await service.apply_archive(
            database,
            guild_id,
            principal["user_id"],
            request.state.request_id,
            identifier,
            key,
            body,
        ),
    )
