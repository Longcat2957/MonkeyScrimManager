"""Persistent dummy-user creation, query, and security boundaries."""

import uuid

import httpx
import pytest
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncEngine

from tests.support.assertions import assert_failure
from tests.support.auth.session import TOKEN, USER_ID, seed_session
from tests.support.guilds.context import GUILD_A, GUILD_B, ROLE_A, guild_fixture

pytestmark = pytest.mark.postgres

SESSION = {"cookie": f"monkeybot_session={TOKEN}"}
CSRF = {
    "cookie": f"monkeybot_session={TOKEN}; monkeybot_csrf=integration-csrf",
    "origin": "http://localhost:5173",
    "x-csrf-token": "integration-csrf",
}
USERS = "/api/dev/dummy-users"
LANE_MMR = dict.fromkeys(("top", "jungle", "mid", "bottom", "support"), 1500.0)


def body(stable: str, *, guild: str | None = None) -> dict:
    return {
        "stableKey": stable,
        "displayName": "테스트 더미",
        "privacy": {
            "publicProfileEnabled": False,
            "showRiotIds": True,
            "allowExternalRecordSearch": False,
        },
        "guildMemberships": []
        if guild is None
        else [
            {
                "guildId": guild,
                "displayName": "길드 더미",
                "status": "ACTIVE",
                "mainLane": "TOP",
                "laneMmr": {**LANE_MMR, "top": 1610.0},
            }
        ],
        "riotAccount": {
            "platform": "KR",
            "routingRegion": "ASIA",
            "gameName": "Dummy Top",
            "tagLine": "TEST",
            "profileIconId": 29,
            "mainPosition": "TOP",
        },
    }


def command_headers(key: str) -> dict[str, str]:
    return {**CSRF, "idempotency-key": key}


async def test_route_flag_service_admin_and_csrf(
    owner: AsyncEngine,
    runtime_client: httpx.AsyncClient,
    dummy_enabled_client: httpx.AsyncClient,
) -> None:
    await seed_session(owner)
    assert_failure(
        await runtime_client.get(USERS, headers=SESSION),
        404,
        "not_found",
        "요청한 API 경로를 찾을 수 없습니다.",
    )
    assert_failure(await dummy_enabled_client.get(USERS), 401, "unauthorized", "인증이 필요합니다.")
    settings = dummy_enabled_client._transport.app.state.settings
    settings.service_admin_ids = ""
    try:
        assert_failure(
            await dummy_enabled_client.get(USERS, headers=SESSION),
            403,
            "service_admin_required",
            "서비스 관리자 권한이 필요합니다.",
        )
    finally:
        settings.service_admin_ids = USER_ID
    assert_failure(
        await dummy_enabled_client.post(
            USERS, json=body("blocked-csrf"), headers={**SESSION, "idempotency-key": "csrf"}
        ),
        403,
        "csrf_validation_failed",
        "요청 출처를 확인할 수 없습니다.",
    )
    assert_failure(
        await dummy_enabled_client.post(USERS, json=body("blocked-key"), headers=CSRF),
        400,
        "invalid_idempotency_key",
        "Idempotency-Key가 올바르지 않습니다.",
    )


async def test_create_full_projection_replay_search_and_catalog(
    owner: AsyncEngine, dummy_enabled_client: httpx.AsyncClient
) -> None:
    await seed_session(owner)
    await guild_fixture(owner, GUILD_A, "Alpha", ROLE_A)
    path = USERS
    catalog = await dummy_enabled_client.get(USERS + "/catalog", headers=SESSION)
    assert catalog.status_code == 200, catalog.text
    assert any(
        row["guildId"] == GUILD_A and row["activeSeasonId"] is not None
        for row in catalog.json()["data"]["guilds"]
    )
    headers = command_headers("create-top-01")
    response = await dummy_enabled_client.post(
        path, json=body("top-01", guild=GUILD_A), headers=headers
    )
    assert response.status_code == 201, response.text
    created = response.json()["data"]
    assert created["userId"].startswith("dmy_")
    assert uuid.UUID(created["userId"].removeprefix("dmy_")).version == 7
    assert created["identityKind"] == "DUMMY"
    assert created["privacy"] == {
        "publicProfileEnabled": False,
        "showRiotIds": True,
        "allowExternalRecordSearch": False,
    }
    assert created["riotAccount"] == {
        "platform": "KR",
        "routingRegion": "ASIA",
        "gameName": "Dummy Top",
        "tagLine": "TEST",
        "profileIconId": 29,
        "mainPosition": "TOP",
    }
    assert created["championPreferences"] == {
        lane: [] for lane in ("TOP", "JUNGLE", "MID", "BOTTOM", "SUPPORT")
    }
    assert created["guildMemberships"][0]["laneMmr"] == {**LANE_MMR, "top": 1610.0}
    assert created["groupIds"] == []
    assert (await dummy_enabled_client.get(f"{path}/{created['userId']}", headers=SESSION)).json()[
        "data"
    ] == created
    replay = await dummy_enabled_client.post(
        path, json=body("top-01", guild=GUILD_A), headers=headers
    )
    assert replay.status_code == 201 and replay.json()["data"] == created
    assert_failure(
        await dummy_enabled_client.post(path, json=body("top-02", guild=GUILD_A), headers=headers),
        409,
        "idempotency_conflict",
        "같은 Idempotency-Key가 다른 요청에 사용되었습니다.",
    )
    listed = await dummy_enabled_client.get(
        USERS, headers=SESSION, params={"query": "TOP", "limit": 1}
    )
    assert listed.status_code == 200, listed.text
    assert listed.json()["data"] == {"users": [created], "total": 1}
    assert (
        await dummy_enabled_client.get(USERS, headers=SESSION, params={"guildId": GUILD_B})
    ).json()["data"] == {"users": [], "total": 0}
    async with owner.connect() as c:
        assert await c.scalar(text("SELECT count(*) FROM dummy_user_profiles")) == 1
        assert await c.scalar(text("SELECT count(*) FROM user_lane_mmr")) == 5
        assert await c.scalar(text("SELECT count(*) FROM dummy_user_audit_log")) == 1
        assert await c.scalar(text("SELECT count(*) FROM admin_audit_log")) == 1
        assert (
            await c.scalar(text("SELECT revision FROM guild_admin_state WHERE scope='MEMBERS'"))
            == 1
        )
        account = (
            await c.execute(
                text("SELECT origin, puuid, last_synced_at FROM riot_accounts WHERE user_id=:user"),
                {"user": created["userId"]},
            )
        ).one()
        assert account.origin == "DUMMY" and account.puuid.startswith("dummy:")
        assert account.last_synced_at is None


async def test_bulk_rollback_and_validation_reasons(
    owner: AsyncEngine, dummy_enabled_client: httpx.AsyncClient
) -> None:
    await seed_session(owner)
    await guild_fixture(owner, GUILD_A, "Alpha", ROLE_A)
    assert_failure(
        await dummy_enabled_client.post(
            USERS, json=body("Invalid Key"), headers=command_headers("invalid-stable")
        ),
        422,
        "invalid_dummy_user_input",
        "더미 사용자 입력값을 확인해 주세요.",
    )
    assert_failure(
        await dummy_enabled_client.post(
            USERS + "/bulk",
            json={"users": [body("rollback-one"), body("rollback-two", guild=GUILD_B)]},
            headers=command_headers("bulk-rollback"),
        ),
        404,
        "guild_not_found",
        "배치할 길드를 찾을 수 없습니다.",
    )
    async with owner.connect() as c:
        assert await c.scalar(text("SELECT count(*) FROM dummy_user_profiles")) == 0
        assert await c.scalar(text("SELECT count(*) FROM idempotency_records")) == 0
    valid = {"users": [body("bulk-one"), body("bulk-two")]}
    response = await dummy_enabled_client.post(
        USERS + "/bulk", json=valid, headers=command_headers("bulk-create")
    )
    assert response.status_code == 201, response.text
    created = response.json()["data"]
    assert created["total"] == 2 and len(created["users"]) == 2
    replay = await dummy_enabled_client.post(
        USERS + "/bulk", json=valid, headers=command_headers("bulk-create")
    )
    assert replay.status_code == 201, replay.text
    assert replay.json()["data"] == created
    assert_failure(
        await dummy_enabled_client.post(
            USERS + "/bulk",
            json={"users": [body("duplicate"), body("duplicate")]},
            headers=command_headers("bulk-duplicate"),
        ),
        409,
        "dummy_stable_key_conflict",
        "이미 사용 중인 더미 사용자 식별 키입니다.",
    )
    assert_failure(
        await dummy_enabled_client.get(USERS, headers=SESSION, params={"limit": 101}),
        422,
        "invalid_dummy_user_input",
        "더미 사용자 입력값을 확인해 주세요.",
    )
