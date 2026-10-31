"""Guild leaderboard snapshot; only played lanes appear in each player's stats."""

from typing import Literal

from pydantic import BaseModel, ConfigDict, Field

Lane = Literal["TOP", "JUNGLE", "MID", "BOTTOM", "SUPPORT"]


class SnapshotModel(BaseModel):
    model_config = ConfigDict(strict=True, allow_inf_nan=False)


class LeaderboardSeason(SnapshotModel):
    id: str
    name: str


class LeaderboardStat(SnapshotModel):
    mmr: float
    games: int = Field(ge=1)
    wins: int = Field(ge=0)
    championId: str | None


class LeaderboardPlayer(SnapshotModel):
    id: str
    name: str
    tag: str | None
    profileIconId: int | None = Field(ge=0)
    compositeMmr: float
    mainRole: Lane
    isMe: bool
    stats: dict[Lane, LeaderboardStat]


class LeaderboardSnapshot(SnapshotModel):
    season: LeaderboardSeason | None
    players: list[LeaderboardPlayer]
