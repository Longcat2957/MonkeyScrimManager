"""HTTP boundary for complete guild data export/import."""

from fastapi import APIRouter, Request
from starlette.responses import JSONResponse

from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.requests import command_json, command_key
from monkeybot_api.api.sessions import resolve_session
from monkeybot_api.api.validation import guild_path
from monkeybot_api.core.errors import ApiError, success
from monkeybot_api.core.security import require_csrf
from monkeybot_api.services.guilds import full_data_import
from monkeybot_api.services.guilds.full_data_transfer import export_full_data

router = APIRouter()


@router.post("/api/guilds/{guild_id}/full-data-export")
async def export(
    request: Request, guild_id: str, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    guild_path(guild_id)
    principal = await resolve_session(request, database, settings)
    require_csrf(request, settings, principal["csrf_secret_hash"])
    return success(
        request,
        await export_full_data(database, guild_id, principal["user_id"], request.state.request_id),
    )


@router.post("/api/guilds/{guild_id}/full-data-imports/preview")
async def preview(
    request: Request, guild_id: str, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    guild_path(guild_id)
    principal = await resolve_session(request, database, settings)
    body = await command_json(request)
    require_csrf(request, settings, principal["csrf_secret_hash"])
    if not isinstance(body, dict) or "archive" not in body or set(body) != {"archive"}:
        raise full_data_import.invalid_archive()
    return success(
        request,
        await full_data_import.preview(
            database,
            guild_id,
            principal["user_id"],
            request.state.request_id,
            body["archive"],
        ),
    )


@router.post("/api/guilds/{guild_id}/full-data-imports/{import_id}/apply")
async def apply(
    request: Request,
    guild_id: str,
    import_id: str,
    settings: SettingsDep,
    database: DatabaseDep,
) -> JSONResponse:
    guild_path(guild_id)
    principal = await resolve_session(request, database, settings)
    body = await command_json(request)
    require_csrf(request, settings, principal["csrf_secret_hash"])
    try:
        key = command_key(request, False)
    except ApiError as exc:
        raise ApiError(
            exc.status, exc.code, "Idempotency-Key가 올바르지 않습니다.", exc.details
        ) from None
    return success(
        request,
        await full_data_import.apply(
            database,
            guild_id,
            principal["user_id"],
            request.state.request_id,
            import_id,
            key,
            body,
        ),
    )
