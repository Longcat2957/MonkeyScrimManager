from fastapi import APIRouter, Request
from starlette.responses import Response

from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.sessions import resolve_session
from monkeybot_api.api.validation import guild_path
from monkeybot_api.core.errors import success
from monkeybot_api.services.guilds import leaderboard as service

router = APIRouter()


@router.api_route("/api/guilds/{guild_id}/leaderboard", methods=["GET", "HEAD"])
async def leaderboard(
    request: Request, guild_id: str, settings: SettingsDep, database: DatabaseDep
) -> Response:
    guild_path(guild_id)
    principal = await resolve_session(request, database, settings)
    return success(
        request,
        await service.get_leaderboard(
            database, guild_id, principal["user_id"], request.state.request_id
        ),
    )
