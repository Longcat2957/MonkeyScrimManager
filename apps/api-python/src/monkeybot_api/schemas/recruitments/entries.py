from typing import Literal

from pydantic import BaseModel, ConfigDict, field_validator

from monkeybot_api.schemas.common import user_id


class MoveParticipant(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid")
    type: Literal["MOVE_PARTICIPANT"]
    participantId: str
    slot: str | None = None

    @field_validator("participantId")
    @classmethod
    def validate_id(cls, value: str) -> str:
        return user_id(value)
