import uuid

from fastapi import APIRouter, Request
from starlette.responses import JSONResponse

from monkeybot_api.api.authentication import require_bot
from monkeybot_api.api.body import read_json
from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.sessions import resolve_session
from monkeybot_api.api.validation import guild_path
from monkeybot_api.core.errors import success
from monkeybot_api.core.security import require_csrf
from monkeybot_api.db.transactions import mutation
from monkeybot_api.repositories.guilds.settings import replace_catalog
from monkeybot_api.schemas.guilds.settings import Catalog, UpdateSettings
from monkeybot_api.services.guilds import settings as service

router = APIRouter()


@router.put("/internal/guilds/{guild_id}/discord-catalog")
async def catalog(
    request: Request, guild_id: str, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    guild_path(guild_id)
    require_bot(request, settings)
    body = await read_json(request, Catalog)
    service.normalize_catalog(guild_id, body)
    return success(request, await mutation(database, lambda c: replace_catalog(c, guild_id, body)))


@router.api_route("/api/guilds/{guild_id}/settings", methods=["GET", "HEAD"])
async def get_settings(
    request: Request, guild_id: str, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    guild_path(guild_id)
    principal = await resolve_session(request, database, settings)
    return success(request, await service.get_settings(database, guild_id, principal["user_id"]))


@router.put("/api/guilds/{guild_id}/settings")
async def put_settings(
    request: Request, guild_id: str, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    guild_path(guild_id)
    principal = await resolve_session(request, database, settings)
    body = await read_json(request, UpdateSettings)
    require_csrf(request, settings, principal["csrf_secret_hash"])
    service.normalize_settings(guild_id, body)
    p = {
        "guild": guild_id,
        "actor_id": principal["user_id"],
        "request_id": request.state.request_id,
        "audit_id": uuid.uuid7(),
    }
    return success(request, await mutation(database, lambda c: service.update_settings(c, p, body)))
