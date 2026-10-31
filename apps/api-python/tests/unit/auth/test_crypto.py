import uuid

import pytest

from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.security import (
    AuthCrypto,
    encode_secret,
    hash_secret,
    new_secret,
    validate_next,
)
from tests.support.auth.crypto import ZERO_KEY, decode, rust_auth


def test_rust_python_pkce_and_session_hashes_interoperate() -> None:
    rust = rust_auth({"operation": "material"})
    crypto = AuthCrypto(ZERO_KEY)
    attempt_id = uuid.UUID(rust["attemptId"])
    assert crypto.decrypt_pkce(attempt_id, decode(rust["ciphertext"])) == rust["verifier"]
    assert encode_secret(hash_secret(rust["verifier"])) == rust["challenge"]
    for source, hashed in [("state", "stateHash"), ("token", "tokenHash"), ("csrf", "csrfHash")]:
        assert encode_secret(hash_secret(rust[source])) == rust[hashed]
    verifier = new_secret()
    envelope = crypto.encrypt_pkce(attempt_id, verifier)
    assert rust_auth(
        {
            "operation": "decrypt",
            "attempt_id": str(attempt_id),
            "ciphertext": encode_secret(envelope),
        }
    ) == {"verifier": verifier}
    assert rust_auth({"operation": "hash", "value": verifier}) == {
        "hash": encode_secret(hash_secret(verifier))
    }


def test_pkce_rejects_tampering_wrong_attempt_and_version() -> None:
    crypto = AuthCrypto(ZERO_KEY)
    attempt = uuid.uuid7()
    encrypted = crypto.encrypt_pkce(attempt, new_secret())
    for identity, ciphertext in [
        (uuid.uuid7(), encrypted),
        (attempt, encrypted[:-1]),
        (attempt, encrypted[:-1] + bytes([encrypted[-1] ^ 1])),
        (attempt, b"\x02" + encrypted[1:]),
        (attempt, b""),
    ]:
        with pytest.raises(ValueError):
            crypto.decrypt_pkce(identity, ciphertext)


@pytest.mark.parametrize(
    "value", [None, "/", "/me?tab=privacy", "/g/123/series", "/users/123", "/admin/status"]
)
def test_redirect_allowlist(value: str | None) -> None:
    assert validate_next(value) == ("/" if value is None else value)


@pytest.mark.parametrize(
    "value",
    [
        "",
        "https://evil.example",
        "//evil.example",
        "/\\evil.example",
        "/api/me",
        "/ws",
        "/oauth/callback",
        "/g/",
        "/users/",
        "/unknown",
        "/me\r\nLocation: evil",
        "/g/" + "a" * 2048,
    ],
)
def test_invalid_redirects_have_safe_reason(value: str) -> None:
    with pytest.raises(ApiError) as failure:
        validate_next(value)
    assert failure.value.code == "invalid_oauth_redirect"
    assert failure.value.message == "로그인 후 이동할 경로가 올바르지 않습니다."
