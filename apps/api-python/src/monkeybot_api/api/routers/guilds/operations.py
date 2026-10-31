from fastapi import APIRouter, Request
from starlette.responses import JSONResponse

from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.sessions import resolve_session
from monkeybot_api.api.validation import guild_path
from monkeybot_api.core.errors import success
from monkeybot_api.services.guilds.operations import list_operations

router = APIRouter()


@router.api_route("/api/guilds/{guild_id}/operations", methods=["GET", "HEAD"])
async def operations(
    request: Request, guild_id: str, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    guild_path(guild_id)
    principal = await resolve_session(request, database, settings)
    return success(
        request,
        await list_operations(database, guild_id, principal["user_id"], request.state.request_id),
    )
