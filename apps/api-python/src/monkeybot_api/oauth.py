import hmac
import ipaddress
import uuid

from fastapi import APIRouter, Request
from starlette.responses import Response

from monkeybot_api import auth_repository
from monkeybot_api.auth import (
    AuthCrypto,
    OAuthStartLimiter,
    cookie_value,
    encode_secret,
    hash_secret,
    new_secret,
    require_csrf,
    set_cookie,
    validate_next,
)
from monkeybot_api.config import Settings
from monkeybot_api.database import Database
from monkeybot_api.discord import DiscordClient
from monkeybot_api.errors import ApiError, success
from monkeybot_api.session import resolve_session


def query_value(request: Request, key: str, *, required: bool = False) -> str | None:
    values = request.query_params.getlist(key)
    if len(values) > 1 or (required and not values):
        raise ApiError.from_status(400)
    return values[0] if values else None


def router(settings: Settings, database: Database, discord: DiscordClient) -> APIRouter:
    routes = APIRouter()
    limiter = OAuthStartLimiter()
    crypto = (
        AuthCrypto(settings.oauth_encryption_key.get_secret_value())
        if settings.oauth_encryption_key
        else None
    )

    def require_crypto() -> AuthCrypto:
        if crypto is None or database.engine is None:
            raise ApiError(
                503, "authentication_unavailable", "인증 서비스를 일시적으로 사용할 수 없습니다."
            )
        return crypto

    @routes.api_route("/oauth/discord/start", methods=["GET", "HEAD"])
    async def start(request: Request) -> Response:
        next_path = query_value(request, "next")
        auth = require_crypto()
        peer = "unknown"
        for name in ("x-real-ip", "cf-connecting-ip"):
            value = request.headers.get(name)
            if value is not None:
                try:
                    if "%" in value:
                        raise ValueError("Rust IpAddr does not accept zone identifiers")
                    address = ipaddress.ip_address(value.strip())
                    peer = str(address)
                    if isinstance(address, ipaddress.IPv6Address) and address.ipv4_mapped:
                        peer = f"::ffff:{address.ipv4_mapped}"
                    break
                except ValueError:
                    pass
        if not limiter.check(peer):
            raise ApiError(
                429, "rate_limited", "로그인 요청이 너무 많습니다. 잠시 후 다시 시도해 주세요."
            )
        redirect = validate_next(next_path)
        attempt_id, state, verifier = uuid.uuid7(), new_secret(), new_secret()
        await auth_repository.create_attempt(
            database, attempt_id, state, auth.encrypt_pkce(attempt_id, verifier), redirect
        )
        response = Response(
            status_code=302,
            headers={
                "location": discord.authorization_url(state, encode_secret(hash_secret(verifier)))
            },
        )
        set_cookie(response, settings, "monkeybot_oauth_state", state)
        return response

    @routes.api_route("/oauth/callback", methods=["GET", "HEAD"])
    async def callback(request: Request) -> Response:
        state = query_value(request, "state", required=True)
        code = query_value(request, "code")
        denied = query_value(request, "error")
        auth = require_crypto()
        cookie = cookie_value(request, "monkeybot_oauth_state")
        invalid = ApiError(
            400, "invalid_oauth_state", "로그인 요청이 만료되었거나 올바르지 않습니다."
        )
        if (
            state is None
            or cookie is None
            or len(state.encode()) != 43
            or len(cookie.encode()) != 43
            or not hmac.compare_digest(state.encode(), cookie.encode())
        ):
            raise invalid
        attempt = await auth_repository.consume_attempt(database, state)
        if attempt is None:
            raise invalid
        if not code or denied is not None:
            raise ApiError(
                400,
                "oauth_authorization_denied",
                "Discord 로그인이 취소되었거나 승인되지 않았습니다.",
            )
        try:
            verifier = auth.decrypt_pkce(attempt["id"], attempt["pkce_verifier_ciphertext"])
        except ValueError:
            raise ApiError.from_status(500) from None
        user = await discord.authenticate(code, verifier, request.state.request_id)
        token, csrf = new_secret(), new_secret()
        await auth_repository.complete_login(
            database,
            user,
            cookie_value(request, "monkeybot_session"),
            token,
            csrf,
            settings.session_ttl_seconds,
        )
        response = Response(status_code=303, headers={"location": attempt["redirect_path"]})
        set_cookie(response, settings, "monkeybot_session", token)
        set_cookie(response, settings, "monkeybot_csrf", csrf)
        set_cookie(response, settings, "monkeybot_oauth_state", "", clear=True)
        return response

    @routes.post("/api/logout")
    async def logout(request: Request) -> Response:
        principal = await resolve_session(request, database, settings)
        require_csrf(request, settings, principal["csrf_secret_hash"])
        await auth_repository.revoke_session(database, principal["token_hash"])
        response = success(request, {"loggedOut": True})
        for name in ("monkeybot_session", "monkeybot_csrf"):
            set_cookie(response, settings, name, "", clear=True)
        return response

    return routes
