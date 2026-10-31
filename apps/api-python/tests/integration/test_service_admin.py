"""Service-admin status, reset guards, and isolated reset workflow."""

import uuid
from collections.abc import AsyncIterator
from datetime import UTC, datetime, timedelta

import httpx
import pytest
from pydantic import SecretStr
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncEngine

from monkeybot_api.app import create_app
from monkeybot_api.core.config import Settings
from monkeybot_api.services.service_admin import _bot_age
from tests.support.assertions import assert_failure
from tests.support.auth.session import TOKEN, USER_ID, seed_session
from tests.support.database import isolated_database_url
from tests.support.guilds.context import GUILD_A, GUILD_B, ROLE_A, ROLE_B, guild_fixture

pytestmark = pytest.mark.postgres

SESSION = {"cookie": f"monkeybot_session={TOKEN}"}
CSRF = {
    "cookie": f"monkeybot_session={TOKEN}; monkeybot_csrf=integration-csrf",
    "origin": "http://localhost:5173",
    "x-csrf-token": "integration-csrf",
}


def test_heartbeat_classification_exact_boundaries() -> None:
    checked = datetime(2026, 10, 3, 0, 0, tzinfo=UTC)
    assert _bot_age(checked - timedelta(seconds=90), checked)[0] == "normal"
    assert _bot_age(checked - timedelta(seconds=91), checked)[0] == "delayed"
    assert _bot_age(checked - timedelta(minutes=5), checked)[0] == "delayed"
    assert _bot_age(checked - timedelta(seconds=301), checked)[0] == "down"


@pytest.fixture
async def reset_client(owner: AsyncEngine) -> AsyncIterator[httpx.AsyncClient]:
    # The URL guard makes every destructive request in this module isolated to the test DB.
    settings = Settings(
        env="test",
        public_origin="http://localhost:5173",
        database_url=SecretStr(isolated_database_url("FASTAPI_TEST_RUNTIME_URL")),
        discord_application_id="123456789012345678",
        discord_client_secret=SecretStr("integration-test-only"),
        oauth_encryption_key=SecretStr("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
        service_admin_ids=USER_ID,
        dummy_users_enabled=True,
    )
    app = create_app(settings)
    async with app.router.lifespan_context(app):
        async with httpx.AsyncClient(
            transport=httpx.ASGITransport(app=app), base_url="http://localhost"
        ) as client:
            yield client


async def test_status_requires_service_admin_and_classifies_persisted_heartbeat(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    assert_failure(
        await runtime_client.get("/api/admin/status"), 401, "unauthorized", "인증이 필요합니다."
    )
    await seed_session(owner)
    settings = runtime_client._transport.app.state.settings
    settings.service_admin_ids = ""
    try:
        assert_failure(
            await runtime_client.get("/api/admin/status", headers=SESSION),
            403,
            "service_admin_required",
            "서비스 관리자 권한이 필요합니다.",
        )
    finally:
        settings.service_admin_ids = USER_ID
    async with owner.begin() as c:
        await c.execute(text("TRUNCATE bot_instances"))
    response = await runtime_client.get("/api/admin/status", headers=SESSION)
    assert response.status_code == 200, response.text
    status = response.json()["data"]
    assert [item["key"] for item in status["services"]] == [
        "API",
        "POSTGRESQL",
        "BOT_HEARTBEAT",
        "VALKEY",
    ]
    assert [item["status"] for item in status["services"]] == [
        "normal",
        "normal",
        "down",
        "delayed",
    ]
    bot = status["services"][2]
    assert bot["value"] == "heartbeat 없음" and bot["lastHealthyAt"] is None
    assert bot["issue"] == "마지막 heartbeat가 5분을 초과했거나 아직 수신되지 않았습니다."
    assert uuid.UUID(response.json()["meta"]["requestId"]).version == 7
    for age, expected in ((30, "normal"), (120, "delayed"), (360, "down")):
        heartbeat_at = datetime.now(UTC) - timedelta(seconds=age)
        async with owner.begin() as c:
            await c.execute(
                text("""
                INSERT INTO bot_instances
                    (instance_id, version, started_at, last_heartbeat_at, updated_at)
                VALUES ('status-test', 'test', :heartbeat, :heartbeat, :heartbeat)
                ON CONFLICT (instance_id) DO UPDATE
                SET last_heartbeat_at=EXCLUDED.last_heartbeat_at,
                    updated_at=EXCLUDED.updated_at
            """),
                {"heartbeat": heartbeat_at},
            )
        response = await runtime_client.get("/api/admin/status", headers=SESSION)
        assert response.status_code == 200, response.text
        bot = response.json()["data"]["services"][2]
        assert bot["status"] == expected
        assert datetime.fromisoformat(bot["lastHealthyAt"].replace("Z", "+00:00")) == heartbeat_at


async def test_reset_rejects_disabled_flag_and_invalid_request(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, reset_client: httpx.AsyncClient
) -> None:
    await seed_session(owner)
    path = f"/api/admin/guilds/{GUILD_A}/record-reset"
    assert_failure(
        await runtime_client.get(path, headers=SESSION),
        403,
        "test_record_reset_disabled",
        "테스트 기록 초기화 기능이 비활성화되어 있습니다.",
    )
    assert_failure(
        await reset_client.get("/api/admin/guilds/bad/record-reset", headers=SESSION),
        400,
        "invalid_guild_id",
        "Discord 서버 ID 형식이 올바르지 않습니다.",
    )
    assert_failure(
        await reset_client.get(path, headers=SESSION),
        404,
        "guild_not_found",
        "초기화할 서버를 찾을 수 없습니다.",
    )
    await guild_fixture(owner, GUILD_A, "Alpha", ROLE_A)
    assert_failure(
        await reset_client.post(path, json={"confirmation": f"RESET {GUILD_A}"}, headers=SESSION),
        403,
        "csrf_validation_failed",
        "요청 출처를 확인할 수 없습니다.",
    )
    assert_failure(
        await reset_client.post(path, json={"confirmation": "RESET other"}, headers=CSRF),
        422,
        "reset_confirmation_mismatch",
        "확인 문구가 대상 서버와 일치하지 않습니다.",
    )
    assert_failure(
        await reset_client.post(path, json={"confirmation": f"RESET {GUILD_A}"}, headers=CSRF),
        400,
        "invalid_idempotency_key",
        "Idempotency-Key가 올바르지 않습니다.",
    )


async def test_reset_preview_requires_active_guild_and_season(
    owner: AsyncEngine, reset_client: httpx.AsyncClient
) -> None:
    await seed_session(owner)
    await guild_fixture(owner, GUILD_A, "Alpha", ROLE_A)
    path = f"/api/admin/guilds/{GUILD_A}/record-reset"
    async with owner.begin() as c:
        await c.execute(
            text("UPDATE guilds SET status='DISABLED' WHERE guild_id=:guild"), {"guild": GUILD_A}
        )
    assert_failure(
        await reset_client.get(path, headers=SESSION),
        409,
        "guild_not_active",
        "활성 상태인 서버만 테스트 기록을 초기화할 수 있습니다.",
    )
    async with owner.begin() as c:
        await c.execute(
            text("UPDATE guilds SET status='ACTIVE' WHERE guild_id=:guild"), {"guild": GUILD_A}
        )
        await c.execute(text("DELETE FROM seasons WHERE guild_id=:guild"), {"guild": GUILD_A})
    assert_failure(
        await reset_client.get(path, headers=SESSION),
        409,
        "active_season_missing",
        "활성 시즌이 없어 테스트 기록을 초기화할 수 없습니다.",
    )


async def test_reset_preserves_guild_and_members_reseeds_and_replays(
    owner: AsyncEngine, reset_client: httpx.AsyncClient
) -> None:
    await seed_session(owner)
    old_a = await guild_fixture(owner, GUILD_A, "Alpha", ROLE_A)
    old_b = await guild_fixture(owner, GUILD_B, "Beta", ROLE_B)
    async with owner.begin() as c:
        for guild in (GUILD_A, GUILD_B):
            await c.execute(
                text("""
                INSERT INTO guild_counters (guild_id, counter_key, next_value)
                VALUES (:guild, 'SERIES', 8)
            """),
                {"guild": guild},
            )
            await c.execute(
                text("""
                INSERT INTO admin_audit_log
                    (guild_id, id, principal_type, principal_id, action, result, created_at)
                VALUES (:guild, :id, 'USER', :actor, 'TEST', 'APPLIED', now())
            """),
                {"guild": guild, "id": uuid.uuid7(), "actor": USER_ID},
            )
        await c.execute(
            text("UPDATE guild_config SET display_name='Alpha display' WHERE guild_id=:guild"),
            {"guild": GUILD_A},
        )
    path = f"/api/admin/guilds/{GUILD_A}/record-reset"
    preview_response = await reset_client.get(path, headers=SESSION)
    assert preview_response.status_code == 200, preview_response.text
    preview = preview_response.json()["data"]
    assert preview == {
        "guildId": GUILD_A,
        "displayName": "Alpha display",
        "memberCount": 1,
        "activeSeasonName": "현재 시즌",
        "confirmation": f"RESET {GUILD_A}",
        "counts": {
            "totalRows": 3,
            "seasons": 1,
            "recruitments": 0,
            "series": 0,
            "games": 0,
            "auctionRecruitments": 0,
            "auctionTournaments": 0,
            "mmrRows": 0,
            "importJobs": 0,
        },
    }
    key = str(uuid.uuid7())
    command_headers = {**CSRF, "idempotency-key": key}
    response = await reset_client.post(
        path, json={"confirmation": preview["confirmation"]}, headers=command_headers
    )
    assert response.status_code == 200, response.text
    result = response.json()["data"]
    assert result["guildId"] == GUILD_A
    assert result["displayName"] == "Alpha display"
    assert result["deleted"] == preview["counts"]
    assert result["freshSeason"]["name"] == "현재 시즌"
    new_a = uuid.UUID(result["freshSeason"]["id"])
    assert new_a != old_a
    replay = await reset_client.post(
        path, json={"confirmation": preview["confirmation"]}, headers=command_headers
    )
    assert replay.status_code == 200 and replay.json()["data"] == result
    assert_failure(
        await reset_client.post(
            f"/api/admin/guilds/{GUILD_B}/record-reset",
            json={"confirmation": f"RESET {GUILD_B}"},
            headers=command_headers,
        ),
        409,
        "idempotency_conflict",
        "같은 Idempotency-Key가 다른 초기화 요청에 사용되었습니다.",
    )
    async with owner.connect() as c:
        rows = (
            await c.execute(
                text("""
                SELECT guild_id, id, name FROM seasons ORDER BY guild_id
            """)
            )
        ).all()
        assert rows == [(GUILD_A, new_a, "현재 시즌"), (GUILD_B, old_b, "현재 시즌")]
        counters = (
            await c.execute(text("SELECT guild_id, counter_key, next_value FROM guild_counters"))
        ).all()
        assert sorted(counters) == sorted(
            [
                (GUILD_A, key, 1)
                for key in ("RECRUITMENT", "SERIES", "AUCTION_RECRUITMENT", "AUCTION_TOURNAMENT")
            ]
            + [(GUILD_B, "SERIES", 8)]
        )
        assert await c.scalar(text("SELECT count(*) FROM guilds")) == 2
        assert await c.scalar(text("SELECT count(*) FROM guild_members")) == 2
        assert await c.scalar(text("SELECT count(*) FROM admin_audit_log")) == 1
        assert await c.scalar(text("SELECT count(*) FROM idempotency_records")) == 1
