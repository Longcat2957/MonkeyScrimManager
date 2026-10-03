import base64
import hashlib
import hmac
import re
import secrets
import time
import uuid
from urllib.parse import urlsplit

from cryptography.exceptions import InvalidTag
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from fastapi import Request
from starlette.responses import Response

from monkeybot_api.core.config import Settings
from monkeybot_api.core.errors import ApiError


def encode_secret(value: bytes) -> str:
    return base64.urlsafe_b64encode(value).decode("ascii").rstrip("=")


def hash_secret(value: str) -> bytes:
    return hashlib.sha256(value.encode()).digest()


def new_secret() -> str:
    return encode_secret(secrets.token_bytes(32))


class AuthCrypto:
    def __init__(self, key: str) -> None:
        self.cipher = AESGCM(base64.urlsafe_b64decode(key + "="))

    def encrypt_pkce(self, attempt_id: uuid.UUID, verifier: str) -> bytes:
        nonce = secrets.token_bytes(12)
        # Rust ring envelope: version 1 | nonce | ciphertext | 16-byte tag; UUID bytes are AAD.
        return b"\x01" + nonce + self.cipher.encrypt(nonce, verifier.encode(), attempt_id.bytes)

    def decrypt_pkce(self, attempt_id: uuid.UUID, envelope: bytes) -> str:
        if len(envelope) < 29 or envelope[0] != 1:
            raise ValueError("invalid OAuth verifier envelope")
        try:
            verifier = self.cipher.decrypt(envelope[1:13], envelope[13:], attempt_id.bytes).decode()
        except InvalidTag, UnicodeDecodeError:
            raise ValueError("invalid OAuth verifier ciphertext") from None
        if not re.fullmatch(r"[A-Za-z0-9._~-]{43,128}", verifier):
            raise ValueError("invalid PKCE verifier")
        return verifier


def cookie_value(request: Request, name: str) -> str | None:
    matches = []
    for header in request.headers.getlist("cookie"):
        for pair in header.split(";"):
            key, separator, value = pair.strip().partition("=")
            if separator and key.strip() == name:
                matches.append(value.strip())
    return matches[0] if len(matches) == 1 else None


def set_cookie(
    response: Response, settings: Settings, name: str, value: str, *, clear: bool = False
) -> None:
    oauth = name == "monkeybot_oauth_state"
    response.set_cookie(
        key=name,
        value=value,
        max_age=0 if clear else (300 if oauth else settings.session_ttl_seconds),
        path="/oauth/callback" if oauth else "/",
        secure=settings.env in {"development", "production"},
        httponly=name != "monkeybot_csrf",
        samesite="lax",
    )


def require_csrf(request: Request, settings: Settings, csrf_secret_hash: bytes) -> None:
    origins = request.headers.getlist("origin")
    tokens = request.headers.getlist("x-csrf-token")
    cookie = cookie_value(request, "monkeybot_csrf")
    if (
        origins != [settings.public_origin]
        or len(tokens) != 1
        or cookie is None
        or not hmac.compare_digest(tokens[0].encode(), cookie.encode())
        or not hmac.compare_digest(hash_secret(cookie), csrf_secret_hash)
    ):
        raise ApiError(403, "csrf_validation_failed", "요청 출처를 확인할 수 없습니다.")


def validate_next(value: str | None) -> str:
    if value is None:
        return "/"
    invalid = ApiError(400, "invalid_oauth_redirect", "로그인 후 이동할 경로가 올바르지 않습니다.")
    # http::Uri rejects literal whitespace/non-ASCII. Redirect headers must remain safe ASCII.
    if (
        not value
        or len(value.encode()) > 2048
        or not value.isascii()
        or value.startswith("//")
        or "\\" in value
        or any(ord(char) <= 32 or ord(char) == 127 for char in value)
    ):
        raise invalid
    parsed = urlsplit(value)
    if parsed.scheme or parsed.netloc or not parsed.path.startswith("/"):
        raise invalid
    if parsed.path not in {"/", "/admin/status", "/docs", "/me", "/records"} and not any(
        parsed.path.startswith(prefix) and len(parsed.path) > len(prefix)
        for prefix in ("/g/", "/users/")
    ):
        raise invalid
    return value


class OAuthStartLimiter:
    """Process-local 30-request burst, replenishing one token per two seconds."""

    def __init__(self) -> None:
        self.buckets: dict[str, tuple[float, float]] = {}
        self.checks = 0

    def check(self, peer: str) -> bool:
        now = time.monotonic()
        self.checks += 1
        if self.checks % 256 == 0:
            self.buckets = {
                key: value for key, value in self.buckets.items() if now - value[1] < 60
            }
        tokens, updated = self.buckets.get(peer, (30.0, now))
        tokens = min(30.0, tokens + (now - updated) / 2)
        allowed = tokens >= 1
        self.buckets[peer] = (tokens - 1 if allowed else tokens, now)
        return allowed
