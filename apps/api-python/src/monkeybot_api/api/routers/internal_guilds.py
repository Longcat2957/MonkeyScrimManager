from fastapi import APIRouter, Request
from starlette.responses import JSONResponse

from monkeybot_api.api.authentication import require_bot
from monkeybot_api.api.body import read_json
from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.requests import command_key
from monkeybot_api.api.validation import guild_path
from monkeybot_api.core.errors import success
from monkeybot_api.schemas.internal_guilds import RegisterGuildMember, SetupGuild
from monkeybot_api.services import internal_guilds as service

router = APIRouter()


@router.post("/internal/guilds/{guild_id}/setup")
async def setup_guild(
    request: Request, guild_id: str, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    guild_path(guild_id)
    require_bot(request, settings)
    body = await read_json(request, SetupGuild)
    key = command_key(request, True)
    return success(
        request,
        await service.setup_guild(database, guild_id, key, request.state.request_id, body),
    )


@router.post("/internal/guilds/{guild_id}/registrations")
async def register_guild_member(
    request: Request, guild_id: str, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    guild_path(guild_id)
    require_bot(request, settings)
    body = await read_json(request, RegisterGuildMember)
    key = command_key(request, True)
    return success(
        request, await service.register_guild_member(database, settings, guild_id, key, body)
    )
