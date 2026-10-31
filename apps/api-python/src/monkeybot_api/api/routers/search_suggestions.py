"""HTTP boundaries for public Riot and member-visible guild autocomplete."""

from fastapi import APIRouter, Request
from starlette.responses import JSONResponse

from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.sessions import resolve_session
from monkeybot_api.api.validation import guild_path
from monkeybot_api.core.errors import success
from monkeybot_api.schemas.search_suggestions import GuildSuggestionQuery, PublicSuggestionQuery
from monkeybot_api.services.search_suggestions import guild_members, public_riot_accounts

router = APIRouter()


@router.api_route("/api/riot/records/suggestions", methods=["GET", "HEAD"])
async def riot_suggestions(request: Request, database: DatabaseDep) -> JSONResponse:
    query = PublicSuggestionQuery.from_pairs(list(request.query_params.multi_items()))
    prefix, limit = query.parameters(public=True)
    platform = query.normalized_platform()
    return success(request, await public_riot_accounts(database, platform, prefix, limit))


@router.api_route("/api/guilds/{guild_id}/records/suggestions", methods=["GET", "HEAD"])
async def guild_suggestions(
    request: Request, guild_id: str, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    guild_path(guild_id)
    # The frozen handler extracts the path and session before the query.
    principal = await resolve_session(request, database, settings)
    query = GuildSuggestionQuery.from_pairs(list(request.query_params.multi_items()))
    prefix, limit = query.parameters()
    return success(
        request, await guild_members(database, principal["user_id"], guild_id, prefix, limit)
    )
