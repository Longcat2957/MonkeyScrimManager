"""Commands for development-only dummy recruitment participants."""

from typing import Annotated, Literal

from pydantic import AfterValidator, BaseModel, ConfigDict, Field

from monkeybot_api.schemas.common import user_id

UserId = Annotated[str, AfterValidator(user_id)]
Action = Literal["ADD", "REMOVE"]


class PreviewParticipants(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)

    action: Action
    userIds: list[UserId]


class ApplyParticipants(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)

    userIds: list[UserId]
    expectedRevision: int = Field(ge=-(2**63), le=2**63 - 1)
