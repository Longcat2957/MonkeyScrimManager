"""Riot HTTP client and typed, untrusted provider payloads."""

import asyncio
from collections.abc import Mapping
from typing import Protocol
from urllib.parse import quote

import httpx
from pydantic import BaseModel, ConfigDict, Field, ValidationError
from pydantic.alias_generators import to_camel

from .domain import ErrorKind, Platform, RiotError


class ProviderModel(BaseModel):
    model_config = ConfigDict(alias_generator=to_camel, extra="ignore", strict=True)


class Account(ProviderModel):
    puuid: str
    game_name: str
    tag_line: str


class Summoner(ProviderModel):
    profile_icon_id: int | None = None
    summoner_level: int | None = None


class Rank(ProviderModel):
    queue_type: str
    tier: str | None = None
    rank: str | None = None
    league_points: int = 0
    wins: int = 0
    losses: int = 0


class Objective(ProviderModel):
    kills: int = 0


class Objectives(ProviderModel):
    baron: Objective = Field(default_factory=Objective)
    champion: Objective = Field(default_factory=Objective)
    dragon: Objective = Field(default_factory=Objective)
    horde: Objective = Field(default_factory=Objective)
    inhibitor: Objective = Field(default_factory=Objective)
    rift_herald: Objective = Field(default_factory=Objective)
    tower: Objective = Field(default_factory=Objective)


class Team(ProviderModel):
    team_id: int
    win: bool
    objectives: Objectives = Field(default_factory=Objectives)


class Participant(ProviderModel):
    puuid: str
    riot_id_game_name: str | None = None
    riot_id_tagline: str | None = None
    champion_name: str
    champ_level: int
    team_id: int
    win: bool
    kills: int
    deaths: int
    assists: int
    total_minions_killed: int
    neutral_minions_killed: int
    gold_earned: int
    total_damage_dealt_to_champions: int
    total_damage_taken: int
    vision_score: int
    summoner1_id: int
    summoner2_id: int
    item0: int
    item1: int
    item2: int
    item3: int
    item4: int
    item5: int
    item6: int
    triple_kills: int = 0
    quadra_kills: int = 0
    penta_kills: int = 0
    team_position: str = ""


class MatchMetadata(ProviderModel):
    match_id: str


class MatchInfo(ProviderModel):
    game_creation: int
    game_duration: int
    game_version: str
    queue_id: int
    participants: list[Participant]
    teams: list[Team] = Field(default_factory=list)


class Match(ProviderModel):
    metadata: MatchMetadata
    info: MatchInfo


class RiotProvider(Protocol):
    async def account_by_riot_id(
        self, platform: Platform, game_name: str, tag_line: str
    ) -> Account: ...
    async def account_by_puuid(self, platform: Platform, puuid: str) -> Account: ...
    async def summoner_by_puuid(self, platform: Platform, puuid: str) -> Summoner: ...
    async def league_entries_by_puuid(self, platform: Platform, puuid: str) -> list[Rank]: ...
    async def match_ids(
        self,
        platform: Platform,
        puuid: str,
        *,
        start: int,
        count: int,
        start_time: int | None = None,
        ranked: bool = False,
    ) -> list[str]: ...
    async def match_detail(self, platform: Platform, match_id: str) -> Match: ...


class HttpRiotProvider:
    def __init__(
        self, client: httpx.AsyncClient, api_key: str, request_timeout_ms: int = 5000
    ) -> None:
        self.client = client
        self.api_key = api_key
        self.request_timeout_seconds = request_timeout_ms / 1000

    async def _get(
        self, host: str, segments: tuple[str, ...], params: Mapping[str, str] | None = None
    ) -> object:
        path = "/".join(quote(segment, safe="") for segment in segments)
        url = f"https://{host}/{path}"
        try:
            async with asyncio.timeout(self.request_timeout_seconds):
                response = await self.client.get(
                    url, params=params, headers={"X-Riot-Token": self.api_key}
                )
        except (TimeoutError, httpx.TimeoutException) as exc:
            raise RiotError(ErrorKind.TIMEOUT) from exc
        except httpx.RequestError as exc:
            raise RiotError(ErrorKind.UNAVAILABLE) from exc
        if response.status_code >= 400:
            raise _status_error(response)
        try:
            return response.json()
        except ValueError as exc:
            raise RiotError(ErrorKind.BAD_GATEWAY, stale_eligible=True) from exc

    async def account_by_riot_id(
        self, platform: Platform, game_name: str, tag_line: str
    ) -> Account:
        payload = await self._get(
            platform.routing_host,
            ("riot", "account", "v1", "accounts", "by-riot-id", game_name, tag_line),
        )
        return _decode(Account, payload)

    async def account_by_puuid(self, platform: Platform, puuid: str) -> Account:
        payload = await self._get(
            platform.routing_host, ("riot", "account", "v1", "accounts", "by-puuid", puuid)
        )
        return _decode(Account, payload)

    async def summoner_by_puuid(self, platform: Platform, puuid: str) -> Summoner:
        payload = await self._get(
            platform.host, ("lol", "summoner", "v4", "summoners", "by-puuid", puuid)
        )
        return _decode(Summoner, payload)

    async def league_entries_by_puuid(self, platform: Platform, puuid: str) -> list[Rank]:
        payload = await self._get(
            platform.host, ("lol", "league", "v4", "entries", "by-puuid", puuid)
        )
        if not isinstance(payload, list):
            raise RiotError(ErrorKind.BAD_GATEWAY, stale_eligible=True)
        return [_decode(Rank, entry) for entry in payload]

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
        params = {"start": str(start), "count": str(count)}
        if start_time is not None:
            params["startTime"] = str(start_time)
        if ranked:
            params["type"] = "ranked"
        payload = await self._get(
            platform.routing_host,
            ("lol", "match", "v5", "matches", "by-puuid", puuid, "ids"),
            params,
        )
        if not isinstance(payload, list) or any(not isinstance(item, str) for item in payload):
            raise RiotError(ErrorKind.BAD_GATEWAY, stale_eligible=True)
        return payload

    async def match_detail(self, platform: Platform, match_id: str) -> Match:
        payload = await self._get(
            platform.routing_host, ("lol", "match", "v5", "matches", match_id)
        )
        return _decode(Match, payload)


def _decode[T: BaseModel](model: type[T], payload: object) -> T:
    try:
        return model.model_validate(payload)
    except ValidationError as exc:
        raise RiotError(ErrorKind.BAD_GATEWAY, stale_eligible=True) from exc


def _status_error(response: httpx.Response) -> RiotError:
    status = response.status_code
    if status == 400:
        return RiotError(ErrorKind.INVALID_REQUEST)
    if status in (401, 403):
        return RiotError(ErrorKind.CREDENTIALS_REJECTED)
    if status == 404:
        return RiotError(ErrorKind.NOT_FOUND)
    if status == 429:
        raw = response.headers.get("Retry-After", "")
        try:
            retry = int(raw) if raw.isascii() and raw.isdecimal() else None
        except ValueError:
            retry = None
        if retry is not None and retry > 2**64 - 1:
            retry = None
        return RiotError(ErrorKind.RATE_LIMITED, retry_after_seconds=retry)
    if status >= 500:
        return RiotError(ErrorKind.UNAVAILABLE)
    return RiotError(ErrorKind.BAD_GATEWAY)
