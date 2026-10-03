from typing import Any

from monkeybot_api.core.errors import ApiError
from monkeybot_api.domain.auctions.recruitment_errors import BOT, SETUP

CREATE = {
    "guild_membership_required": (403, "해당 길드의 활성 멤버만 경매 내전을 시작할 수 있습니다."),
    "operator_required": (403, "경매 내전을 시작하려면 현재 길드 운영자 권한이 필요합니다."),
    "auction_recruitment_not_found": (404, "요청한 경매 모집을 찾을 수 없습니다."),
    "auction_setup_draft_revision_conflict": (
        409,
        "경매 팀장 설정이 변경되었습니다. 최신 상태를 확인해 주세요.",
    ),
    "auction_tournament_source_not_ready": (
        409,
        "모집 마감 후에만 경매 내전을 시작할 수 있습니다.",
    ),
    "auction_setup_incomplete": (
        422,
        "모든 팀의 팀장과 초기 포인트를 설정한 뒤 경매 내전을 시작할 수 있습니다.",
    ),
    "auction_setup_captain_not_joined": (422, "팀장은 현재 모집에 참가한 길드 멤버여야 합니다."),
    "auction_tournament_already_exists": (409, "이 모집으로 이미 진행 중인 경매 내전이 있습니다."),
    "auction_recruitment_not_full": (422, "정원을 모두 채운 뒤 경매 내전을 시작할 수 있습니다."),
    "idempotency_conflict": (409, "같은 Idempotency-Key가 다른 요청에 사용되었습니다."),
    "idempotency_in_progress": (409, "같은 경매 내전 시작 요청을 처리하고 있습니다."),
}

DRAFT = {
    "guild_membership_required": (
        403,
        "해당 길드의 활성 멤버만 경매 드래프트를 확인하거나 변경할 수 있습니다.",
    ),
    "operator_required": (403, "경매 드래프트를 변경하려면 현재 길드 운영자 권한이 필요합니다."),
    "auction_tournament_not_found": (404, "요청한 경매 내전을 찾을 수 없습니다."),
    "auction_tournament_revision_conflict": (
        409,
        "다른 운영자가 경매 드래프트를 변경했습니다. 최신 상태를 확인해 주세요.",
    ),
    "auction_tournament_draft_not_editable": (
        409,
        "현재 경매 내전 상태에서는 드래프트를 변경할 수 없습니다.",
    ),
    "auction_tournament_draft_locked": (
        409,
        "대진 또는 경기 기록이 있어 경매 드래프트를 변경할 수 없습니다.",
    ),
    "auction_draft_candidate_not_found": (422, "경매 후보 풀에서 해당 참가자를 찾을 수 없습니다."),
    "auction_draft_team_not_found": (422, "경매 내전에서 해당 팀을 찾을 수 없습니다."),
    "auction_draft_unsold_lot_not_found": (
        409,
        "선택한 유찰 후보의 처리 기록을 찾을 수 없습니다. 최신 상태를 확인해 주세요.",
    ),
    "auction_draft_current_target_required": (
        409,
        "현재 추첨된 후보가 있어야 이 작업을 수행할 수 있습니다.",
    ),
    "auction_draft_current_target_exists": (409, "현재 추첨된 후보를 먼저 처리해야 합니다."),
    "auction_draft_current_target_mismatch": (
        409,
        "선택한 후보가 현재 추첨 대상과 다릅니다. 최신 상태를 확인해 주세요.",
    ),
    "auction_draft_candidate_state_mismatch": (
        409,
        "선택한 후보의 현재 상태에서는 이 작업을 수행할 수 없습니다.",
    ),
    "auction_draft_no_queued_candidate": (409, "다음으로 추첨할 후보가 없습니다."),
    "auction_draft_team_full": (409, "선택한 팀은 이미 정원을 채웠습니다."),
    "invalid_auction_draft_bid_points": (
        422,
        "낙찰 포인트는 팀의 현재 포인트 범위에서 입력해야 합니다.",
    ),
    "auction_draft_no_revertable_action": (409, "되돌릴 수 있는 마지막 경매 작업이 없습니다."),
    "auction_draft_revert_unavailable": (
        409,
        "이후 변경이 있어 마지막 경매 작업을 안전하게 되돌릴 수 없습니다.",
    ),
    "idempotency_conflict": (
        409,
        "같은 Idempotency-Key가 다른 경매 드래프트 요청에 사용되었습니다.",
    ),
    "idempotency_in_progress": (409, "같은 경매 드래프트 요청을 처리하고 있습니다."),
}


def error(code: str, details: dict[str, Any] | None = None, *, create: bool = False) -> ApiError:
    mapping = {**BOT, **SETUP, **(CREATE if create else DRAFT)}
    status, message = mapping[code]
    return ApiError(status, code, message, details)
