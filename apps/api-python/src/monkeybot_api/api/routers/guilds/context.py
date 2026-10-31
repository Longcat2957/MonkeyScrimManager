from fastapi import APIRouter, Request
from starlette.responses import Response

from monkeybot_api.api.body import read_json
from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.sessions import resolve_session
from monkeybot_api.api.validation import guild_path
from monkeybot_api.core.errors import success
from monkeybot_api.core.security import require_csrf
from monkeybot_api.schemas.guilds.context import DefaultGuildInput
from monkeybot_api.services.guilds import context as service

router = APIRouter()


@router.api_route("/api/me/guilds", methods=["GET", "HEAD"])
async def list_guilds(request: Request, settings: SettingsDep, database: DatabaseDep) -> Response:
    principal = await resolve_session(request, database, settings)
    return success(request, await service.list_guilds(database, principal["user_id"]))


@router.api_route("/api/guilds/{guild_id}", methods=["GET", "HEAD"])
async def guild(
    request: Request, guild_id: str, settings: SettingsDep, database: DatabaseDep
) -> Response:
    # Preserve the path validation before authentication.
    guild_path(guild_id)
    principal = await resolve_session(request, database, settings)
    return success(request, await service.get_guild(database, principal["user_id"], guild_id))


@router.put("/api/me/default-guild")
async def default_guild(request: Request, settings: SettingsDep, database: DatabaseDep) -> Response:
    principal = await resolve_session(request, database, settings)
    command = await read_json(request, DefaultGuildInput)
    require_csrf(request, settings, principal["csrf_secret_hash"])
    return success(
        request, await service.set_default_guild(database, principal["user_id"], command.guildId)
    )
