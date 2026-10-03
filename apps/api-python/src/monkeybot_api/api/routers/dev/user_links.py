"""Service-admin routes for dummy-user guild, champion, and Riot links."""

from fastapi import APIRouter, Request
from starlette.responses import JSONResponse

from monkeybot_api.api.body import read_json
from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.dev_common import authorize_mutation, command_key
from monkeybot_api.api.sessions import resolve_session
from monkeybot_api.core.errors import ApiError, success
from monkeybot_api.schemas.dev.user_links import (
    ReplaceChampionPreferences,
    UpsertMembership,
    UpsertRiotAccount,
)
from monkeybot_api.services.dev import user_links as service

router = APIRouter()


def key(request: Request) -> str:
    try:
        return command_key(request)
    except ApiError as error:
        raise ApiError(
            error.status, error.code, "Idempotency-Key가 올바르지 않습니다.", {"maxLength": 128}
        ) from None


@router.put("/api/dev/dummy-users/{user_id}/guilds/{guild_id}")
async def upsert_membership(
    request: Request,
    user_id: str,
    guild_id: str,
    settings: SettingsDep,
    database: DatabaseDep,
) -> JSONResponse:
    user = service.parse_user(user_id)
    guild = service.parse_guild(guild_id)
    principal = await resolve_session(request, database, settings)
    body = await read_json(request, UpsertMembership)
    actor = authorize_mutation(request, principal, settings)
    return success(
        request,
        await service.upsert_membership(
            database, user, guild, actor, request.state.request_id, key(request), body
        ),
    )


@router.delete("/api/dev/dummy-users/{user_id}/guilds/{guild_id}")
async def remove_membership(
    request: Request,
    user_id: str,
    guild_id: str,
    settings: SettingsDep,
    database: DatabaseDep,
) -> JSONResponse:
    user = service.parse_user(user_id)
    guild = service.parse_guild(guild_id)
    principal = await resolve_session(request, database, settings)
    actor = authorize_mutation(request, principal, settings)
    return success(
        request,
        await service.remove_membership(
            database, user, guild, actor, request.state.request_id, key(request)
        ),
    )


@router.put("/api/dev/dummy-users/{user_id}/champion-preferences/{lane}")
async def replace_preferences(
    request: Request,
    user_id: str,
    lane: str,
    settings: SettingsDep,
    database: DatabaseDep,
) -> JSONResponse:
    user = service.parse_user(user_id)
    principal = await resolve_session(request, database, settings)
    body = await read_json(request, ReplaceChampionPreferences)
    actor = authorize_mutation(request, principal, settings)
    parsed_lane = service.parse_lane(lane)
    return success(
        request,
        await service.replace_preferences(
            database, user, parsed_lane, actor, request.state.request_id, key(request), body
        ),
    )


@router.put("/api/dev/dummy-users/{user_id}/riot-account")
async def upsert_riot(
    request: Request,
    user_id: str,
    settings: SettingsDep,
    database: DatabaseDep,
) -> JSONResponse:
    user = service.parse_user(user_id)
    principal = await resolve_session(request, database, settings)
    body = await read_json(request, UpsertRiotAccount)
    actor = authorize_mutation(request, principal, settings)
    return success(
        request,
        await service.upsert_riot(
            database, user, actor, request.state.request_id, key(request), body
        ),
    )


@router.delete("/api/dev/dummy-users/{user_id}/riot-account")
async def remove_riot(
    request: Request,
    user_id: str,
    settings: SettingsDep,
    database: DatabaseDep,
) -> JSONResponse:
    user = service.parse_user(user_id)
    principal = await resolve_session(request, database, settings)
    actor = authorize_mutation(request, principal, settings)
    return success(
        request,
        await service.remove_riot(database, user, actor, request.state.request_id, key(request)),
    )
