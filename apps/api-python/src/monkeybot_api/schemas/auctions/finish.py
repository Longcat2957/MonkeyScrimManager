"""Browser-only manual completion contract, preserving the Axum error envelope."""

from typing import Any

from monkeybot_api.core.errors import ApiError
from monkeybot_api.domain.auctions.recruitment_errors import BOT
from monkeybot_api.schemas.common import Revision


class Finish(Revision):
    reason: str


ERRORS = {
    **BOT,
    "invalid_auction_tournament_finish_request": (
        422,
        "경매 내전 마무리 요청 형식이 올바르지 않습니다.",
    ),
    "guild_membership_required": (
        403,
        "해당 길드의 활성 멤버만 경매 내전을 마무리할 수 있습니다.",
    ),
    "operator_required": (403, "경매 내전을 마무리하려면 현재 길드 운영자 권한이 필요합니다."),
    "auction_tournament_not_found": (404, "요청한 경매 내전을 찾을 수 없습니다."),
    "auction_tournament_revision_conflict": (
        409,
        "다른 운영자가 경매 내전을 변경했습니다. 최신 상태를 확인해 주세요.",
    ),
    "auction_tournament_finish_unavailable": (
        409,
        "완료되었거나 취소된 경매 내전은 마무리할 수 없습니다.",
    ),
    "idempotency_conflict": (
        409,
        "같은 Idempotency-Key가 다른 경매 내전 마무리 요청에 사용되었습니다.",
    ),
    "idempotency_in_progress": (409, "같은 경매 내전 마무리 요청을 처리하고 있습니다."),
}


def error(code: str, details: dict[str, Any] | None = None) -> ApiError:
    status, message = ERRORS[code]
    return ApiError(status, code, message, details)
