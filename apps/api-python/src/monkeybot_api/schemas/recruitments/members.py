from typing import Literal

from pydantic import BaseModel, ConfigDict, field_validator

from monkeybot_api.schemas.bot import Actor
from monkeybot_api.schemas.common import user_id


class AddMember(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid")
    memberId: str

    @field_validator("memberId")
    @classmethod
    def validate_id(cls, value: str) -> str:
        return user_id(value)


class RemoveMember(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid")


class BotRemove(RemoveMember):
    actor: Actor


class BotAdd(BotRemove):
    member: Actor


class PreferredLanes(BaseModel):
    # The Axum outer request permits unknown fields; ActorRequest does not.
    model_config = ConfigDict(strict=True, extra="ignore")
    actor: Actor
    preferredLanes: list[Literal["TOP", "JUNGLE", "MID", "BOTTOM", "SUPPORT"]]
