import ipaddress
from typing import cast

from fastapi import APIRouter, Request
from starlette.responses import Response

from monkeybot_api.api.dependencies import DatabaseDep, DiscordDep, SettingsDep
from monkeybot_api.api.sessions import resolve_session
from monkeybot_api.core.errors import ApiError, success
from monkeybot_api.core.security import (
    AuthCrypto,
    OAuthStartLimiter,
    cookie_value,
    require_csrf,
    set_cookie,
)
from monkeybot_api.db.database import Database
from monkeybot_api.services.auth import oauth as service

router = APIRouter()


def query_value(request: Request, key: str, *, required: bool = False) -> str | None:
    values = request.query_params.getlist(key)
    if len(values) > 1 or (required and not values):
        raise ApiError.from_status(400)
    return values[0] if values else None


def require_crypto(request: Request, database: Database) -> AuthCrypto:
    crypto = cast(AuthCrypto | None, request.app.state.oauth_crypto)
    if crypto is None or database.engine is None:
        raise ApiError(
            503, "authentication_unavailable", "인증 서비스를 일시적으로 사용할 수 없습니다."
        )
    return crypto


def login_peer(request: Request) -> str:
    for name in ("x-real-ip", "cf-connecting-ip"):
        value = request.headers.get(name)
        if value is not None:
            try:
                if "%" in value:
                    raise ValueError("Rust IpAddr does not accept zone identifiers")
                address = ipaddress.ip_address(value.strip())
                if isinstance(address, ipaddress.IPv6Address) and address.ipv4_mapped:
                    return f"::ffff:{address.ipv4_mapped}"
                return str(address)
            except ValueError:
                pass
    return "unknown"


@router.api_route("/oauth/discord/start", methods=["GET", "HEAD"])
async def start(
    request: Request, settings: SettingsDep, database: DatabaseDep, discord: DiscordDep
) -> Response:
    next_path = query_value(request, "next")
    crypto = require_crypto(request, database)
    state, location = await service.start_login(
        database,
        discord,
        crypto,
        cast(OAuthStartLimiter, request.app.state.oauth_start_limiter),
        login_peer(request),
        next_path,
    )
    response = Response(status_code=302, headers={"location": location})
    set_cookie(response, settings, "monkeybot_oauth_state", state)
    return response


@router.api_route("/oauth/callback", methods=["GET", "HEAD"])
async def callback(
    request: Request, settings: SettingsDep, database: DatabaseDep, discord: DiscordDep
) -> Response:
    state = query_value(request, "state", required=True)
    code = query_value(request, "code")
    denied = query_value(request, "error")
    crypto = require_crypto(request, database)
    location, token, csrf = await service.complete_login(
        database,
        discord,
        crypto,
        settings.session_ttl_seconds,
        state,
        cookie_value(request, "monkeybot_oauth_state"),
        code,
        denied,
        cookie_value(request, "monkeybot_session"),
        request.state.request_id,
    )
    response = Response(status_code=303, headers={"location": location})
    set_cookie(response, settings, "monkeybot_session", token)
    set_cookie(response, settings, "monkeybot_csrf", csrf)
    set_cookie(response, settings, "monkeybot_oauth_state", "", clear=True)
    return response


@router.post("/api/logout")
async def logout(request: Request, settings: SettingsDep, database: DatabaseDep) -> Response:
    principal = await resolve_session(request, database, settings)
    require_csrf(request, settings, principal["csrf_secret_hash"])
    await service.logout(database, principal)
    response = success(request, {"loggedOut": True})
    for name in ("monkeybot_session", "monkeybot_csrf"):
        set_cookie(response, settings, name, "", clear=True)
    return response
