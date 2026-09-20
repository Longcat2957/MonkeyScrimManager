import base64
import json
import os
import subprocess
import uuid

import pytest

from monkeybot_api.auth import AuthCrypto, encode_secret, hash_secret, new_secret, validate_next
from monkeybot_api.errors import ApiError

ZERO_KEY = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"


def rust_auth(payload: dict) -> dict:
    executable = os.environ.get("FASTAPI_AUTH_COMPAT_EXE")
    if not executable:
        pytest.skip("scripts/test-fastapi.sh builds the Rust auth interop oracle")
    result = subprocess.run(
        [executable],
        input=json.dumps(payload),
        text=True,
        capture_output=True,
        check=True,
        timeout=10,
    )
    return json.loads(result.stdout)


def decode(value: str) -> bytes:
    return base64.urlsafe_b64decode(value + "=" * (-len(value) % 4))


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
