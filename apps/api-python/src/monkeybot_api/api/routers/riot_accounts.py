from fastapi import APIRouter, Request
from starlette.responses import JSONResponse

from monkeybot_api.api.body import read_json
from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.sessions import resolve_session
from monkeybot_api.core.errors import success
from monkeybot_api.core.security import require_csrf
from monkeybot_api.schemas.riot_accounts import RiotIdentity
from monkeybot_api.services import riot_accounts as service

router = APIRouter()


@router.api_route("/api/me/riot-accounts", methods=["GET", "HEAD"])
async def list_accounts(
    request: Request, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    principal = await resolve_session(request, database, settings)
    return success(request, await service.list_accounts(database, principal["user_id"]))


@router.post("/api/me/riot-accounts")
async def create_account(
    request: Request, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    principal = await resolve_session(request, database, settings)
    body = await read_json(request, RiotIdentity)
    require_csrf(request, settings, principal["csrf_secret_hash"])
    result = await service.create(database, settings, principal["user_id"], body)
    response = success(request, result)
    response.status_code = 201
    return response


@router.put("/api/me/riot-accounts/{puuid}/main")
async def set_main_account(
    request: Request, puuid: str, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    principal = await resolve_session(request, database, settings)
    require_csrf(request, settings, principal["csrf_secret_hash"])
    service.validate_puuid(puuid)
    return success(request, await service.set_main(database, principal["user_id"], puuid))


@router.post("/api/me/riot-accounts/{puuid}/refresh")
async def refresh_account(
    request: Request, puuid: str, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    principal = await resolve_session(request, database, settings)
    require_csrf(request, settings, principal["csrf_secret_hash"])
    service.validate_puuid(puuid)
    return success(request, await service.refresh(database, settings, principal["user_id"], puuid))


@router.delete("/api/me/riot-accounts/{puuid}")
async def delete_account(
    request: Request, puuid: str, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    principal = await resolve_session(request, database, settings)
    require_csrf(request, settings, principal["csrf_secret_hash"])
    service.validate_puuid(puuid)
    return success(request, await service.delete(database, principal["user_id"], puuid))
