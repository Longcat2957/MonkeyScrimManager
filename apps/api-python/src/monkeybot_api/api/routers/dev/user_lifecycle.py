"""Service-admin dummy-user update, archive, restore, and deletion routes."""

from fastapi import APIRouter, Request
from starlette.responses import JSONResponse

from monkeybot_api.api.body import read_json
from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.dev_common import authorize, authorize_mutation, command_key
from monkeybot_api.api.sessions import resolve_session
from monkeybot_api.core.errors import ApiError, success
from monkeybot_api.schemas.dev.user_lifecycle import ExpectedRevision, UpdateUser
from monkeybot_api.services.dev import user_lifecycle as service
from monkeybot_api.services.dev.users import parse_user

router = APIRouter()


def _key(request: Request) -> str:
    try:
        return command_key(request)
    except ApiError as error:
        raise ApiError(
            error.status,
            error.code,
            "Idempotency-Key가 올바르지 않습니다.",
            {"maxLength": 128},
        ) from None


@router.api_route("/api/dev/dummy-users/{user_id}/deletion-impact", methods=["GET", "HEAD"])
async def deletion_impact(
    request: Request, user_id: str, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    principal = await resolve_session(request, database, settings)
    authorize(principal, settings)
    return success(request, await service.deletion_impact(database, parse_user(user_id)))


@router.delete("/api/dev/dummy-users/{user_id}")
async def delete_user(
    request: Request, user_id: str, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    principal = await resolve_session(request, database, settings)
    actor = authorize_mutation(request, principal, settings)
    result = await service.delete_user(
        database, parse_user(user_id), actor, request.state.request_id, _key(request)
    )
    return success(request, {"deleted": True, "impact": result})


@router.patch("/api/dev/dummy-users/{user_id}")
async def update_user(
    request: Request, user_id: str, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    principal = await resolve_session(request, database, settings)
    body = await read_json(request, UpdateUser)
    actor = authorize_mutation(request, principal, settings)
    result = await service.update_user(
        database, parse_user(user_id), actor, request.state.request_id, _key(request), body
    )
    return success(request, result)


@router.post("/api/dev/dummy-users/{user_id}/archive")
async def archive_user(
    request: Request, user_id: str, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    return await _set_archived(request, user_id, settings, database, archived=True)


@router.post("/api/dev/dummy-users/{user_id}/restore")
async def restore_user(
    request: Request, user_id: str, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    return await _set_archived(request, user_id, settings, database, archived=False)


async def _set_archived(
    request: Request, user_id: str, settings: SettingsDep, database: DatabaseDep, *, archived: bool
) -> JSONResponse:
    principal = await resolve_session(request, database, settings)
    body = await read_json(request, ExpectedRevision)
    actor = authorize_mutation(request, principal, settings)
    result = await service.set_archived(
        database,
        parse_user(user_id),
        actor,
        request.state.request_id,
        _key(request),
        body.expectedRevision,
        archived=archived,
    )
    return success(request, result)
