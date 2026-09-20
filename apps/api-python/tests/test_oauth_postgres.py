import asyncio
import base64
import hashlib
import uuid
from http.cookies import SimpleCookie
from urllib.parse import parse_qs, urlsplit

import httpx
import pytest
from conftest import DiscordStub
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncEngine
from test_auth_crypto import decode, rust_auth
from test_foundation import assert_failure

pytestmark = pytest.mark.postgres
USER_ID = "328516947210438656"
ORIGIN = "http://localhost:5173"


def cookies(response: httpx.Response) -> dict[str, str]:
    result = {}
    for header in response.headers.get_list("set-cookie"):
        parsed = SimpleCookie()
        parsed.load(header)
        result.update({name: value.value for name, value in parsed.items()})
    return result


async def start_login(client: httpx.AsyncClient, next_path: str = "/me") -> tuple[str, dict]:
    response = await client.get("/oauth/discord/start", params={"next": next_path})
    assert response.status_code == 302
    query = parse_qs(urlsplit(response.headers["location"]).query)
    assert query["scope"] == ["identify"]
    assert query["redirect_uri"] == [ORIGIN + "/oauth/callback"]
    assert query["code_challenge_method"] == ["S256"]
    state = query["state"][0]
    assert cookies(response)["monkeybot_oauth_state"] == state
    cookie = response.headers["set-cookie"]
    assert "Path=/oauth/callback" in cookie and "HttpOnly" in cookie and "Max-Age=300" in cookie
    return state, query


async def finish_login(client: httpx.AsyncClient, state: str, previous: str | None = None):
    header = f"monkeybot_oauth_state={state}"
    if previous is not None:
        header += f"; monkeybot_session={previous}"
    return await client.get(
        "/oauth/callback", params={"state": state, "code": "test-code"}, headers={"cookie": header}
    )


async def test_login_rotation_and_csrf_logout(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, discord_stub: DiscordStub
) -> None:
    state, query = await start_login(runtime_client)
    async with owner.connect() as connection:
        attempt = (
            (await connection.execute(text("SELECT * FROM oauth_login_attempts"))).mappings().one()
        )
    verifier = rust_auth(
        {
            "operation": "decrypt",
            "attempt_id": str(attempt["id"]),
            "ciphertext": base64.urlsafe_b64encode(attempt["pkce_verifier_ciphertext"])
            .decode()
            .rstrip("="),
        }
    )["verifier"]
    assert query["code_challenge"] == [
        base64.urlsafe_b64encode(hashlib.sha256(verifier.encode()).digest()).decode().rstrip("=")
    ]
    assert attempt["state_hash"] == hashlib.sha256(state.encode()).digest()
    response = await finish_login(runtime_client, state)
    assert response.status_code == 303
    assert response.headers["location"] == "/me"
    material = cookies(response)
    assert material["monkeybot_session"] != material["monkeybot_csrf"]
    assert material["monkeybot_oauth_state"] == ""
    assert len(discord_stub.requests) == 2
    token_request = discord_stub.requests[0]
    form = parse_qs(token_request.content.decode())
    assert form["code_verifier"] == [verifier]
    expected_basic = base64.b64encode(b"123456789012345678:integration-test-only").decode()
    assert token_request.headers["authorization"] == "Basic " + expected_basic
    assert "client_secret" not in form
    first_token = material["monkeybot_session"]
    first_csrf = material["monkeybot_csrf"]
    async with owner.connect() as connection:
        session = (await connection.execute(text("SELECT * FROM user_sessions"))).mappings().one()
        privacy = (
            (await connection.execute(text("SELECT * FROM user_privacy_settings"))).mappings().one()
        )
    assert session["token_hash"] == hashlib.sha256(first_token.encode()).digest()
    assert session["csrf_secret_hash"] == hashlib.sha256(first_csrf.encode()).digest()
    assert (
        privacy["public_profile_enabled"],
        privacy["show_riot_ids"],
        privacy["allow_external_record_search"],
    ) == (True, True, False)
    assert (await runtime_client.get("/api/me")).json()["data"]["displayName"] == "OAuth 사용자"

    state2, _ = await start_login(runtime_client)
    rotated = await finish_login(runtime_client, state2, first_token)
    assert rotated.status_code == 303
    material2 = cookies(rotated)
    assert material2["monkeybot_session"] != first_token
    async with owner.connect() as connection:
        rows = (
            (await connection.execute(text("SELECT * FROM user_sessions ORDER BY created_at")))
            .mappings()
            .all()
        )
    assert rows[0]["revoked_at"] is not None
    assert rows[1]["rotated_from_id"] == rows[0]["id"]
    assert_failure(
        await runtime_client.get("/api/me", headers={"cookie": f"monkeybot_session={first_token}"}),
        401,
        "unauthorized",
        "인증이 필요합니다.",
    )
    denied = await runtime_client.post("/api/logout")
    assert_failure(denied, 403, "csrf_validation_failed", "요청 출처를 확인할 수 없습니다.")
    assert (await runtime_client.get("/api/me")).status_code == 200
    logout = await runtime_client.post(
        "/api/logout",
        headers={
            "origin": ORIGIN,
            "x-csrf-token": material2["monkeybot_csrf"],
        },
    )
    assert logout.status_code == 200 and logout.json()["data"] == {"loggedOut": True}
    assert cookies(logout) == {"monkeybot_session": "", "monkeybot_csrf": ""}
    assert all("Max-Age=0" in item for item in logout.headers.get_list("set-cookie"))
    assert_failure(await runtime_client.get("/api/me"), 401, "unauthorized", "인증이 필요합니다.")


async def test_rust_started_oauth_completes_in_python(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, discord_stub: DiscordStub
) -> None:
    material = rust_auth({"operation": "material"})
    async with owner.begin() as connection:
        await connection.execute(
            text("""
            INSERT INTO oauth_login_attempts
                (id, state_hash, pkce_verifier_ciphertext, redirect_path, created_at, expires_at)
            VALUES (:id, :hash, :ciphertext, '/records', now(), now() + interval '5 minutes')
        """),
            {
                "id": uuid.UUID(material["attemptId"]),
                "hash": decode(material["stateHash"]),
                "ciphertext": decode(material["ciphertext"]),
            },
        )
    response = await finish_login(runtime_client, material["state"])
    assert response.status_code == 303 and response.headers["location"] == "/records"
    assert parse_qs(discord_stub.requests[0].content.decode())["code_verifier"] == [
        material["verifier"]
    ]


@pytest.mark.parametrize(
    "mode", ["mismatch", "duplicate", "missing", "expired", "tampered", "denied"]
)
async def test_invalid_callback_does_not_issue_session(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, discord_stub: DiscordStub, mode: str
) -> None:
    state, _ = await start_login(runtime_client)
    params = {"state": state, "code": "test-code"}
    cookie = f"monkeybot_oauth_state={state}"
    if mode == "mismatch":
        cookie = "monkeybot_oauth_state=" + "x" * 43
    elif mode == "duplicate":
        cookie += "; " + cookie
    elif mode == "missing":
        cookie = "unrelated=x"
    elif mode == "expired":
        async with owner.begin() as connection:
            await connection.execute(
                text("UPDATE oauth_login_attempts SET expires_at = now() - interval '1 second'")
            )
    elif mode == "tampered":
        async with owner.begin() as connection:
            await connection.execute(
                text(
                    "UPDATE oauth_login_attempts "
                    "SET pkce_verifier_ciphertext = decode('0102', 'hex')"
                )
            )
    elif mode == "denied":
        params["error"] = "access_denied"
    response = await runtime_client.get(
        "/oauth/callback", params=params, headers={"cookie": cookie}
    )
    if mode == "denied":
        assert_failure(
            response,
            400,
            "oauth_authorization_denied",
            "Discord 로그인이 취소되었거나 승인되지 않았습니다.",
        )
    elif mode == "tampered":
        assert_failure(
            response, 500, "internal_error", "요청을 처리하는 중 내부 오류가 발생했습니다."
        )
    else:
        assert_failure(
            response, 400, "invalid_oauth_state", "로그인 요청이 만료되었거나 올바르지 않습니다."
        )
    assert "set-cookie" not in response.headers
    assert not discord_stub.requests
    async with owner.connect() as connection:
        assert await connection.scalar(text("SELECT count(*) FROM user_sessions")) == 0


async def test_callback_is_consumed_once_under_concurrency(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, discord_stub: DiscordStub
) -> None:
    state, _ = await start_login(runtime_client)
    responses = await asyncio.gather(
        finish_login(runtime_client, state), finish_login(runtime_client, state)
    )
    assert sorted(response.status_code for response in responses) == [303, 400]
    failure = next(response for response in responses if response.status_code == 400)
    assert_failure(
        failure, 400, "invalid_oauth_state", "로그인 요청이 만료되었거나 올바르지 않습니다."
    )
    assert len(discord_stub.requests) == 2
    async with owner.connect() as connection:
        assert await connection.scalar(text("SELECT count(*) FROM user_sessions")) == 1


@pytest.mark.parametrize(
    "stage,status", [("token", 400), ("token", 429), ("token", 500), ("user", 401), ("user", 503)]
)
async def test_upstream_failure_is_safe_and_attempt_cannot_replay(
    owner: AsyncEngine,
    runtime_client: httpx.AsyncClient,
    discord_stub: DiscordStub,
    stage: str,
    status: int,
) -> None:
    setattr(discord_stub, stage + "_status", status)
    state, _ = await start_login(runtime_client)
    response = await finish_login(runtime_client, state)
    if stage == "token" and status == 400:
        assert_failure(
            response,
            400,
            "oauth_exchange_failed",
            "Discord 로그인을 완료할 수 없습니다. 다시 시도해 주세요.",
        )
    else:
        assert_failure(
            response,
            503,
            "oauth_provider_unavailable",
            "Discord 인증 서비스를 일시적으로 사용할 수 없습니다.",
        )
    assert "test-provider-token" not in response.text
    assert "set-cookie" not in response.headers
    assert_failure(
        await finish_login(runtime_client, state),
        400,
        "invalid_oauth_state",
        "로그인 요청이 만료되었거나 올바르지 않습니다.",
    )
    async with owner.connect() as connection:
        assert await connection.scalar(text("SELECT count(*) FROM user_sessions")) == 0


async def test_login_identity_conflict_rolls_back_all_login_writes(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    existing_owner = "328516947210438657"
    async with owner.begin() as connection:
        await connection.execute(
            text("""
            INSERT INTO users (user_id, identity_kind, display_name, created_at, updated_at)
            VALUES (:owner, 'DISCORD', '기존 사용자', now(), now())
        """),
            {"owner": existing_owner},
        )
        await connection.execute(
            text("""
            INSERT INTO user_discord_identities (user_id, discord_id, created_at, updated_at)
            VALUES (:owner, :discord, now(), now())
        """),
            {"owner": existing_owner, "discord": USER_ID},
        )
    state, _ = await start_login(runtime_client)
    response = await finish_login(runtime_client, state)
    assert_failure(response, 500, "internal_error", "요청을 처리하는 중 내부 오류가 발생했습니다.")
    assert "set-cookie" not in response.headers
    async with owner.connect() as connection:
        assert await connection.scalar(text("SELECT count(*) FROM users")) == 1
        assert await connection.scalar(text("SELECT count(*) FROM user_sessions")) == 0
        assert await connection.scalar(text("SELECT count(*) FROM user_privacy_settings")) == 0


async def test_oauth_start_limit_preserves_axum_forwarding_header_key(
    runtime_client: httpx.AsyncClient,
) -> None:
    for _ in range(30):
        response = await runtime_client.get(
            "/oauth/discord/start", headers={"x-real-ip": "192.0.2.1"}
        )
        assert response.status_code == 302
    response = await runtime_client.get(
        "/oauth/discord/start",
        headers={"x-real-ip": "192.0.2.1", "cf-connecting-ip": "198.51.100.1"},
    )
    assert_failure(
        response, 429, "rate_limited", "로그인 요청이 너무 많습니다. 잠시 후 다시 시도해 주세요."
    )

    fallback = await runtime_client.get(
        "/oauth/discord/start", headers={"x-real-ip": "invalid", "cf-connecting-ip": "198.51.100.1"}
    )
    assert fallback.status_code == 302
