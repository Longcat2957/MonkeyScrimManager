from fastapi import APIRouter, Request
from starlette.responses import JSONResponse

from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.sessions import resolve_session
from monkeybot_api.api.validation import guild_path
from monkeybot_api.core.errors import ApiError, success
from monkeybot_api.schemas.guilds.record_profile import ProfileQuery, validated_query
from monkeybot_api.services.guilds.record_profile import get_profile

router = APIRouter()


@router.api_route("/api/guilds/{guild_id}/records/profile", methods=["GET", "HEAD"])
async def profile(
    request: Request, guild_id: str, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    guild_path(guild_id)
    principal = await resolve_session(request, database, settings)
    pairs = list(request.query_params.multi_items())
    query = dict(pairs)
    if len(query) != len(pairs) or set(query) != {"q"}:
        raise ApiError.from_status(400)
    parameters = ProfileQuery.model_validate(query)
    return success(
        request,
        await get_profile(
            database,
            guild_id,
            principal["user_id"],
            validated_query(parameters.q),
            request.state.request_id,
        ),
    )
