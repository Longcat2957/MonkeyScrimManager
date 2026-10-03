"""Request and replay contracts for persistent development dummy users."""

from typing import Literal

from pydantic import BaseModel, ConfigDict, Field

Lane = Literal["TOP", "JUNGLE", "MID", "BOTTOM", "SUPPORT"]
LANES: tuple[Lane, ...] = ("TOP", "JUNGLE", "MID", "BOTTOM", "SUPPORT")


class Strict(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid")


class Privacy(Strict):
    publicProfileEnabled: bool = False
    showRiotIds: bool = False
    allowExternalRecordSearch: bool = False


class LaneMmr(Strict):
    top: float
    jungle: float
    mid: float
    bottom: float
    support: float


class CreateMembership(Strict):
    guildId: str
    displayName: str
    status: Literal["ACTIVE", "INACTIVE"]
    mainLane: Lane | None = None
    laneMmr: LaneMmr | None = None


class CreateRiotAccount(Strict):
    platform: str
    routingRegion: str
    gameName: str
    tagLine: str
    profileIconId: int | None = None
    mainPosition: Literal["TOP", "JUNGLE", "MID", "BOTTOM", "SUPPORT", "FLEX"] | None = None


class CreateUser(Strict):
    stableKey: str
    displayName: str
    avatarSeed: str | None = None
    notes: str | None = None
    privacy: Privacy = Field(default_factory=Privacy)
    guildMemberships: list[CreateMembership] = Field(default_factory=list)
    riotAccount: CreateRiotAccount | None = None


class CreateUsersBulk(Strict):
    users: list[CreateUser]


class ReplayPrivacy(BaseModel):
    model_config = ConfigDict(strict=True, extra="ignore")

    publicProfileEnabled: bool
    showRiotIds: bool
    allowExternalRecordSearch: bool


class ReplayUser(BaseModel):
    model_config = ConfigDict(strict=True, extra="ignore")

    userId: str
    identityKind: Literal["DUMMY"]
    stableKey: str
    displayName: str
    avatarSeed: str | None
    notes: str | None
    state: Literal["ACTIVE", "ARCHIVED"]
    revision: int
    privacy: ReplayPrivacy
    riotAccount: dict[str, object] | None
    championPreferences: dict[str, list[str]]
    guildMemberships: list[dict[str, object]]
    groupIds: list[str]
    createdAt: str
    updatedAt: str
