import json

import httpx
import pytest
from pydantic import SecretStr
from sqlalchemy import text

from monkeybot_api.core.config import Settings
from monkeybot_api.core.errors import ApiError
from monkeybot_api.services import internal_guilds
from tests.support.assertions import assert_failure
from tests.support.riot_accounts import PATH, account, authenticate

pytestmark = pytest.mark.postgres


async def test_riot_account_flow_returns_full_snapshot_and_preserves_one_main(
    owner, runtime_client, monkeypatch
):
    headers = await authenticate(owner, runtime_client)
    assert (await runtime_client.get(PATH)).json()["data"] == {"riotAccounts": []}

    async def resolve(_settings, game_name, _tag_line, _platform, _region):
        return account("puuid-two", "Canonical Two", 20) if game_name == "Second" else account()

    async def refresh(_settings, _puuid, _platform, _region):
        return account(name="Canonical Renamed", icon=22)

    monkeypatch.setattr(internal_guilds, "resolve_account", resolve)
    monkeypatch.setattr(internal_guilds, "resolve_account_by_puuid", refresh)
    first = await runtime_client.post(
        PATH,
        json={"gameName": "Alias One", "tagLine": "kr1", "platform": "kr"},
        headers=headers,
    )
    assert first.status_code == 201, first.text
    first_data = first.json()["data"]["riotAccounts"]
    assert len(first_data) == 1
    assert (
        first_data[0].items()
        >= {
            "puuid": "puuid-one",
            "platform": "KR",
            "routingRegion": "ASIA",
            "gameName": "Canonical One",
            "tagLine": "KR1",
            "profileIconId": 12,
            "isMain": True,
            "mainPosition": None,
        }.items()
    )
    assert first_data[0]["lastSyncedAt"] == "2026-01-01T00:00:00Z"
    assert first_data[0]["createdAt"].endswith("Z")
    second = await runtime_client.post(
        PATH,
        json={"gameName": "Second", "tagLine": "KR1", "platform": "KR"},
        headers=headers,
    )
    assert second.status_code == 201
    assert [row["puuid"] for row in second.json()["data"]["riotAccounts"]] == [
        "puuid-one",
        "puuid-two",
    ]
    promoted = await runtime_client.put(PATH + "/puuid-two/main", headers=headers)
    assert promoted.status_code == 200, promoted.text
    assert [row["puuid"] for row in promoted.json()["data"]["riotAccounts"]] == [
        "puuid-two",
        "puuid-one",
    ]
    assert promoted.json()["data"]["riotAccounts"][0]["isMain"] is True
    deleted = await runtime_client.delete(PATH + "/puuid-two", headers=headers)
    assert deleted.status_code == 200, deleted.text
    assert [row["puuid"] for row in deleted.json()["data"]["riotAccounts"]] == ["puuid-one"]
    assert deleted.json()["data"]["riotAccounts"][0]["isMain"] is True
    refreshed = await runtime_client.post(PATH + "/puuid-one/refresh", headers=headers)
    assert refreshed.status_code == 200, refreshed.text
    assert refreshed.json()["data"]["riotAccounts"][0]["gameName"] == "Canonical Renamed"
    assert refreshed.json()["data"]["riotAccounts"][0]["profileIconId"] == 22
    assert (await runtime_client.get(PATH)).json()["data"] == refreshed.json()["data"]
    assert (await runtime_client.head(PATH)).status_code == 200
    async with owner.connect() as c:
        assert await c.scalar(text("SELECT count(*) FROM riot_accounts WHERE is_main")) == 1


async def test_riot_account_denials_prevent_lookup_and_keep_existing_account(
    owner, runtime_client, monkeypatch
):
    assert_failure(await runtime_client.get(PATH), 401, "unauthorized", "인증이 필요합니다.")
    headers = await authenticate(owner, runtime_client)
    called = 0

    async def resolve(*_args):
        nonlocal called
        called += 1
        return account()

    monkeypatch.setattr(internal_guilds, "resolve_account", resolve)
    body = {"gameName": "Alias One", "tagLine": "KR1", "platform": "kr"}
    assert_failure(
        await runtime_client.post(PATH, json=body),
        403,
        "csrf_validation_failed",
        "요청 출처를 확인할 수 없습니다.",
    )
    assert_failure(
        await runtime_client.post(PATH, json={**body, "puuid": "injected"}, headers=headers),
        422,
        "unprocessable_entity",
        "요청 JSON의 필드 또는 값이 API 계약과 일치하지 않습니다.",
    )
    assert called == 0
    assert (await runtime_client.post(PATH, json=body, headers=headers)).status_code == 201
    assert called == 1
    assert_failure(
        await runtime_client.put(PATH + "/invalid%20puuid/main", headers=headers),
        400,
        "invalid_riot_account",
        "Riot 계정 식별자가 올바르지 않습니다.",
    )
    assert_failure(
        await runtime_client.delete(PATH + "/not-owned", headers=headers),
        404,
        "riot_account_not_found",
        "요청한 Riot 계정을 찾을 수 없습니다.",
    )
    assert (await runtime_client.get(PATH)).json()["data"]["riotAccounts"][0][
        "puuid"
    ] == "puuid-one"


async def test_riot_refresh_checks_ownership_before_provider_call(
    owner, runtime_client, monkeypatch
):
    headers = await authenticate(owner, runtime_client)
    called = False

    async def no_lookup(*_args):
        nonlocal called
        called = True
        return account()

    monkeypatch.setattr(internal_guilds, "resolve_account_by_puuid", no_lookup)
    assert_failure(
        await runtime_client.post(PATH + "/not-owned/refresh", headers=headers),
        404,
        "riot_account_not_found",
        "요청한 Riot 계정을 찾을 수 없습니다.",
    )
    assert not called


async def test_riot_link_prevents_other_owner_and_canonical_id_conflict(
    owner, runtime_client, monkeypatch
):
    headers = await authenticate(owner, runtime_client)

    async def first(*_args):
        return account()

    monkeypatch.setattr(internal_guilds, "resolve_account", first)
    body = {"gameName": "Alias One", "tagLine": "KR1", "platform": "KR"}
    assert (await runtime_client.post(PATH, json=body, headers=headers)).status_code == 201

    async def same_riot_id(*_args):
        return account("puuid-other")

    monkeypatch.setattr(internal_guilds, "resolve_account", same_riot_id)
    assert_failure(
        await runtime_client.post(PATH, json=body, headers=headers),
        409,
        "riot_id_conflict",
        "동일한 Riot ID가 다른 계정에 연결되어 있습니다.",
    )
    async with owner.begin() as c:
        await c.execute(
            text("""
            INSERT INTO users (user_id, identity_kind, display_name, created_at, updated_at)
            VALUES ('328516947210438657', 'DISCORD', 'other', now(), now())
        """)
        )
        await c.execute(
            text("""
            UPDATE riot_accounts SET user_id = '328516947210438657' WHERE puuid = 'puuid-one'
        """)
        )
    monkeypatch.setattr(internal_guilds, "resolve_account", first)
    assert_failure(
        await runtime_client.post(PATH, json=body, headers=headers),
        409,
        "riot_account_already_linked",
        "이 Riot 계정은 이미 다른 사용자에게 연결되어 있습니다.",
    )
    assert (await runtime_client.get(PATH)).json()["data"] == {"riotAccounts": []}


async def test_riot_refresh_puuid_request_and_mismatched_provider_identity(monkeypatch):
    settings = Settings(
        env="test",
        public_origin="http://localhost:5173",
        riot_service_url="http://riot.local/internal/riot/",
        riot_service_token=SecretStr("isolated-riot-service-token-32-chars"),
    )
    original_client = httpx.AsyncClient
    seen: list[dict] = []

    def handler(request: httpx.Request) -> httpx.Response:
        seen.append(json.loads(request.content))
        return httpx.Response(
            200,
            json={
                "account": {"puuid": "different-puuid", "gameName": "Name", "tagLine": "KR1"},
                "platform": "KR",
                "routingRegion": "ASIA",
                "profileIconId": None,
                "summonerLevel": None,
                "syncedAt": "2026-01-01T00:00:00Z",
            },
        )

    monkeypatch.setattr(
        internal_guilds.httpx,
        "AsyncClient",
        lambda **kwargs: original_client(transport=httpx.MockTransport(handler), **kwargs),
    )
    with pytest.raises(ApiError) as mismatch:
        await internal_guilds.resolve_account_by_puuid(settings, "puuid-one", "KR", "ASIA")
    assert (mismatch.value.status, mismatch.value.code, mismatch.value.message) == (
        502,
        "riot_upstream_invalid_response",
        "Riot 검색 응답 형식이 올바르지 않습니다.",
    )
    assert seen == [{"platform": "KR", "lookup": {"kind": "puuid", "puuid": "puuid-one"}}]
