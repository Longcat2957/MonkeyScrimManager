import uuid

import httpx
import pytest
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncEngine

from tests.support.assertions import assert_failure
from tests.support.auth.oauth import ORIGIN
from tests.support.guilds.context import (
    GUILD_A,
    GUILD_B,
    ROLE_A,
    ROLE_B,
    USER_ID,
    authenticated_setup,
    guild_fixture,
)

pytestmark = pytest.mark.postgres


async def test_guild_context_roles_season_and_default_membership(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    material = await authenticated_setup(owner, runtime_client)
    await guild_fixture(owner, GUILD_B, "Beta", ROLE_B)
    async with owner.begin() as connection:
        await connection.execute(
            text("""
            INSERT INTO guild_member_discord_roles (guild_id, user_id, role_id, last_synced_at)
            VALUES (:guild, :user, :role, now())
        """),
            {"guild": GUILD_A, "user": USER_ID, "role": ROLE_A},
        )
    response = await runtime_client.get("/api/me/guilds")
    assert response.status_code == 200
    guilds = response.json()["data"]["guilds"]
    assert [item["id"] for item in guilds] == [GUILD_A, GUILD_B]
    assert [item["currentUserRole"] for item in guilds] == ["OPERATOR", "MEMBER"]
    assert [item["memberCount"] for item in guilds] == [1, 1]
    assert [item["displayName"] for item in guilds] == ["Alpha", "Beta"]
    context = await runtime_client.get(f"/api/guilds/{GUILD_A}")
    assert context.status_code == 200
    data = context.json()["data"]
    assert data["currentUserRole"] == "OPERATOR" and data["isExternallyPublic"] is True
    assert data["currentSeason"]["name"] == "현재 시즌"
    assert data["currentSeason"]["startedAt"] == "2026-09-13T01:02:03.12Z"
    uuid.UUID(data["currentSeason"]["id"])

    changed = await runtime_client.put(
        "/api/me/default-guild",
        json={"guildId": GUILD_B},
        headers={
            "origin": ORIGIN,
            "x-csrf-token": material["monkeybot_csrf"],
        },
    )
    assert changed.status_code == 200 and changed.json()["data"] == {"defaultGuildId": GUILD_B}
    assert (await runtime_client.get("/api/me")).json()["data"]["defaultGuildId"] == GUILD_B
    async with owner.begin() as connection:
        await connection.execute(
            text("UPDATE guild_members SET status = 'INACTIVE' WHERE guild_id = :guild"),
            {"guild": GUILD_B},
        )
    assert (await runtime_client.get("/api/me")).json()["data"]["defaultGuildId"] is None
    assert [
        item["id"] for item in (await runtime_client.get("/api/me/guilds")).json()["data"]["guilds"]
    ] == [GUILD_A]
    assert_failure(
        await runtime_client.get(f"/api/guilds/{GUILD_B}"),
        404,
        "not_found",
        "요청한 길드를 찾을 수 없습니다.",
    )
    denied = await runtime_client.put(
        "/api/me/default-guild",
        json={"guildId": GUILD_B},
        headers={
            "origin": ORIGIN,
            "x-csrf-token": material["monkeybot_csrf"],
        },
    )
    assert_failure(denied, 404, "not_found", "요청한 길드를 찾을 수 없습니다.")


@pytest.mark.parametrize("mode", ["disabled", "missing", "not_member"])
async def test_hidden_guilds_do_not_leak_even_to_service_admin(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, mode: str
) -> None:
    await authenticated_setup(owner, runtime_client)
    target = GUILD_A
    async with owner.begin() as connection:
        if mode == "disabled":
            await connection.execute(text("UPDATE guilds SET status = 'DISABLED'"))
        elif mode == "not_member":
            await connection.execute(text("DELETE FROM guild_members"))
        else:
            target = GUILD_B
    response = await runtime_client.get(f"/api/guilds/{target}")
    assert_failure(response, 404, "not_found", "요청한 길드를 찾을 수 없습니다.")


@pytest.mark.parametrize(
    "mode",
    [
        "missing",
        "wrong_origin",
        "wrong_token",
        "duplicate_origin",
        "duplicate_token",
        "duplicate_cookie",
    ],
)
async def test_csrf_denial_never_changes_default_or_revokes_session(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, mode: str
) -> None:
    material = await authenticated_setup(owner, runtime_client)
    headers = [("origin", ORIGIN), ("x-csrf-token", material["monkeybot_csrf"])]
    if mode == "missing":
        headers = []
    elif mode == "wrong_origin":
        headers[0] = ("origin", "https://untrusted.example")
    elif mode == "wrong_token":
        headers[1] = ("x-csrf-token", "wrong")
    elif mode == "duplicate_origin":
        headers.append(headers[0])
    elif mode == "duplicate_token":
        headers.append(headers[1])
    elif mode == "duplicate_cookie":
        headers.append(
            (
                "cookie",
                f"monkeybot_session={material['monkeybot_session']}; "
                f"monkeybot_csrf={material['monkeybot_csrf']}; "
                f"monkeybot_csrf={material['monkeybot_csrf']}",
            )
        )
    changed = await runtime_client.put(
        "/api/me/default-guild", json={"guildId": GUILD_A}, headers=headers
    )
    assert_failure(changed, 403, "csrf_validation_failed", "요청 출처를 확인할 수 없습니다.")
    logged_out = await runtime_client.post("/api/logout", headers=headers)
    assert_failure(logged_out, 403, "csrf_validation_failed", "요청 출처를 확인할 수 없습니다.")
    assert (await runtime_client.get("/api/me")).status_code == 200
    async with owner.connect() as connection:
        assert (
            await connection.scalar(text("SELECT default_guild_id FROM user_guild_preferences"))
            is None
        )
        assert await connection.scalar(text("SELECT revoked_at FROM user_sessions")) is None


@pytest.mark.parametrize(
    "body,status",
    [
        ('{"guildId": 123}', 422),
        ('{"guildId":"01"}', 422),
        ('{"guildId":"123","extra":true}', 422),
        ('{"guildId":"123","guildId":"124"}', 422),
        ('{"guildId":NaN}', 400),
        ("{", 400),
        ("{}", 422),
    ],
)
async def test_default_guild_json_validation_matches_contract(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, body: str, status: int
) -> None:
    await authenticated_setup(owner, runtime_client)
    response = await runtime_client.put(
        "/api/me/default-guild", content=body, headers={"content-type": "application/json"}
    )
    if status == 400:
        assert_failure(response, 400, "bad_request", "요청 형식이 올바르지 않습니다.")
    else:
        assert_failure(
            response,
            422,
            "unprocessable_entity",
            "요청 JSON의 필드 또는 값이 API 계약과 일치하지 않습니다.",
        )


async def test_content_type_and_path_validation_order(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    assert_failure(
        await runtime_client.get("/api/guilds/invalid"),
        400,
        "bad_request",
        "요청 형식이 올바르지 않습니다.",
    )
    assert_failure(
        await runtime_client.get(f"/api/guilds/{GUILD_A}"),
        401,
        "unauthorized",
        "인증이 필요합니다.",
    )
    await authenticated_setup(owner, runtime_client)
    response = await runtime_client.put("/api/me/default-guild", content='{"guildId":"123"}')
    assert_failure(response, 415, "unsupported_media_type", "지원하지 않는 요청 본문 형식입니다.")
