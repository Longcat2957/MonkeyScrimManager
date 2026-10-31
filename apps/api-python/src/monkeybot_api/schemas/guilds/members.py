"""Guild operator member commands and stored replay contracts."""

from typing import Annotated, Literal

from pydantic import AfterValidator, BaseModel, ConfigDict

from monkeybot_api.schemas.common import user_id
from monkeybot_api.schemas.guilds.seasons import I64, Timestamp
from monkeybot_api.schemas.series import Strict

Lane = Literal["TOP", "JUNGLE", "MID", "BOTTOM", "SUPPORT"]
MemberStatus = Literal["ACTIVE", "INACTIVE"]
UserId = Annotated[str, AfterValidator(user_id)]


class AdjustMmr(Strict):
    # Field order and float serialization are part of the legacy idempotency hash.
    lane: Lane
    delta: float
    note: str
    expectedRevision: I64


class SetMemberStatus(Strict):
    status: MemberStatus
    expectedRevision: I64


class StoredModel(BaseModel):
    # Rust response DTOs ignore unknown fields and default missing Option fields to None.
    model_config = ConfigDict(strict=True, extra="ignore", allow_inf_nan=False)


class LaneMmr(StoredModel):
    TOP: float
    JUNGLE: float
    MID: float
    BOTTOM: float
    SUPPORT: float


class Member(StoredModel):
    id: UserId
    identityKind: Literal["DISCORD", "DUMMY"]
    discordName: str
    riotId: str | None = None
    role: Literal["MEMBER", "OPERATOR"]
    mainLane: Lane | None = None
    laneMmr: LaneMmr
    registered: bool
    status: MemberStatus
    lastActiveAt: Timestamp | None = None


class MembersSnapshot(StoredModel):
    revision: I64
    updatedAt: Timestamp
    updatedBy: UserId | None = None
    members: list[Member]
