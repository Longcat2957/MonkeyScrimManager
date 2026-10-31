import hmac
import uuid
from typing import Any

from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.security import (
    AuthCrypto,
    OAuthStartLimiter,
    encode_secret,
    hash_secret,
    new_secret,
    validate_next,
)
from monkeybot_api.db.database import Database
from monkeybot_api.integrations.discord import DiscordClient
from monkeybot_api.repositories import auth as repository


async def start_login(
    database: Database,
    discord: DiscordClient,
    crypto: AuthCrypto,
    limiter: OAuthStartLimiter,
    peer: str,
    next_path: str | None,
) -> tuple[str, str]:
    if not limiter.check(peer):
        raise ApiError(
            429, "rate_limited", "로그인 요청이 너무 많습니다. 잠시 후 다시 시도해 주세요."
        )
    redirect = validate_next(next_path)
    attempt_id, state, verifier = uuid.uuid7(), new_secret(), new_secret()
    await repository.create_attempt(
        database, attempt_id, state, crypto.encrypt_pkce(attempt_id, verifier), redirect
    )
    return state, discord.authorization_url(state, encode_secret(hash_secret(verifier)))


async def complete_login(
    database: Database,
    discord: DiscordClient,
    crypto: AuthCrypto,
    ttl: int,
    state: str | None,
    cookie: str | None,
    code: str | None,
    denied: str | None,
    previous_token: str | None,
    request_id: str,
) -> tuple[str, str, str]:
    invalid = ApiError(400, "invalid_oauth_state", "로그인 요청이 만료되었거나 올바르지 않습니다.")
    if (
        state is None
        or cookie is None
        or len(state.encode()) != 43
        or len(cookie.encode()) != 43
        or not hmac.compare_digest(state.encode(), cookie.encode())
    ):
        raise invalid
    attempt = await repository.consume_attempt(database, state)
    if attempt is None:
        raise invalid
    if not code or denied is not None:
        raise ApiError(
            400,
            "oauth_authorization_denied",
            "Discord 로그인이 취소되었거나 승인되지 않았습니다.",
        )
    try:
        verifier = crypto.decrypt_pkce(attempt["id"], attempt["pkce_verifier_ciphertext"])
    except ValueError:
        raise ApiError.from_status(500) from None
    user = await discord.authenticate(code, verifier, request_id)
    token, csrf = new_secret(), new_secret()
    await repository.complete_login(database, user, previous_token, token, csrf, ttl)
    redirect: str = attempt["redirect_path"]
    return redirect, token, csrf


async def logout(database: Database, principal: dict[str, Any]) -> None:
    await repository.revoke_session(database, principal["token_hash"])
