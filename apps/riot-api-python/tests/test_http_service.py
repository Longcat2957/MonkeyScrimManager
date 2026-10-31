"""HTTP and service contract checks without live Riot traffic."""

import asyncio
from collections import Counter

import httpx
import pytest
from pydantic import SecretStr

from monkeybot_riot_api.config import Settings
from monkeybot_riot_api.domain import ErrorKind, Platform, RiotError, RiotId
from monkeybot_riot_api.http import create_app
from monkeybot_riot_api.provider import Account, Match, Rank, Summoner
from monkeybot_riot_api.service import RiotService

TOKEN = "0123456789abcdef0123456789abcdef"
AUTH = {"Authorization": f"Bearer {TOKEN}"}
RIOT_ID = {"gameName": "Longcat", "tagLine": "KR1", "platform": "KR"}


def match(match_id: str, *, champion: str = "Ahri", win: bool = True) -> Match:
    return Match.model_validate(
        {
            "metadata": {"matchId": match_id},
            "info": {
                "gameCreation": 1720000000000,
                "gameDuration": 1800,
                "gameVersion": "16.13.1",
                "queueId": 420,
                "participants": [
                    {
                        "puuid": "target-puuid",
                        "riotIdGameName": "Longcat",
                        "riotIdTagline": "KR1",
                        "championName": champion,
                        "champLevel": 18,
                        "teamId": 100,
                        "win": win,
                        "kills": 10,
                        "deaths": 2,
                        "assists": 8,
                        "totalMinionsKilled": 200,
                        "neutralMinionsKilled": 20,
                        "goldEarned": 14000,
                        "totalDamageDealtToChampions": 30000,
                        "totalDamageTaken": 20000,
                        "visionScore": 40,
                        "summoner1Id": 4,
                        "summoner2Id": 11,
                        **{f"item{index}": index + 1001 for index in range(7)},
                        "tripleKills": 1,
                        "quadraKills": 1,
                        "pentaKills": 1,
                        "teamPosition": "MIDDLE",
                    }
                ],
                "teams": [
                    {
                        "teamId": 100,
                        "win": True,
                        "objectives": {
                            "baron": {"kills": 1},
                            "dragon": {"kills": 3},
                            "tower": {"kills": 11},
                        },
                    }
                ],
            },
        }
    )


class FakeProvider:
    def __init__(self) -> None:
        self.calls: Counter[str] = Counter()
        self.failure: RiotError | None = None
        self.started: asyncio.Event | None = None
        self.release: asyncio.Event | None = None
        self.details = {
            "M1": match("M1"),
            "M2": match("M2"),
            "M3": match("M3", champion="Zed", win=False),
        }

    async def account_by_riot_id(
        self, platform: Platform, game_name: str, tag_line: str
    ) -> Account:
        self.calls["account"] += 1
        if self.started is not None:
            self.started.set()
        if self.release is not None:
            await self.release.wait()
        if self.failure:
            raise self.failure
        assert (platform.code, game_name, tag_line) == ("KR", "Longcat", "KR1")
        return Account(puuid="target-puuid", gameName="Longcat", tagLine="KR1")

    async def account_by_puuid(self, platform: Platform, puuid: str) -> Account:
        self.calls["account_by_puuid"] += 1
        assert (platform.code, puuid) == ("KR", "target-puuid")
        return Account(puuid=puuid, gameName="Longcat", tagLine="KR1")

    async def summoner_by_puuid(self, platform: Platform, puuid: str) -> Summoner:
        self.calls["summoner"] += 1
        return Summoner(profileIconId=123, summonerLevel=456)

    async def league_entries_by_puuid(self, platform: Platform, puuid: str) -> list[Rank]:
        self.calls["ranks"] += 1
        return [
            Rank(queueType="RANKED_FLEX_SR", leaguePoints=7, wins=3, losses=2),
            Rank(
                queueType="RANKED_SOLO_5x5",
                tier="PLATINUM",
                rank="III",
                leaguePoints=23,
                wins=22,
                losses=29,
            ),
        ]

    async def match_ids(
        self,
        platform: Platform,
        puuid: str,
        *,
        start: int,
        count: int,
        start_time: int | None = None,
        ranked: bool = False,
    ) -> list[str]:
        self.calls["season_ids" if ranked else "recent_ids"] += 1
        if ranked:
            assert start_time == 1735689600
            return ["M2", "M3"] if start == 0 else []
        if start == 5:
            return ["M1", "M2"][:count]
        return ["M1", "M2"][:count]

    async def match_detail(self, platform: Platform, match_id: str) -> Match:
        self.calls["detail"] += 1
        return self.details[match_id]


def settings() -> Settings:
    return Settings(
        api_key=SecretStr("RGAPI-test"),
        internal_token=SecretStr(TOKEN),
        season_start_unix_seconds=1735689600,
        match_count=2,
    )


@pytest.mark.asyncio
async def test_all_five_routes_project_expected_records_and_pagination() -> None:
    provider = FakeProvider()
    app = create_app(settings(), provider)
    async with httpx.AsyncClient(
        transport=httpx.ASGITransport(app=app), base_url="http://test"
    ) as client:
        assert (await client.get("/healthz")).json() == {"status": "ok"}
        assert (await client.head("/healthz")).status_code == 200
        resolved = await client.post(
            "/internal/riot/accounts/resolve",
            headers=AUTH,
            json={
                "platform": "kr",
                "lookup": {"kind": "riotId", "gameName": " Longcat ", "tagLine": "KR1"},
            },
        )
        assert resolved.status_code == 200
        assert resolved.json()["account"] == {
            "puuid": "target-puuid",
            "gameName": "Longcat",
            "tagLine": "KR1",
        }
        assert resolved.json()["profileIconId"] == 123
        by_puuid = await client.post(
            "/internal/riot/accounts/resolve",
            headers=AUTH,
            json={"platform": "KR", "lookup": {"kind": "puuid", "puuid": "target-puuid"}},
        )
        assert by_puuid.status_code == 200
        assert provider.calls["account_by_puuid"] == 1

        records = await client.get("/internal/riot/records", headers=AUTH, params=RIOT_ID)
        assert records.status_code == 200
        data = records.json()
        assert data["cache"] == {"state": "fresh"}
        assert data["sample"]["recentMatches"] == {"requested": 2, "returned": 2}
        assert data["sample"]["championAggregates"] == {
            "seasonStartUnixSeconds": 1735689600,
            "discoveredMatches": 2,
            "analyzedMatches": 2,
            "truncated": False,
        }
        assert [rank["queueType"] for rank in data["ranks"]] == [
            "RANKED_SOLO_5x5",
            "RANKED_FLEX_SR",
        ]
        assert [entry["championName"] for entry in data["championAggregates"]] == ["Ahri", "Zed"]
        assert data["championAggregates"][0]["tripleKills"] == 1
        assert data["recentMatches"][0]["participants"][0]["creepScore"] == 220
        assert data["recentMatches"][0]["teams"][0]["objectives"]["towerKills"] == 11
        assert (await client.get("/internal/riot/records", headers=AUTH, params=RIOT_ID)).json()[
            "cache"
        ]["state"] == "fresh"
        assert provider.calls["ranks"] == 1

        page = await client.get(
            "/internal/riot/records/matches",
            headers=AUTH,
            params={**RIOT_ID, "start": 5, "count": 1},
        )
        assert page.status_code == 200
        assert page.json()["page"] == {
            "offset": 5,
            "requested": 1,
            "returned": 1,
            "hasMore": True,
            "nextOffset": 6,
        }
        assert [game["matchId"] for game in page.json()["matches"]] == ["M1"]

        refreshed = await client.post("/internal/riot/records/refresh", headers=AUTH, json=RIOT_ID)
        assert refreshed.status_code == 200
        assert refreshed.json()["cache"] == {"state": "refreshed"}
        assert provider.calls["ranks"] == 2


@pytest.mark.asyncio
async def test_auth_input_and_safe_provider_failures() -> None:
    provider = FakeProvider()
    app = create_app(settings(), provider)
    async with httpx.AsyncClient(
        transport=httpx.ASGITransport(app=app), base_url="http://test"
    ) as client:
        unauthorized = await client.get("/internal/riot/records", params=RIOT_ID)
        assert unauthorized.status_code == 401
        assert unauthorized.json() == {
            "error": {"code": "unauthorized", "message": "Internal service authentication failed."}
        }
        unauthorized_bad_platform = await client.get(
            "/internal/riot/records", params={**RIOT_ID, "platform": "MOON"}
        )
        assert unauthorized_bad_platform.status_code == 401
        bad_page = await client.get(
            "/internal/riot/records/matches", headers=AUTH, params={**RIOT_ID, "count": 0}
        )
        assert bad_page.status_code == 400
        assert bad_page.json()["error"] == {
            "code": "invalid_request",
            "message": "Riot lookup parameters are invalid.",
        }
        invalid_lookup = await client.post(
            "/internal/riot/accounts/resolve",
            headers=AUTH,
            json={"platform": "KR", "lookup": {"kind": "puuid", "puuid": "bad puuid"}},
        )
        assert invalid_lookup.status_code == 400
        missing = await client.get("/internal/riot/no-such-route")
        assert missing.json()["error"]["code"] == "not_found"
        assert (await client.get("/healthz/")).status_code == 404
        oversized = await client.post(
            "/internal/riot/accounts/resolve", headers=AUTH, json={"padding": "x" * 17000}
        )
        assert oversized.status_code == 413
        provider.failure = RiotError(ErrorKind.RATE_LIMITED, retry_after_seconds=7)
        limited = await client.get("/internal/riot/records", headers=AUTH, params=RIOT_ID)
        assert limited.status_code == 429
        assert limited.headers["Retry-After"] == "7"
        assert limited.json()["error"] == {
            "code": "riot_rate_limited",
            "message": "The Riot API rate limit was exceeded.",
        }
        provider.failure = RiotError(ErrorKind.CREDENTIALS_REJECTED)
        rejected = await client.get("/internal/riot/records", headers=AUTH, params=RIOT_ID)
        assert rejected.status_code == 503
        assert rejected.json()["error"]["code"] == "riot_credentials_rejected"


@pytest.mark.asyncio
async def test_stale_fallback_and_single_flight(monkeypatch: pytest.MonkeyPatch) -> None:
    provider = FakeProvider()
    service = RiotService(provider, 2, 1735689600)
    riot_id = RiotId.parse(**{"game_name": "Longcat", "tag_line": "KR1", "platform": "KR"})
    clock = [1000.0]
    monkeypatch.setattr("monkeybot_riot_api.service.monotonic", lambda: clock[0])
    first = await service.records(riot_id)
    assert first["cache"]["state"] == "fresh"
    clock[0] += 301
    provider.failure = RiotError(ErrorKind.TIMEOUT)
    assert (await service.records(riot_id))["cache"]["state"] == "stale"
    assert (await service.records(riot_id, force_refresh=True))["cache"]["state"] == "stale"
    provider.failure = RiotError(ErrorKind.CREDENTIALS_REJECTED)
    with pytest.raises(RiotError, match="riot_credentials_rejected"):
        await service.records(riot_id)
    clock[0] += 1800
    provider.failure = RiotError(ErrorKind.TIMEOUT)
    with pytest.raises(RiotError, match="riot_timeout"):
        await service.records(riot_id)

    provider = FakeProvider()
    provider.started = asyncio.Event()
    provider.release = asyncio.Event()
    service = RiotService(provider, 2, 1735689600)
    first_task = asyncio.create_task(service.records(riot_id))
    await provider.started.wait()
    second_task = asyncio.create_task(service.records(riot_id))
    await asyncio.sleep(0)
    provider.release.set()
    await asyncio.gather(first_task, second_task)
    assert provider.calls["account"] == 1
    assert provider.calls["detail"] == 3


@pytest.mark.asyncio
async def test_match_mismatch_is_bad_gateway() -> None:
    provider = FakeProvider()
    provider.details["M1"] = match("OTHER")
    app = create_app(settings(), provider)
    async with httpx.AsyncClient(
        transport=httpx.ASGITransport(app=app), base_url="http://test"
    ) as client:
        response = await client.get("/internal/riot/records", headers=AUTH, params=RIOT_ID)
    assert response.status_code == 502
    assert response.json()["error"] == {
        "code": "riot_bad_gateway",
        "message": "The Riot API returned an unusable response.",
    }


@pytest.mark.asyncio
@pytest.mark.parametrize("has_overflow", [False, True])
async def test_season_discovery_stops_at_2000_and_checks_overflow(has_overflow: bool) -> None:
    class FullSeasonProvider(FakeProvider):
        async def match_ids(
            self,
            platform: Platform,
            puuid: str,
            *,
            start: int,
            count: int,
            start_time: int | None = None,
            ranked: bool = False,
        ) -> list[str]:
            assert ranked and start_time == 1735689600
            self.calls["season_ids"] += 1
            if start == 2000:
                return ["OVERFLOW"] if has_overflow else []
            return [f"M{i}" for i in range(start, start + count)]

    provider = FullSeasonProvider()
    service = RiotService(provider, 2, 1735689600)
    ids, truncated = await service._season_match_ids(Platform.parse("KR"), "target-puuid")
    assert len(ids) == 2000
    assert truncated is has_overflow
    assert provider.calls["season_ids"] == 21
