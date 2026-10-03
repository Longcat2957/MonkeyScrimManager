"""Frozen Axum contracts for tournament and recruitment force cancellation."""

from typing import Any

from monkeybot_api.core.errors import ApiError
from monkeybot_api.domain.auctions.recruitment_errors import BOT
from monkeybot_api.schemas.bot import Actor
from monkeybot_api.schemas.common import Revision


class Cancel(Revision):
    reason: str


class BotCancel(Cancel):
    actor: Actor


ERRORS = {
    "invalid_auction_tournament_cancel_request": (
        422,
        "경매 내전 조기 종료 요청 형식이 올바르지 않습니다.",
    ),
    "auction_tournament_not_found": (404, "요청한 경매 내전을 찾을 수 없습니다."),
    "auction_tournament_revision_conflict": (
        409,
        "다른 운영자가 경매 내전을 변경했습니다. 최신 상태를 확인해 주세요.",
    ),
    "auction_tournament_cancel_unavailable": (
        409,
        "완료되었거나 이미 취소된 경매 내전은 조기 종료할 수 없습니다.",
    ),
    "auction_tournament_cancel_mmr_history_advanced": (
        409,
        "이후 MMR 변경이 있어 경매 내전 전체를 안전하게 취소할 수 없습니다.",
    ),
    "invalid_auction_recruitment_state": (
        409,
        "현재 경매 모집 상태에서는 이 작업을 수행할 수 없습니다.",
    ),
}
WEB = {
    "guild_membership_required": (
        403,
        "해당 길드의 활성 멤버만 경매 내전을 조기 종료할 수 있습니다.",
    ),
    "operator_required": (403, "경매 내전을 조기 종료하려면 현재 길드 운영자 권한이 필요합니다."),
    "idempotency_conflict": (
        409,
        "같은 Idempotency-Key가 다른 경매 내전 조기 종료 요청에 사용되었습니다.",
    ),
    "idempotency_in_progress": (409, "같은 경매 내전 조기 종료 요청을 처리하고 있습니다."),
}


def error(code: str, details: dict[str, Any] | None = None, *, bot: bool = False) -> ApiError:
    mapping = {**BOT, **ERRORS, **({} if bot else WEB)}
    status, message = mapping[code]
    if bot and code == "auction_tournament_not_found":
        code = "not_found"
    elif bot and code == "auction_tournament_revision_conflict":
        message = "다른 운영자가 경매 드래프트를 변경했습니다. 최신 상태를 확인해 주세요."
    return ApiError(status, code, message, details)
