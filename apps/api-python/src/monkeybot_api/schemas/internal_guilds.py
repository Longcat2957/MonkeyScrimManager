from dataclasses import dataclass
from datetime import datetime
from typing import Literal

from pydantic import BaseModel, ConfigDict, field_validator

from monkeybot_api.core.config import is_snowflake
from monkeybot_api.schemas.bot import Actor


class BotActor(Actor):
    # Serde requires the nullable key to be present on Discord command bodies.
    avatarHash: str | None


class SetupGuild(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid")
    actor: BotActor
    guildName: str
    guildIconHash: str | None
    operatorRoleId: str
    recruitmentChannelId: str
    entryChannelId: str | None
    resultChannelId: str | None
    logChannelId: str | None
    seasonName: str

    @field_validator(
        "operatorRoleId",
        "recruitmentChannelId",
        "entryChannelId",
        "resultChannelId",
        "logChannelId",
    )
    @classmethod
    def valid_snowflake(cls, value: str | None) -> str | None:
        if value is not None and not is_snowflake(value):
            raise ValueError("invalid Discord snowflake")
        return value


class RegisterGuildMember(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid")
    actor: BotActor
    gameName: str
    tagLine: str
    platform: str


class RegisteredMember(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid")
    id: str
    displayName: str
    status: Literal["ACTIVE"]


class RegisteredRiotAccount(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid")
    puuid: str
    gameName: str
    tagLine: str
    platform: str
    isMain: bool


class RegistrationResult(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid")
    member: RegisteredMember
    riotAccount: RegisteredRiotAccount
    registration: Literal["CREATED", "EXISTING"]
    accountLink: Literal["CREATED", "EXISTING"]


@dataclass(frozen=True)
class CanonicalRiotAccount:
    puuid: str
    platform: str
    routing_region: str
    game_name: str
    tag_line: str
    profile_icon_id: int | None
    last_synced_at: datetime
