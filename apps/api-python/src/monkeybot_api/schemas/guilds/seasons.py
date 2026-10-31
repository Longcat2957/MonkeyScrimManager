from typing import Annotated, Literal

from pydantic import AfterValidator, Field

from monkeybot_api.core.errors import ApiError
from monkeybot_api.schemas.notifications import Identifier
from monkeybot_api.schemas.recruitments.creation import canonical_start
from monkeybot_api.schemas.series import Strict


class RotateSeason(Strict):
    name: str
    expectedCurrentSeasonId: Identifier


def stored_timestamp(value: str) -> str:
    if not value or value != value.strip():
        raise ValueError("invalid stored timestamp")
    try:
        return canonical_start(value)[1]
    except ApiError:
        raise ValueError("invalid stored timestamp") from None


Timestamp = Annotated[str, AfterValidator(stored_timestamp)]

I64 = Annotated[int, Field(ge=-(2**63), le=2**63 - 1)]


class SeasonSummary(Strict):
    # Replay deserializes the stored Rust DTO; unknown fields are ignored by serde.
    model_config = {"strict": True, "extra": "ignore"}
    id: Identifier
    name: str
    status: Literal["ACTIVE", "CLOSED"]
    startedAt: Timestamp
    endedAt: Timestamp | None = None
    createdBy: str | None = None
    participantCount: I64
    generalSeriesCount: I64
    auctionTournamentCount: I64
    auctionTenCount: I64
    auctionTwentyCount: I64
    confirmedGameCount: I64
    lastGameAt: Timestamp | None = None


class SeasonsSnapshot(Strict):
    model_config = {"strict": True, "extra": "ignore"}
    current: SeasonSummary
    completed: list[SeasonSummary]
