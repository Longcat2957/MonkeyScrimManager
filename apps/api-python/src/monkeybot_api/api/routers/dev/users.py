"""Service-admin development dummy-user list, detail, catalog, and creation routes."""

from fastapi import APIRouter, Request
from starlette.responses import JSONResponse

from monkeybot_api.api.body import read_json
from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.dev_common import authorize, authorize_mutation, command_key
from monkeybot_api.api.sessions import resolve_session
from monkeybot_api.core.errors import ApiError, success
from monkeybot_api.schemas.dev.users import CreateUser, CreateUsersBulk
from monkeybot_api.services.dev import users as service

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


@router.api_route("/api/dev/dummy-users", methods=["GET", "HEAD"])
async def list_users(
    request: Request, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    principal = await resolve_session(request, database, settings)
    filters = service.parse_filters(request.query_params)
    authorize(principal, settings)
    return success(request, await service.list_users(database, filters))


@router.api_route("/api/dev/dummy-users/catalog", methods=["GET", "HEAD"])
async def catalog(request: Request, settings: SettingsDep, database: DatabaseDep) -> JSONResponse:
    principal = await resolve_session(request, database, settings)
    authorize(principal, settings)
    return success(request, await service.catalog(database))


@router.post("/api/dev/dummy-users/bulk")
async def create_bulk(
    request: Request, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    principal = await resolve_session(request, database, settings)
    body = await read_json(request, CreateUsersBulk)
    actor = authorize_mutation(request, principal, settings)
    data = await service.create(
        database, actor, request.state.request_id, _key(request), body.users, bulk=True
    )
    response = success(request, data)
    response.status_code = 201
    return response


@router.api_route("/api/dev/dummy-users/{user_id}", methods=["GET", "HEAD"])
async def get_user(
    request: Request, user_id: str, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    principal = await resolve_session(request, database, settings)
    authorize(principal, settings)
    user = service.parse_user(user_id)
    return success(request, await service.get_user(database, user))


@router.post("/api/dev/dummy-users")
async def create_user(
    request: Request, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    principal = await resolve_session(request, database, settings)
    body = await read_json(request, CreateUser)
    actor = authorize_mutation(request, principal, settings)
    data = await service.create(
        database, actor, request.state.request_id, _key(request), [body], bulk=False
    )
    response = success(request, data)
    response.status_code = 201
    return response
