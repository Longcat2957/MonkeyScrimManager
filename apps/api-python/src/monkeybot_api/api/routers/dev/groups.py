"""Development-only dummy-user group routes."""

import uuid

from fastapi import APIRouter, Request
from starlette.responses import JSONResponse

from monkeybot_api.api.body import read_json
from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.dev_common import authorize, authorize_mutation, command_key
from monkeybot_api.api.sessions import resolve_session
from monkeybot_api.core.errors import ApiError, success
from monkeybot_api.schemas.dev.groups import CreateGroup, ReplaceGroupMembers, UpdateGroup
from monkeybot_api.services.dev import groups as service

router = APIRouter()


def parse_group_id(value: str) -> uuid.UUID:
    try:
        return uuid.UUID(value)
    except ValueError:
        raise ApiError.from_status(400) from None


def key(request: Request) -> str:
    try:
        return command_key(request)
    except ApiError as error:
        raise ApiError(
            error.status, error.code, "Idempotency-Key가 올바르지 않습니다.", error.details
        ) from None


@router.api_route("/api/dev/dummy-groups", methods=["GET", "HEAD"])
async def list_groups(
    request: Request, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    principal = await resolve_session(request, database, settings)
    authorize(principal, settings)
    return success(request, await service.list_groups(database, request.state.request_id))


@router.post("/api/dev/dummy-groups")
async def create_group(
    request: Request, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    principal = await resolve_session(request, database, settings)
    body = await read_json(request, CreateGroup)
    actor = authorize_mutation(request, principal, settings)
    result = await service.create_group(
        database, actor, request.state.request_id, key(request), body
    )
    response = success(request, result)
    response.status_code = 201
    return response


@router.patch("/api/dev/dummy-groups/{group_id}")
async def update_group(
    request: Request, group_id: str, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    identifier = parse_group_id(group_id)
    principal = await resolve_session(request, database, settings)
    body = await read_json(request, UpdateGroup)
    actor = authorize_mutation(request, principal, settings)
    return success(
        request,
        await service.update_group(
            database, identifier, actor, request.state.request_id, key(request), body
        ),
    )


@router.put("/api/dev/dummy-groups/{group_id}/members")
async def replace_members(
    request: Request, group_id: str, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    identifier = parse_group_id(group_id)
    principal = await resolve_session(request, database, settings)
    body = await read_json(request, ReplaceGroupMembers)
    actor = authorize_mutation(request, principal, settings)
    return success(
        request,
        await service.replace_members(
            database, identifier, actor, request.state.request_id, key(request), body
        ),
    )


@router.delete("/api/dev/dummy-groups/{group_id}")
async def delete_group(
    request: Request, group_id: str, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    identifier = parse_group_id(group_id)
    principal = await resolve_session(request, database, settings)
    actor = authorize_mutation(request, principal, settings)
    return success(
        request,
        await service.delete_group(
            database, identifier, actor, request.state.request_id, key(request)
        ),
    )
