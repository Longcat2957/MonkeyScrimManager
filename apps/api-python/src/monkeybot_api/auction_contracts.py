from typing import Any, Literal

from pydantic import BaseModel, ConfigDict, Field

from monkeybot_api.auction_errors import error
from monkeybot_api.errors import ApiError
from monkeybot_api.recruitment_create import Actor
from monkeybot_api.recruitment_lifecycle import Revision
from monkeybot_api.series_contracts import Strict, UserId

Lane = Literal["TOP", "JUNGLE", "MID", "BOTTOM", "SUPPORT"]
Format = Literal["TEN_PERSON", "TWENTY_PERSON"]


class Create(Strict):
    format: Format
    semifinalBestOf: int | None = Field(default=None, ge=-32768, le=32767)
    finalBestOf: int | None = Field(default=None, ge=-32768, le=32767)
    hardFearless: bool | None = None
    expectedStartAt: Any = None


class BotCreate(Create):
    actor: Actor


class Add(Strict):
    memberId: UserId
    declaredLane: Lane


class BotAdd(Strict):
    actor: Actor
    member: Actor | None = None
    declaredLane: Lane | None = None


class Team(BaseModel):
    model_config = ConfigDict(strict=True, extra="ignore")
    teamIndex: int = Field(ge=-32768, le=32767)
    teamName: str
    captainUserId: UserId
    initialPoints: int = Field(ge=-(2**31), le=2**31 - 1)


class Setup(Revision):
    teams: list[Team]


def rules(body: Create) -> dict[str, Any]:
    return {
        "hardFearless": True if body.hardFearless is None else body.hardFearless,
        "semifinalBestOf": (3 if body.semifinalBestOf is None else body.semifinalBestOf)
        if body.format == "TWENTY_PERSON"
        else body.semifinalBestOf,
        "finalBestOf": 3 if body.finalBestOf is None else body.finalBestOf,
    }


def validate_rules(format: str, values: dict[str, Any]) -> None:
    for stage in ("final", "semifinal"):
        value = values[stage + "BestOf"]
        if stage == "semifinal" and format == "TEN_PERSON":
            if value is not None:
                raise error("auction_semifinal_best_of_unsupported")
        elif value not in (1, 3, 5):
            raise error("invalid_auction_recruitment_best_of", {"stage": stage, "bestOf": value})


def invalid(action: str) -> ApiError:
    kind, message = {
        "create": ("auction_recruitment_create", "경매 모집 생성 요청 형식이 올바르지 않습니다."),
        "join": (
            "auction_recruitment_member",
            "경매 모집 참가자 변경 요청 형식이 올바르지 않습니다.",
        ),
        "leave": (
            "auction_recruitment_member",
            "경매 모집 참가자 변경 요청 형식이 올바르지 않습니다.",
        ),
        "reopen": ("auction_recruitment_reopen", "경매 모집 재개 요청 형식이 올바르지 않습니다."),
        "cancel": (
            "auction_recruitment_lifecycle",
            "경매 모집 취소 또는 복구 요청 형식이 올바르지 않습니다.",
        ),
        "restore": (
            "auction_recruitment_lifecycle",
            "경매 모집 취소 또는 복구 요청 형식이 올바르지 않습니다.",
        ),
        "setup": ("auction_setup_draft", "경매 팀장 설정 요청 형식이 올바르지 않습니다."),
    }[action]
    return ApiError(422, "invalid_" + kind + "_request", message)
