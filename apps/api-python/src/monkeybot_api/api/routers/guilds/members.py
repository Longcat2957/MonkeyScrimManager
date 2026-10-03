"""HTTP boundary for operator member administration."""

from fastapi import APIRouter, Request
from starlette.responses import JSONResponse

from monkeybot_api.api.body import read_json
from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.requests import command_key
from monkeybot_api.api.sessions import resolve_session
from monkeybot_api.api.validation import guild_path
from monkeybot_api.core.errors import ApiError, success
from monkeybot_api.core.legacy_json import parse_float, parse_int
from monkeybot_api.core.security import require_csrf
from monkeybot_api.schemas.common import user_id
from monkeybot_api.schemas.guilds.members import AdjustMmr, SetMemberStatus
from monkeybot_api.services.guilds import members as service

router = APIRouter()


def member_path(guild: str, member: str) -> None:
    guild_path(guild)
    try:
        user_id(member)
    except ValueError:
        raise ApiError.from_status(400) from None


@router.api_route("/api/guilds/{guild_id}/members", methods=["GET", "HEAD"])
async def members(
    request: Request, guild_id: str, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    guild_path(guild_id)
    principal = await resolve_session(request, database, settings)
    return success(
        request,
        await service.get_members(
            database, guild_id, principal["user_id"], request.state.request_id
        ),
    )


@router.patch("/api/guilds/{guild_id}/members/{member_id}/mmr")
async def adjust_mmr(
    request: Request, guild_id: str, member_id: str, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    member_path(guild_id, member_id)
    principal = await resolve_session(request, database, settings)
    body = await read_json(request, AdjustMmr, parse_float=parse_float, parse_int=parse_int)
    require_csrf(request, settings, principal["csrf_secret_hash"])
    try:
        key = command_key(request, False)
    except ApiError as exc:
        raise ApiError(
            exc.status, exc.code, "Idempotency-Key가 올바르지 않습니다.", exc.details
        ) from None
    return success(
        request,
        await service.adjust_mmr(
            database, guild_id, member_id, principal["user_id"], request.state.request_id, key, body
        ),
    )


@router.patch("/api/guilds/{guild_id}/members/{member_id}/status")
async def set_status(
    request: Request, guild_id: str, member_id: str, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    member_path(guild_id, member_id)
    principal = await resolve_session(request, database, settings)
    body = await read_json(request, SetMemberStatus, parse_float=parse_float, parse_int=parse_int)
    require_csrf(request, settings, principal["csrf_secret_hash"])
    return success(
        request,
        await service.set_status(
            database, guild_id, member_id, principal["user_id"], request.state.request_id, body
        ),
    )
