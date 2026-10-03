import uuid

import httpx
import pytest
from pydantic import SecretStr
from sqlalchemy import text

from monkeybot_api.core.config import Settings
from monkeybot_api.core.errors import ApiError
from monkeybot_api.services import internal_guilds as service
from tests.support.assertions import assert_failure
from tests.support.guilds.context import GUILD_A, GUILD_B, USER_ID
from tests.support.internal_guilds import (
    REGISTER_A,
    REGISTER_B,
    SETUP_A,
    actor,
    bot_headers,
    registration_body,
    resolved,
    setup_body,
)

pytestmark = pytest.mark.postgres


async def test_setup_creates_one_guild_and_replays_original_response(owner, runtime_client):
    body = setup_body(name="  First Guild  ")
    response = await runtime_client.post(SETUP_A, json=body, headers=bot_headers())
    assert response.status_code == 200, response.text
    data = response.json()["data"]
    assert data["guild"]["id"] == GUILD_A
    assert data["guild"]["displayName"] == "First Guild"
    assert data["guild"]["currentSeason"]["name"] == "첫 시즌"
    assert uuid.UUID(data["guild"]["currentSeason"]["id"]).version == 7
    assert data["guild"]["dashboardHref"] == f"/g/{GUILD_A}"
    assert (await runtime_client.post(SETUP_A, json=body, headers=bot_headers())).json()[
        "data"
    ] == data
    assert_failure(
        await runtime_client.post(SETUP_A, json=body, headers=bot_headers("123456789012345680")),
        409,
        "guild_already_configured",
        "이미 초기 설정이 완료된 길드입니다. 길드 관리 화면에서 설정을 변경해 주세요.",
    )
    async with owner.connect() as c:
        guild = (
            (
                await c.execute(
                    text("SELECT status, installed_by FROM guilds WHERE guild_id = :guild"),
                    {"guild": GUILD_A},
                )
            )
            .mappings()
            .one()
        )
        assert dict(guild) == {"status": "ACTIVE", "installed_by": USER_ID}
        assert await c.scalar(text("SELECT count(*) FROM guild_config")) == 1
        assert await c.scalar(text("SELECT count(*) FROM seasons WHERE status = 'ACTIVE'")) == 1
        assert await c.scalar(text("SELECT count(*) FROM guild_counters")) == 4
        assert (
            await c.scalar(text("SELECT count(*) FROM admin_audit_log WHERE action='GUILD_SETUP'"))
            == 1
        )
        record = (
            (
                await c.execute(
                    text(
                        "SELECT scope_key, guild_id, principal_type, state FROM idempotency_records"
                    )
                )
            )
            .mappings()
            .one()
        )
        assert dict(record) == {
            "scope_key": "GLOBAL",
            "guild_id": None,
            "principal_type": "BOT",
            "state": "COMPLETED",
        }


async def test_setup_rejects_disabled_guild_and_invalid_commands(owner, runtime_client):
    async with owner.begin() as c:
        await c.execute(
            text("""
            INSERT INTO guilds (guild_id, discord_name, status, installed_at, updated_at)
            VALUES (:guild, 'disabled', 'DISABLED', now(), now())
        """),
            {"guild": GUILD_A},
        )
    assert_failure(
        await runtime_client.post(SETUP_A, json=setup_body(), headers=bot_headers()),
        409,
        "guild_setup_unavailable",
        "비활성화되었거나 연결이 해제된 길드는 초기 설정을 실행할 수 없습니다.",
    )
    response = await runtime_client.post(SETUP_A, json=setup_body(), headers={})
    assert_failure(response, 401, "unauthorized", "인증이 필요합니다.")
    body = setup_body()
    body["seasonName"] = " "
    response = await runtime_client.post(SETUP_A, json=body, headers=bot_headers())
    assert_failure(
        response, 400, "invalid_bot_command", "Bot command 요청 형식이 올바르지 않습니다."
    )
    assert response.json()["error"]["details"] == {"field": "seasonName"}


async def test_registration_preflight_replay_and_active_member_reuse(
    owner, runtime_client, monkeypatch
):
    assert (
        await runtime_client.post(SETUP_A, json=setup_body(), headers=bot_headers())
    ).status_code == 200
    calls = 0

    async def fake_resolve(*_args):
        nonlocal calls
        calls += 1
        return resolved()

    monkeypatch.setattr(service, "resolve_account", fake_resolve)
    first = await runtime_client.post(
        REGISTER_A, json=registration_body(), headers=bot_headers("123456789012345680")
    )
    assert first.status_code == 200, first.text
    data = first.json()["data"]
    assert data == {
        "member": {"id": USER_ID, "displayName": "Discord user", "status": "ACTIVE"},
        "riotAccount": {
            "puuid": "riot-puuid-1",
            "gameName": "Hide on bush",
            "tagLine": "KR1",
            "platform": "KR",
            "isMain": True,
        },
        "registration": "EXISTING",
        "accountLink": "CREATED",
    }
    replay = await runtime_client.post(
        REGISTER_A, json=registration_body(), headers=bot_headers("123456789012345680")
    )
    assert replay.status_code == 200
    assert replay.json()["data"] == data
    assert calls == 1
    repeated = await runtime_client.post(
        REGISTER_A, json=registration_body(), headers=bot_headers("123456789012345681")
    )
    assert repeated.status_code == 200
    assert repeated.json()["data"]["accountLink"] == "EXISTING"
    assert calls == 2
    async with owner.connect() as c:
        assert await c.scalar(text("SELECT count(*) FROM riot_accounts")) == 1
        assert await c.scalar(text("SELECT count(*) FROM guild_members")) == 1


async def test_registration_rejects_inactive_member_before_riot(owner, runtime_client, monkeypatch):
    await runtime_client.post(SETUP_A, json=setup_body(), headers=bot_headers())
    body = registration_body()
    body["actor"] = actor("328516947210438657")
    called = False

    async def no_riot(*_args):
        nonlocal called
        called = True
        return resolved()

    monkeypatch.setattr(service, "resolve_account", no_riot)
    async with owner.begin() as c:
        await c.execute(
            text("""
            INSERT INTO users (user_id, identity_kind, display_name, created_at, updated_at)
            VALUES (:user, 'DISCORD', 'other', now(), now())
        """),
            {"user": body["actor"]["id"]},
        )
        await c.execute(
            text("""
            INSERT INTO guild_members (guild_id, user_id, display_name, status, first_seen_at)
            VALUES (:guild, :user, 'other', 'INACTIVE', now())
        """),
            {"guild": GUILD_A, "user": body["actor"]["id"]},
        )
    response = await runtime_client.post(
        REGISTER_A, json=body, headers=bot_headers("123456789012345682")
    )
    assert_failure(
        response, 403, "member_inactive", "비활성화된 길드 멤버는 이 작업을 수행할 수 없습니다."
    )
    assert not called
    async with owner.connect() as c:
        assert await c.scalar(text("SELECT count(*) FROM riot_accounts")) == 0


async def test_registration_link_conflict_rolls_back_membership(owner, runtime_client, monkeypatch):
    await runtime_client.post(SETUP_A, json=setup_body(), headers=bot_headers())

    async def same_account(*_args):
        return resolved()

    monkeypatch.setattr(service, "resolve_account", same_account)
    first = await runtime_client.post(
        REGISTER_A, json=registration_body(), headers=bot_headers("123456789012345680")
    )
    assert first.status_code == 200
    other = "328516947210438657"
    response = await runtime_client.post(
        REGISTER_A, json=registration_body(other), headers=bot_headers("123456789012345681")
    )
    assert_failure(
        response,
        409,
        "riot_account_already_linked",
        "이 Riot 계정은 이미 다른 사용자에게 연결되어 있습니다.",
    )
    async with owner.connect() as c:
        assert (
            await c.scalar(
                text("SELECT count(*) FROM guild_members WHERE user_id = :user"), {"user": other}
            )
            == 0
        )
        assert (
            await c.scalar(
                text("SELECT count(*) FROM users WHERE user_id = :user"), {"user": other}
            )
            == 0
        )


async def test_registration_creates_new_membership_in_second_guild(
    owner, runtime_client, monkeypatch
):
    await runtime_client.post(SETUP_A, json=setup_body(), headers=bot_headers())
    setup_b = f"/internal/guilds/{GUILD_B}/setup"
    assert (
        await runtime_client.post(
            setup_b,
            json=setup_body(identifier="328516947210438657", name="Guild B"),
            headers=bot_headers("123456789012345683"),
        )
    ).status_code == 200

    async def same_account(*_args):
        return resolved()

    monkeypatch.setattr(service, "resolve_account", same_account)
    first = await runtime_client.post(
        REGISTER_A, json=registration_body(), headers=bot_headers("123456789012345680")
    )
    second = await runtime_client.post(
        REGISTER_B, json=registration_body(), headers=bot_headers("123456789012345681")
    )
    assert first.status_code == second.status_code == 200
    assert second.json()["data"]["registration"] == "CREATED"
    assert second.json()["data"]["accountLink"] == "EXISTING"
    async with owner.connect() as c:
        # Both setup actors and the newly registered user are members.
        assert await c.scalar(text("SELECT count(*) FROM guild_members")) == 3
        assert await c.scalar(text("SELECT count(*) FROM riot_accounts")) == 1


async def test_riot_resolver_classifies_upstream_error_and_validates_response(monkeypatch):
    settings = Settings(
        env="test",
        public_origin="http://localhost:5173",
        riot_service_url="http://riot.local/internal/riot/",
        riot_service_token=SecretStr("isolated-riot-service-token-32-chars"),
    )
    original_client = httpx.AsyncClient

    def transport_for(response: httpx.Response):
        monkeypatch.setattr(
            service.httpx,
            "AsyncClient",
            lambda **kwargs: original_client(
                transport=httpx.MockTransport(lambda _request: response), **kwargs
            ),
        )

    transport_for(
        httpx.Response(
            429,
            json={"error": {"code": "riot_rate_limited"}},
            headers={"Retry-After": "13"},
        )
    )
    with pytest.raises(ApiError) as rate_limit:
        await service.resolve_account(settings, "Hide on bush", "KR1", "KR", "ASIA")
    assert (rate_limit.value.status, rate_limit.value.code, rate_limit.value.details) == (
        429,
        "riot_rate_limited",
        {"retryAfterSeconds": 13},
    )
    transport_for(
        httpx.Response(
            200,
            json={
                "account": {"puuid": "riot-puuid-1", "gameName": "Hide on bush", "tagLine": "KR1"},
                "platform": "KR",
                "routingRegion": "ASIA",
                "profileIconId": 10,
                "summonerLevel": 100,
                "syncedAt": "2026-01-01T00:00:00Z",
            },
        )
    )
    assert (
        await service.resolve_account(settings, "Hide on bush", "KR1", "KR", "ASIA")
    ).puuid == "riot-puuid-1"
