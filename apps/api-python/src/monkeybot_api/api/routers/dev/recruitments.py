"""Development-only dummy participant management for regular recruitments."""

import uuid

from fastapi import APIRouter, Request
from starlette.responses import JSONResponse

from monkeybot_api.api.body import read_json
from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.dev_common import (
    authorize,
    authorize_mutation,
    command_key,
)
from monkeybot_api.api.sessions import resolve_session
from monkeybot_api.api.validation import guild_path
from monkeybot_api.core.errors import ApiError, success
from monkeybot_api.schemas.dev.recruitments import ApplyParticipants, PreviewParticipants
from monkeybot_api.services.dev import recruitments as service

router = APIRouter()
BASE = "/api/dev/guilds/{guild_id}/recruitments/{recruitment_id}/participants"


def recruitment_uuid(value: str) -> uuid.UUID:
    try:
        return uuid.UUID(value)
    except ValueError:
        raise ApiError.from_status(400) from None


@router.api_route(BASE, methods=["GET", "HEAD"])
async def overview(
    request: Request,
    guild_id: str,
    recruitment_id: str,
    settings: SettingsDep,
    database: DatabaseDep,
) -> JSONResponse:
    principal = await resolve_session(request, database, settings)
    actor = authorize(principal, settings)
    guild_path(guild_id)
    recruitment = recruitment_uuid(recruitment_id)
    return success(request, await service.overview(database, guild_id, recruitment, actor))


@router.post(BASE + "/preview")
async def preview(
    request: Request,
    guild_id: str,
    recruitment_id: str,
    settings: SettingsDep,
    database: DatabaseDep,
) -> JSONResponse:
    principal = await resolve_session(request, database, settings)
    body = await read_json(request, PreviewParticipants)
    actor = authorize(principal, settings)
    guild_path(guild_id)
    recruitment = recruitment_uuid(recruitment_id)
    return success(request, await service.preview(database, guild_id, recruitment, actor, body))


@router.post(BASE)
async def add(
    request: Request,
    guild_id: str,
    recruitment_id: str,
    settings: SettingsDep,
    database: DatabaseDep,
) -> JSONResponse:
    return await _change(request, guild_id, recruitment_id, settings, database, "ADD")


@router.delete(BASE)
async def remove(
    request: Request,
    guild_id: str,
    recruitment_id: str,
    settings: SettingsDep,
    database: DatabaseDep,
) -> JSONResponse:
    return await _change(request, guild_id, recruitment_id, settings, database, "REMOVE")


async def _change(
    request: Request,
    guild_id: str,
    recruitment_id: str,
    settings: SettingsDep,
    database: DatabaseDep,
    action: service.Action,
) -> JSONResponse:
    principal = await resolve_session(request, database, settings)
    body = await read_json(request, ApplyParticipants)
    actor = authorize_mutation(request, principal, settings)
    guild_path(guild_id)
    recruitment = recruitment_uuid(recruitment_id)
    key = command_key(request)
    return success(
        request,
        await service.apply(
            database,
            guild_id,
            recruitment,
            actor,
            request.state.request_id,
            key,
            action,
            body,
        ),
    )
