"""Public Riot records cache and refresh use the private service only when needed."""

import uuid

import pytest
from pydantic import SecretStr
from sqlalchemy import text

from monkeybot_api.core.errors import ApiError
from monkeybot_api.services import riot_records
from tests.support.assertions import assert_failure
from tests.support.guilds.context import authenticated_setup
from tests.support.users.profile import csrf

pytestmark = pytest.mark.postgres

RECORDS = "/api/riot/records?gameName=Longcat&tagLine=KR1&platform=kr"
MATCHES = RECORDS.replace("/records?", "/records/matches?")


def record(name: str, puuid: str) -> dict:
    return {
        "account": {"puuid": puuid, "gameName": name, "tagLine": "KR1"},
        "platform": "KR",
        "routingRegion": "ASIA",
        "profileIconId": 12,
        "summonerLevel": 100,
        "ranks": [],
        "championAggregates": [],
        "recentMatches": [],
        "sample": {
            "recentMatches": {"requested": 10, "returned": 0},
            "championAggregates": {
                "seasonStartUnixSeconds": 1_700_000_000,
                "discoveredMatches": 0,
                "analyzedMatches": 0,
                "truncated": False,
            },
        },
        "refreshedAt": "2026-10-03T00:00:00Z",
        "cache": {"state": "fresh"},
    }


def page(name: str, puuid: str) -> dict:
    return {
        "account": {"puuid": puuid, "gameName": name, "tagLine": "KR1"},
        "platform": "KR",
        "routingRegion": "ASIA",
        "targetPuuid": puuid,
        "matches": [],
        "page": {
            "offset": 0,
            "requested": 10,
            "returned": 0,
            "hasMore": False,
            "nextOffset": None,
        },
    }


async def test_records_snapshot_recent_page_cooldown_and_stale_fallback(
    owner, runtime_client, monkeypatch
):
    name = f"Longcat{uuid.uuid7().hex[:12]}"
    puuid = f"riot-puuid-{uuid.uuid7().hex}"
    records_url = RECORDS.replace("Longcat", name)
    matches_url = MATCHES.replace("Longcat", name)
    settings = runtime_client._transport.app.state.settings
    settings.riot_service_url = "http://riot.test/internal/riot/"
    settings.riot_service_token = SecretStr("isolated-riot-service-token-32-characters")
    calls = []

    async def private(_settings, method, path, *, params=None, body=None):
        calls.append((method, path))
        return page(name, puuid) if path == "records/matches" else record(name, puuid)

    monkeypatch.setattr(riot_records, "private_request", private)
    first = await runtime_client.get(records_url)
    assert first.status_code == 200, first.text
    assert first.json()["data"]["cache"]["state"] == "fresh"
    second = await runtime_client.get(records_url)
    assert second.status_code == 200
    assert second.json()["data"] == first.json()["data"]
    matches = await runtime_client.get(matches_url)
    assert matches.status_code == 200, matches.text
    replay = await runtime_client.get(matches_url)
    assert replay.status_code == 200
    assert replay.json()["data"] == matches.json()["data"]
    assert calls == [("GET", "records"), ("GET", "records/matches")]

    headers = csrf(await authenticated_setup(owner, runtime_client))
    refresh = await runtime_client.post(
        "/api/riot/records/refresh",
        headers=headers,
        json={"gameName": name, "tagLine": "KR1", "platform": "KR"},
    )
    assert_failure(
        refresh,
        429,
        "riot_records_refresh_cooldown",
        "전적은 30초마다 갱신할 수 있습니다.",
    )
    async with owner.begin() as c:
        await c.execute(
            text("""
            UPDATE riot_records_snapshots
            SET refreshed_at=clock_timestamp() - INTERVAL '31 seconds'
            WHERE puuid=:puuid
        """),
            {"puuid": puuid},
        )

    async def failed_private(_settings, method, path, *, params=None, body=None):
        raise ApiError(503, "riot_unavailable", "Riot API를 일시적으로 사용할 수 없습니다.")

    monkeypatch.setattr(riot_records, "private_request", failed_private)
    stale = await runtime_client.post(
        "/api/riot/records/refresh",
        headers=headers,
        json={"gameName": name, "tagLine": "KR1", "platform": "KR"},
    )
    assert stale.status_code == 200, stale.text
    assert stale.json()["data"]["cache"]["state"] == "stale"
    async with owner.connect() as c:
        assert (
            await c.scalar(
                text("SELECT count(*) FROM riot_records_snapshots WHERE puuid=:puuid"),
                {"puuid": puuid},
            )
            == 1
        )
        assert (
            await c.scalar(
                text("""
            SELECT lease_token FROM riot_records_refresh_gates
            WHERE normalized_game_name=:name AND normalized_tag_line='kr1'
        """),
                {"name": name.casefold()},
            )
            is None
        )


async def test_records_query_validation_and_unconfigured_dependency(runtime_client):
    assert_failure(
        await runtime_client.get("/api/riot/records?gameName=&tagLine=KR1&platform=KR"),
        400,
        "invalid_riot_records_request",
        "Riot 전적 조회 조건이 올바르지 않습니다.",
    )
    assert_failure(
        await runtime_client.get(MATCHES + "&count=21"),
        400,
        "invalid_riot_records_request",
        "Riot 전적 조회 조건이 올바르지 않습니다.",
    )
    assert_failure(
        await runtime_client.get(RECORDS + "&platform=NA1"),
        400,
        "invalid_riot_records_request",
        "Riot 전적 조회 조건이 올바르지 않습니다.",
    )
    assert_failure(
        await runtime_client.get(MATCHES + "&count=1_0"),
        400,
        "invalid_riot_records_request",
        "Riot 전적 조회 조건이 올바르지 않습니다.",
    )
    assert_failure(
        await runtime_client.get(RECORDS),
        503,
        "riot_unavailable",
        "Riot 검색 서비스를 사용할 수 없습니다.",
    )


async def test_invalid_recent_match_is_rejected_before_page_cache(
    owner, runtime_client, monkeypatch
):
    name = f"Page{uuid.uuid7().hex[:12]}"
    puuid = f"riot-puuid-{uuid.uuid7().hex}"
    settings = runtime_client._transport.app.state.settings
    settings.riot_service_url = "http://riot.test/internal/riot/"
    settings.riot_service_token = SecretStr("isolated-riot-service-token-32-characters")
    invalid_page = page(name, puuid)
    invalid_page["matches"] = [
        {
            "matchId": "KR_100",
            "queueId": 420,
            "gameVersion": "16.13.1",
            "durationSeconds": -1,
            "startedAt": "2026-10-03T00:00:00Z",
            "targetPuuid": puuid,
            "participants": [
                {
                    "puuid": puuid,
                    "championName": "LeeSin",
                    "championLevel": 18,
                    "teamId": 100,
                    "win": True,
                    "kills": 10,
                    "deaths": 2,
                    "assists": 8,
                    "creepScore": 220,
                    "goldEarned": 14000,
                    "damageDealtToChampions": 30000,
                    "damageTaken": 20000,
                    "visionScore": 40,
                    "summonerSpellIds": [4, 11],
                    "itemIds": [1001, 1002, 1003, 1004, 1005, 1006, 3364],
                    "teamPosition": "JUNGLE",
                }
            ],
        }
    ]
    invalid_page["page"]["returned"] = 1

    async def private(_settings, method, path, *, params=None, body=None):
        return invalid_page if path == "records/matches" else record(name, puuid)

    monkeypatch.setattr(riot_records, "private_request", private)
    url = RECORDS.replace("Longcat", name)
    assert (await runtime_client.get(url)).status_code == 200
    response = await runtime_client.get(url.replace("/records?", "/records/matches?"))
    assert_failure(
        response,
        502,
        "riot_upstream_invalid_response",
        "Riot 검색 응답 형식이 올바르지 않습니다.",
    )
    async with owner.connect() as c:
        assert (
            await c.scalar(
                text("SELECT count(*) FROM riot_records_recent_pages WHERE puuid=:puuid"),
                {"puuid": puuid},
            )
            == 0
        )
