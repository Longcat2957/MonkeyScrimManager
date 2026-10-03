"""Commands for dummy-user guild, champion, and synthetic Riot links."""

from typing import Literal

from pydantic import Field

from monkeybot_api.schemas.dev.users import Lane, LaneMmr, Strict


class UpsertMembership(Strict):
    displayName: str
    status: Literal["ACTIVE", "INACTIVE"]
    mainLane: Lane | None = None
    laneMmr: LaneMmr | None = None


class ReplaceChampionPreferences(Strict):
    championIds: list[str]


class UpsertRiotAccount(Strict):
    platform: str
    routingRegion: str
    gameName: str
    tagLine: str
    profileIconId: int | None = Field(default=None, ge=-(2**31), le=2**31 - 1)
    mainPosition: str | None = None
