"""HTTP boundary for the member-visible season record search."""

from fastapi import APIRouter, Request
from starlette.responses import JSONResponse

from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.sessions import resolve_session
from monkeybot_api.api.validation import guild_path
from monkeybot_api.core.errors import ApiError, success
from monkeybot_api.schemas.guilds.record_search import SearchQuery
from monkeybot_api.services.guilds.record_search import search

router = APIRouter()


@router.api_route("/api/guilds/{guild_id}/records/search", methods=["GET", "HEAD"])
async def records_search(
    request: Request,
    guild_id: str,
    settings: SettingsDep,
    database: DatabaseDep,
) -> JSONResponse:
    guild_path(guild_id)
    principal = await resolve_session(request, database, settings)
    pairs = list(request.query_params.multi_items())
    parameters = dict(pairs)
    if (
        len(parameters) != len(pairs)
        or "q" not in parameters
        or set(parameters) - {"q", "seasonId", "cursor", "limit"}
    ):
        raise ApiError.from_status(400)
    query, season, cursor, limit = SearchQuery.model_validate(parameters).parameters()
    return success(
        request,
        await search(
            database,
            guild_id,
            principal["user_id"],
            query,
            season,
            cursor,
            limit,
            request.state.request_id,
        ),
    )
