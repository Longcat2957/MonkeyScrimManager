"""Account hydration, match projection, bounded season sampling and records cache."""

import asyncio
from collections import defaultdict
from collections.abc import Iterable
from copy import deepcopy
from datetime import UTC, datetime, timedelta
from time import monotonic
from typing import Any

from .domain import ErrorKind, Platform, RiotError, RiotId, validate_puuid
from .provider import Account, Match, MatchInfo, Participant, Rank, RiotProvider

FRESH_SECONDS = 5 * 60
STALE_SECONDS = 30 * 60
MAX_CACHE_ENTRIES = 1024
MAX_SEASON_PAGES = 20
SEASON_PAGE_SIZE = 100
MATCH_CONCURRENCY = 4

type JsonObject = dict[str, Any]
type CacheKey = tuple[str, str, str]


class RiotService:
    def __init__(
        self, provider: RiotProvider, match_count: int, season_start_unix_seconds: int
    ) -> None:
        self.provider = provider
        self.match_count = min(max(match_count, 1), 20)
        self.season_start = season_start_unix_seconds
        self._cache: dict[CacheKey, tuple[float, JsonObject]] = {}
        self._locks: dict[CacheKey, asyncio.Lock] = {}
        self._lock_users: dict[CacheKey, int] = {}
        self._locks_guard = asyncio.Lock()

    async def resolve_by_riot_id(self, riot_id: RiotId) -> JsonObject:
        account = await self.provider.account_by_riot_id(
            riot_id.platform, riot_id.game_name, riot_id.tag_line
        )
        return await self._hydrate(account, riot_id.platform)

    async def resolve_by_puuid(self, platform: Platform, puuid: str) -> JsonObject:
        account = await self.provider.account_by_puuid(platform, validate_puuid(puuid))
        return await self._hydrate(account, platform)

    async def _hydrate(self, account: Account, platform: Platform) -> JsonObject:
        summoner = await self.provider.summoner_by_puuid(platform, account.puuid)
        return {
            "account": _account(account),
            "platform": platform.code,
            "routingRegion": platform.routing,
            "profileIconId": summoner.profile_icon_id,
            "summonerLevel": summoner.summoner_level,
            "syncedAt": _now(),
        }

    async def records(self, riot_id: RiotId, *, force_refresh: bool = False) -> JsonObject:
        key = (riot_id.platform.code, riot_id.game_name.lower(), riot_id.tag_line.lower())
        if not force_refresh:
            fresh = self._cached(key, FRESH_SECONDS, "fresh")
            if fresh is not None:
                return fresh
        async with self._locks_guard:
            key_lock = self._locks.setdefault(key, asyncio.Lock())
            self._lock_users[key] = self._lock_users.get(key, 0) + 1
        try:
            async with key_lock:
                if not force_refresh:
                    fresh = self._cached(key, FRESH_SECONDS, "fresh")
                    if fresh is not None:
                        return fresh
                stale = self._cached(key, STALE_SECONDS, "stale")
                try:
                    snapshot = await self._fetch_records(riot_id)
                except RiotError as exc:
                    if exc.can_use_stale and stale is not None:
                        return stale
                    raise
                snapshot["cache"] = {"state": "refreshed" if force_refresh else "fresh"}
                if len(self._cache) >= MAX_CACHE_ENTRIES and key not in self._cache:
                    oldest = min(self._cache, key=lambda item: self._cache[item][0])
                    self._cache.pop(oldest)
                self._cache[key] = (monotonic(), deepcopy(snapshot))
                return snapshot
        finally:
            async with self._locks_guard:
                remaining = self._lock_users[key] - 1
                if remaining:
                    self._lock_users[key] = remaining
                else:
                    self._lock_users.pop(key)
                    self._locks.pop(key)

    def _cached(self, key: CacheKey, max_age: int, state: str) -> JsonObject | None:
        entry = self._cache.get(key)
        if entry is None or monotonic() - entry[0] > max_age:
            return None
        snapshot = deepcopy(entry[1])
        snapshot["cache"] = {"state": state}
        return snapshot

    async def recent_matches_page(self, riot_id: RiotId, offset: int, count: int) -> JsonObject:
        if count < 1 or count > 20 or offset < 0 or offset > 10000:
            raise RiotError(ErrorKind.INVALID_REQUEST)
        platform = riot_id.platform
        account = await self.provider.account_by_riot_id(
            platform, riot_id.game_name, riot_id.tag_line
        )
        raw_ids = await self.provider.match_ids(
            platform, account.puuid, start=offset, count=count + 1
        )
        unique_ids = _ordered_unique(raw_ids[: count + 1])
        has_more = len(unique_ids) > count
        ids = unique_ids[:count]
        details = await self._match_details(platform, account.puuid, ids)
        matches = [details[match_id]["summary"] for match_id in ids if match_id in details]
        return {
            "account": _account(account),
            "platform": platform.code,
            "routingRegion": platform.routing,
            "targetPuuid": account.puuid,
            "matches": matches,
            "page": {
                "offset": offset,
                "requested": count,
                "returned": len(matches),
                "hasMore": has_more,
                "nextOffset": offset + count if has_more else None,
            },
        }

    async def _fetch_records(self, riot_id: RiotId) -> JsonObject:
        platform = riot_id.platform
        account = await self.provider.account_by_riot_id(
            platform, riot_id.game_name, riot_id.tag_line
        )
        puuid = account.puuid
        summoner_task = asyncio.create_task(self.provider.summoner_by_puuid(platform, puuid))
        ranks_task = asyncio.create_task(self.provider.league_entries_by_puuid(platform, puuid))
        recent_task = asyncio.create_task(
            self.provider.match_ids(platform, puuid, start=0, count=self.match_count)
        )
        season_task = asyncio.create_task(self._season_match_ids(platform, puuid))
        tasks = (summoner_task, ranks_task, recent_task, season_task)
        try:
            summoner, ranks, recent_ids, (season_ids, truncated) = await asyncio.gather(*tasks)
        except BaseException:
            for task in tasks:
                task.cancel()
            await asyncio.gather(*tasks, return_exceptions=True)
            raise
        recent_ids = _ordered_unique(recent_ids[: self.match_count])
        details = await self._match_details(
            platform, puuid, _ordered_unique([*recent_ids, *season_ids])
        )
        recent = [details[match_id]["summary"] for match_id in recent_ids if match_id in details]
        season = [details[match_id] for match_id in season_ids if match_id in details]
        return {
            "account": _account(account),
            "platform": platform.code,
            "routingRegion": platform.routing,
            "profileIconId": summoner.profile_icon_id,
            "summonerLevel": summoner.summoner_level,
            "ranks": [_rank(rank) for rank in sorted(ranks, key=_rank_sort)],
            "championAggregates": _aggregate_champions(season),
            "recentMatches": recent,
            "sample": {
                "recentMatches": {"requested": self.match_count, "returned": len(recent)},
                "championAggregates": {
                    "seasonStartUnixSeconds": self.season_start,
                    "discoveredMatches": len(season_ids),
                    "analyzedMatches": len(season),
                    "truncated": truncated,
                },
            },
            "refreshedAt": _now(),
            "cache": {"state": "fresh"},
        }

    async def _season_match_ids(self, platform: Platform, puuid: str) -> tuple[list[str], bool]:
        ids: list[str] = []
        seen: set[str] = set()
        for page_index in range(MAX_SEASON_PAGES):
            page = await self.provider.match_ids(
                platform,
                puuid,
                start=page_index * SEASON_PAGE_SIZE,
                count=SEASON_PAGE_SIZE,
                start_time=self.season_start,
                ranked=True,
            )
            for match_id in page[:SEASON_PAGE_SIZE]:
                if match_id not in seen:
                    seen.add(match_id)
                    ids.append(match_id)
            if len(page) < SEASON_PAGE_SIZE:
                return ids, False
            if len(page) > SEASON_PAGE_SIZE:
                return ids, True
            if len(ids) >= SEASON_PAGE_SIZE * MAX_SEASON_PAGES:
                overflow = await self.provider.match_ids(
                    platform,
                    puuid,
                    start=SEASON_PAGE_SIZE * MAX_SEASON_PAGES,
                    count=1,
                    start_time=self.season_start,
                    ranked=True,
                )
                return ids, bool(overflow)
        return ids, True

    async def _match_details(
        self, platform: Platform, target_puuid: str, ids: list[str]
    ) -> dict[str, JsonObject]:
        semaphore = asyncio.Semaphore(MATCH_CONCURRENCY)

        async def fetch(match_id: str) -> tuple[str, JsonObject] | None:
            async with semaphore:
                try:
                    detail = await self.provider.match_detail(platform, match_id)
                except RiotError as exc:
                    if exc.kind == ErrorKind.NOT_FOUND:
                        return None
                    raise
            if detail.metadata.match_id != match_id:
                raise RiotError(ErrorKind.BAD_GATEWAY, stale_eligible=True)
            return match_id, _match(detail, target_puuid)

        tasks = [asyncio.create_task(fetch(match_id)) for match_id in ids]
        try:
            results = await asyncio.gather(*tasks)
        except BaseException:
            for task in tasks:
                task.cancel()
            await asyncio.gather(*tasks, return_exceptions=True)
            raise
        return {
            match_id: detail
            for result in results
            if result is not None
            for match_id, detail in [result]
        }


def _account(account: Account) -> JsonObject:
    return {"puuid": account.puuid, "gameName": account.game_name, "tagLine": account.tag_line}


def _rank(rank: Rank) -> JsonObject:
    return {
        "queueType": rank.queue_type,
        "tier": rank.tier,
        "rank": rank.rank,
        "leaguePoints": rank.league_points,
        "wins": rank.wins,
        "losses": rank.losses,
    }


def _rank_sort(rank: Rank) -> tuple[int, str]:
    priority = {"RANKED_SOLO_5x5": 0, "RANKED_FLEX_SR": 1}.get(rank.queue_type, 2)
    return priority, rank.queue_type


def _ordered_unique(values: Iterable[str]) -> list[str]:
    return list(dict.fromkeys(values))


def _now() -> str:
    return datetime.now(UTC).isoformat().replace("+00:00", "Z")


def _started_at(milliseconds: int) -> str:
    try:
        timestamp = datetime.fromtimestamp(milliseconds // 1000, UTC)
        timestamp += timedelta(milliseconds=milliseconds % 1000)
        return timestamp.isoformat().replace("+00:00", "Z")
    except OverflowError, ValueError, OSError:
        return "1970-01-01T00:00:00Z"


def _match(detail: Match, target_puuid: str) -> JsonObject:
    info: MatchInfo = detail.info
    if sum(p.puuid == target_puuid for p in info.participants) != 1:
        raise RiotError(ErrorKind.BAD_GATEWAY, stale_eligible=True)
    summary: JsonObject = {
        "matchId": detail.metadata.match_id,
        "queueId": info.queue_id,
        "gameVersion": info.game_version,
        "durationSeconds": info.game_duration,
        "startedAt": _started_at(info.game_creation),
        "targetPuuid": target_puuid,
        "participants": [_participant(p) for p in info.participants],
        "teams": [
            {
                "teamId": team.team_id,
                "win": team.win,
                "objectives": {
                    "baronKills": team.objectives.baron.kills,
                    "championKills": team.objectives.champion.kills,
                    "dragonKills": team.objectives.dragon.kills,
                    "hordeKills": team.objectives.horde.kills,
                    "inhibitorKills": team.objectives.inhibitor.kills,
                    "riftHeraldKills": team.objectives.rift_herald.kills,
                    "towerKills": team.objectives.tower.kills,
                },
            }
            for team in info.teams
        ],
    }
    return {
        "summary": summary,
        "target": next(p for p in info.participants if p.puuid == target_puuid),
    }


def _participant(p: Participant) -> JsonObject:
    return {
        "puuid": p.puuid,
        "gameName": p.riot_id_game_name,
        "tagLine": p.riot_id_tagline,
        "championName": p.champion_name,
        "championLevel": p.champ_level,
        "teamId": p.team_id,
        "win": p.win,
        "kills": p.kills,
        "deaths": p.deaths,
        "assists": p.assists,
        "creepScore": p.total_minions_killed + p.neutral_minions_killed,
        "goldEarned": p.gold_earned,
        "damageDealtToChampions": p.total_damage_dealt_to_champions,
        "damageTaken": p.total_damage_taken,
        "visionScore": p.vision_score,
        "summonerSpellIds": [p.summoner1_id, p.summoner2_id],
        "itemIds": [p.item0, p.item1, p.item2, p.item3, p.item4, p.item5, p.item6],
        "tripleKills": p.triple_kills,
        "quadraKills": p.quadra_kills,
        "pentaKills": p.penta_kills,
        "teamPosition": p.team_position,
    }


def _aggregate_champions(details: list[JsonObject]) -> list[JsonObject]:
    aggregates: dict[str, JsonObject] = defaultdict(dict)
    for detail in details:
        participant: Participant = detail["target"]
        name = participant.champion_name
        if not aggregates[name]:
            aggregates[name] = {
                "championName": name,
                "games": 0,
                "wins": 0,
                "kills": 0,
                "deaths": 0,
                "assists": 0,
                "creepScore": 0,
                "goldEarned": 0,
                "tripleKills": 0,
                "quadraKills": 0,
                "pentaKills": 0,
            }
        entry = aggregates[name]
        entry["games"] += 1
        entry["wins"] += int(participant.win)
        entry["kills"] += participant.kills
        entry["deaths"] += participant.deaths
        entry["assists"] += participant.assists
        entry["creepScore"] += participant.total_minions_killed + participant.neutral_minions_killed
        entry["goldEarned"] += participant.gold_earned
        entry["tripleKills"] += participant.triple_kills
        entry["quadraKills"] += participant.quadra_kills
        entry["pentaKills"] += participant.penta_kills
    return sorted(aggregates.values(), key=lambda a: (-a["games"], -a["wins"], a["championName"]))
