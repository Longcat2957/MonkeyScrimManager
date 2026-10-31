from typing import Annotated, Any, Literal

from pydantic import Field, TypeAdapter

from monkeybot_api.core.errors import ApiError
from monkeybot_api.schemas.common import Revision
from monkeybot_api.schemas.notifications import Identifier
from monkeybot_api.schemas.series import UserId


class Create(Revision):
    sourceRecruitmentId: Identifier


class Basic(Revision):
    kind: Literal["DRAW_NEXT", "RESHUFFLE_CANDIDATES", "RESET_TO_INITIAL", "REVERT_LAST"]


class Target(Revision):
    kind: Literal["MARK_UNSOLD", "RESTORE_UNSOLD"]
    memberId: UserId


class Assign(Revision):
    kind: Literal["ASSIGN_CURRENT", "ASSIGN_UNSOLD"]
    memberId: UserId
    teamId: Identifier


class Award(Revision):
    kind: Literal["AWARD_BID"]
    memberId: UserId
    teamId: Identifier
    points: int = Field(ge=-(2**31), le=2**31 - 1)


COMMAND: TypeAdapter[Any] = TypeAdapter(
    Annotated[Basic | Target | Assign | Award, Field(discriminator="kind")]
)


def invalid(create: bool = False) -> ApiError:
    return ApiError(
        422,
        "invalid_auction_tournament_" + ("create" if create else "draft_command") + "_request",
        "경매 내전 시작 요청 형식이 올바르지 않습니다."
        if create
        else "경매 드래프트 명령 형식이 올바르지 않습니다.",
    )
