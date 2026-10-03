from fastapi import APIRouter, Request
from starlette.responses import Response

from monkeybot_api.api.body import read_json
from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.sessions import current_user, resolve_session
from monkeybot_api.core.errors import success
from monkeybot_api.core.security import require_csrf
from monkeybot_api.schemas.users import ChampionsInput, PrivacyInput
from monkeybot_api.services.users import profile as service

router = APIRouter()


@router.api_route("/api/me", methods=["GET", "HEAD"])
async def me(request: Request, settings: SettingsDep, database: DatabaseDep) -> Response:
    return success(request, await current_user(request, database, settings))


@router.api_route("/api/users/{discord_id}/profile", methods=["GET", "HEAD"])
async def public_profile(request: Request, discord_id: str, database: DatabaseDep) -> Response:
    return success(request, await service.public_profile(database, discord_id))


@router.api_route("/api/me/privacy", methods=["GET", "HEAD"])
async def privacy(request: Request, settings: SettingsDep, database: DatabaseDep) -> Response:
    principal = await resolve_session(request, database, settings)
    return success(request, await service.privacy(database, principal["user_id"]))


@router.put("/api/me/privacy")
async def update_privacy(
    request: Request, settings: SettingsDep, database: DatabaseDep
) -> Response:
    principal = await resolve_session(request, database, settings)
    command = await read_json(request, PrivacyInput)
    require_csrf(request, settings, principal["csrf_secret_hash"])
    return success(request, await service.update_privacy(database, principal["user_id"], command))


@router.api_route("/api/me/champion-preferences", methods=["GET", "HEAD"])
async def champions(request: Request, settings: SettingsDep, database: DatabaseDep) -> Response:
    principal = await resolve_session(request, database, settings)
    return success(request, await service.champions(database, principal["user_id"]))


@router.put("/api/me/champion-preferences/{lane}")
async def update_champions(
    request: Request, lane: str, settings: SettingsDep, database: DatabaseDep
) -> Response:
    principal = await resolve_session(request, database, settings)
    command = await read_json(request, ChampionsInput)
    require_csrf(request, settings, principal["csrf_secret_hash"])
    return success(
        request,
        await service.update_champions(database, principal["user_id"], lane, command.championIds),
    )
