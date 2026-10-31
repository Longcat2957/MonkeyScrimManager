"""HTTP boundary for member-visible guild season statistics."""

from fastapi import APIRouter, Request
from starlette.responses import JSONResponse

from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.sessions import resolve_session
from monkeybot_api.api.validation import guild_path
from monkeybot_api.core.errors import ApiError, success
from monkeybot_api.schemas.guilds.statistics import StatisticsQuery
from monkeybot_api.services.guilds.statistics import get_statistics

router = APIRouter()


@router.api_route("/api/guilds/{guild_id}/statistics", methods=["GET", "HEAD"])
async def statistics(
    request: Request, guild_id: str, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    guild_path(guild_id)
    pairs = list(request.query_params.multi_items())
    query = dict(pairs)
    if len(query) != len(pairs) or set(query) - {"seasonId", "scope"}:
        raise ApiError.from_status(400)
    parameters = StatisticsQuery.model_validate(query)
    # Preserve Rust extractor order: query shape, session, then UUID/scope values.
    principal = await resolve_session(request, database, settings)
    season, scope = parameters.parameters()
    return success(
        request,
        await get_statistics(
            database,
            guild_id,
            principal["user_id"],
            season,
            scope,
            request.state.request_id,
        ),
    )
