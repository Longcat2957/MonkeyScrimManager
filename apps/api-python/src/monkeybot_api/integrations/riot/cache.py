"""Cached Riot record DTOs used by the auction current-target profile."""

from typing import Annotated, Literal

from pydantic import BaseModel, ConfigDict, Field

I32 = Annotated[int, Field(ge=-(2**31), le=2**31 - 1)]
I64 = Annotated[int, Field(ge=-(2**63), le=2**63 - 1)]
U32 = Annotated[int, Field(ge=0, le=2**32 - 1)]
Usize = Annotated[int, Field(ge=0, le=2**64 - 1)]


class Record(BaseModel):
    model_config = ConfigDict(strict=True, extra="ignore")


class RiotAccount(Record):
    puuid: str
    gameName: str
    tagLine: str


class RiotRank(Record):
    queueType: str
    tier: str | None = None
    rank: str | None = None
    leaguePoints: I32
    wins: I32
    losses: I32


class RiotChampionAggregate(Record):
    championName: str
    games: U32
    wins: U32
    kills: I64
    deaths: I64
    assists: I64
    creepScore: I64
    goldEarned: I64
    tripleKills: I64 = 0
    quadraKills: I64 = 0
    pentaKills: I64 = 0


class RiotTeamObjectives(Record):
    baronKills: I32
    championKills: I32
    dragonKills: I32
    hordeKills: I32
    inhibitorKills: I32
    riftHeraldKills: I32
    towerKills: I32


class RiotMatchTeam(Record):
    teamId: I32
    win: bool
    objectives: RiotTeamObjectives


class RiotMatchParticipant(Record):
    puuid: str
    gameName: str | None = None
    tagLine: str | None = None
    championName: str
    championLevel: I32
    teamId: I32
    win: bool
    kills: I32
    deaths: I32
    assists: I32
    creepScore: I32
    goldEarned: I32
    damageDealtToChampions: I32
    damageTaken: I32
    visionScore: I32
    summonerSpellIds: Annotated[list[I32], Field(min_length=2, max_length=2)]
    itemIds: Annotated[list[I32], Field(min_length=7, max_length=7)]
    tripleKills: I32 = 0
    quadraKills: I32 = 0
    pentaKills: I32 = 0
    teamPosition: str


class RiotRecentMatch(Record):
    matchId: str
    queueId: I32
    gameVersion: str
    durationSeconds: I64
    startedAt: str
    targetPuuid: str
    participants: list[RiotMatchParticipant]
    teams: list[RiotMatchTeam] = Field(default_factory=list)


class RiotRecentMatchSample(Record):
    requested: Usize
    returned: Usize


class RiotChampionAggregateSample(Record):
    seasonStartUnixSeconds: I64
    discoveredMatches: Usize
    analyzedMatches: Usize
    truncated: bool


class RiotRecordsSample(Record):
    recentMatches: RiotRecentMatchSample
    championAggregates: RiotChampionAggregateSample


class RiotCacheMetadata(Record):
    state: Literal["fresh", "stale", "refreshed"]


class RiotRecords(Record):
    account: RiotAccount
    platform: str
    routingRegion: Literal["AMERICAS", "ASIA", "EUROPE", "SEA"]
    profileIconId: I32 | None = None
    summonerLevel: I64 | None = None
    ranks: list[RiotRank]
    championAggregates: list[RiotChampionAggregate]
    recentMatches: list[RiotRecentMatch]
    sample: RiotRecordsSample
    refreshedAt: str
    cache: RiotCacheMetadata


def valid_recent_matches(matches: list[RiotRecentMatch], account_puuid: str) -> bool:
    from monkeybot_api.core.text import trim

    if len({game.matchId for game in matches}) != len(matches):
        return False
    for game in matches:
        if (
            not trim(game.matchId)
            or not trim(game.gameVersion)
            or not trim(game.startedAt)
            or game.durationSeconds < 0
            or game.targetPuuid != account_puuid
            or len(game.participants) > 20
            or sum(p.puuid == game.targetPuuid for p in game.participants) != 1
        ):
            return False
        for p in game.participants:
            if (
                not trim(p.puuid)
                or not trim(p.championName)
                or min(
                    p.championLevel,
                    p.kills,
                    p.deaths,
                    p.assists,
                    p.creepScore,
                    p.goldEarned,
                    p.damageDealtToChampions,
                    p.damageTaken,
                    p.visionScore,
                    p.tripleKills,
                    p.quadraKills,
                    p.pentaKills,
                    *p.summonerSpellIds,
                    *p.itemIds,
                )
                < 0
            ):
                return False
        if any(min(t.objectives.model_dump().values()) < 0 for t in game.teams):
            return False
    return True


def validated(value: object) -> RiotRecords | None:
    import re

    from pydantic import ValidationError

    from monkeybot_api.core.text import trim

    try:
        r = RiotRecords.model_validate(value)
    except ValidationError:
        return None
    platform = trim(r.platform).translate(
        str.maketrans("ABCDEFGHIJKLMNOPQRSTUVWXYZ", "abcdefghijklmnopqrstuvwxyz")
    )
    groups = {
        "AMERICAS": {"br", "br1", "lan", "la1", "las", "la2", "na", "na1", "oce", "oc1"},
        "EUROPE": {"eune", "eun1", "euw", "euw1", "ru", "tr", "tr1"},
        "ASIA": {"jp", "jp1", "kr"},
        "SEA": {"ph", "ph2", "sg", "sg2", "th", "th2", "tw", "tw2", "vn", "vn2"},
    }
    if platform not in groups[r.routingRegion]:
        return None
    a = r.account
    if not re.fullmatch(r"[A-Za-z0-9_-]{1,256}", a.puuid):
        return None
    for value, maximum in ((a.gameName, 64), (a.tagLine, 16)):
        if (
            not 1 <= len(trim(value)) <= maximum
            or "#" in value
            or any(ord(ch) < 32 or 127 <= ord(ch) <= 159 for ch in trim(value))
        ):
            return None
    if (
        (r.profileIconId is not None and r.profileIconId < 0)
        or (r.summonerLevel is not None and r.summonerLevel < 0)
        or not r.refreshedAt
        or len(r.recentMatches) > 20
    ):
        return None
    sample = r.sample
    if (
        not 1 <= sample.recentMatches.requested <= 20
        or sample.recentMatches.returned != len(r.recentMatches)
        or sample.recentMatches.returned > sample.recentMatches.requested
        or sample.championAggregates.seasonStartUnixSeconds <= 0
        or sample.championAggregates.discoveredMatches > 2000
        or sample.championAggregates.analyzedMatches > sample.championAggregates.discoveredMatches
    ):
        return None
    if len({entry.queueType for entry in r.ranks}) != len(r.ranks) or any(
        not trim(entry.queueType) or min(entry.leaguePoints, entry.wins, entry.losses) < 0
        for entry in r.ranks
    ):
        return None
    if not valid_recent_matches(r.recentMatches, a.puuid):
        return None
    aggregates = r.championAggregates
    if (
        len({a.championName for a in aggregates}) != len(aggregates)
        or sum(a.games for a in aggregates) != sample.championAggregates.analyzedMatches
    ):
        return None
    for champion in aggregates:
        if (
            not trim(champion.championName)
            or champion.games == 0
            or champion.wins > champion.games
            or min(
                champion.kills,
                champion.deaths,
                champion.assists,
                champion.creepScore,
                champion.goldEarned,
                champion.tripleKills,
                champion.quadraKills,
                champion.pentaKills,
            )
            < 0
        ):
            return None
    if aggregates != sorted(aggregates, key=lambda a: (-a.games, -a.wins, a.championName)):
        return None
    return r
