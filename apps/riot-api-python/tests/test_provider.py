"""Riot upstream routing, encoding, credentials and failure classification."""

import asyncio

import httpx
import pytest

from monkeybot_riot_api.domain import ErrorKind, Platform, RiotError
from monkeybot_riot_api.provider import HttpRiotProvider


@pytest.mark.asyncio
async def test_account_route_encodes_segments_and_sends_private_token() -> None:
    requests: list[httpx.Request] = []

    def reply(request: httpx.Request) -> httpx.Response:
        requests.append(request)
        return httpx.Response(200, json={"puuid": "p", "gameName": "A/B", "tagLine": "KR#1"})

    async with httpx.AsyncClient(transport=httpx.MockTransport(reply)) as client:
        provider = HttpRiotProvider(client, "RGAPI-secret")
        account = await provider.account_by_riot_id(Platform.parse("kr"), "A/B", "KR#1")
    assert account.puuid == "p"
    assert (
        str(requests[0].url)
        == "https://asia.api.riotgames.com/riot/account/v1/accounts/by-riot-id/A%2FB/KR%231"
    )
    assert requests[0].headers["X-Riot-Token"] == "RGAPI-secret"


@pytest.mark.asyncio
async def test_ranked_season_query_and_rate_limit_header() -> None:
    requests: list[httpx.Request] = []

    def reply(request: httpx.Request) -> httpx.Response:
        requests.append(request)
        return httpx.Response(429, headers={"Retry-After": "3"})

    async with httpx.AsyncClient(transport=httpx.MockTransport(reply)) as client:
        provider = HttpRiotProvider(client, "RGAPI-secret")
        with pytest.raises(RiotError) as error:
            await provider.match_ids(
                Platform.parse("kr"), "p", start=100, count=100, start_time=1735689600, ranked=True
            )
    assert error.value.kind == ErrorKind.RATE_LIMITED
    assert error.value.retry_after_seconds == 3
    assert dict(requests[0].url.params) == {
        "start": "100",
        "count": "100",
        "startTime": "1735689600",
        "type": "ranked",
    }


@pytest.mark.asyncio
@pytest.mark.parametrize(
    ("status", "kind"),
    [
        (400, ErrorKind.INVALID_REQUEST),
        (401, ErrorKind.CREDENTIALS_REJECTED),
        (404, ErrorKind.NOT_FOUND),
        (503, ErrorKind.UNAVAILABLE),
        (418, ErrorKind.BAD_GATEWAY),
    ],
)
async def test_provider_status_mapping(status: int, kind: ErrorKind) -> None:
    async with httpx.AsyncClient(
        transport=httpx.MockTransport(lambda _: httpx.Response(status))
    ) as client:
        with pytest.raises(RiotError) as error:
            await HttpRiotProvider(client, "key").account_by_puuid(Platform.parse("NA"), "puuid")
    assert error.value.kind == kind


@pytest.mark.asyncio
async def test_malformed_provider_payload_is_stale_eligible_bad_gateway() -> None:
    async with httpx.AsyncClient(
        transport=httpx.MockTransport(lambda _: httpx.Response(200, json={"puuid": "p"}))
    ) as client:
        with pytest.raises(RiotError) as error:
            await HttpRiotProvider(client, "key").account_by_puuid(Platform.parse("KR"), "puuid")
    assert error.value.kind == ErrorKind.BAD_GATEWAY
    assert error.value.can_use_stale


@pytest.mark.asyncio
async def test_total_provider_request_timeout() -> None:
    async def slow(_: httpx.Request) -> httpx.Response:
        await asyncio.sleep(0.05)
        return httpx.Response(200, json={"puuid": "p", "gameName": "G", "tagLine": "T"})

    async with httpx.AsyncClient(transport=httpx.MockTransport(slow)) as client:
        with pytest.raises(RiotError) as error:
            await HttpRiotProvider(client, "key", request_timeout_ms=1).account_by_puuid(
                Platform.parse("KR"), "p"
            )
    assert error.value.kind == ErrorKind.TIMEOUT
