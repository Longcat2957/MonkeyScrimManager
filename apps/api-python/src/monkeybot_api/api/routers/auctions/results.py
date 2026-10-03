from fastapi import APIRouter, Request
from starlette.responses import Response

from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.paths import path
from monkeybot_api.api.sessions import resolve_session
from monkeybot_api.core.errors import success
from monkeybot_api.services.auctions import reads

router = APIRouter()


@router.api_route(
    "/api/guilds/{guild_id}/auction-tournaments/{tournament_id}/result", methods=["GET", "HEAD"]
)
async def result(
    request: Request,
    guild_id: str,
    tournament_id: str,
    *,
    settings: SettingsDep,
    database: DatabaseDep,
) -> Response:
    identifier = path(guild_id, tournament_id)
    assert identifier is not None
    principal = await resolve_session(request, database, settings)
    result = await reads.tournament_result(
        database, guild_id, identifier, principal["user_id"], request.state.request_id
    )
    return success(request, result)
