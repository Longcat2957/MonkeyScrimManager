from typing import Any, Literal

from monkeybot_api.core.errors import ApiError
from monkeybot_api.domain.auctions.recruitment_errors import BOT
from monkeybot_api.schemas.common import Revision
from monkeybot_api.schemas.notifications import Identifier


class Assignment(Revision):
    teamId: Identifier
    lane: Literal["TOP", "JUNGLE", "MID", "BOTTOM", "SUPPORT"]
    teamMemberId: Identifier | None = None


ERRORS = {
    "invalid_auction_match_lineup_request": (
        422,
        "경매 경기 라인업 요청 형식이 올바르지 않습니다.",
    ),
    "auction_tournament_not_found": (404, "요청한 경매 내전을 찾을 수 없습니다."),
    "guild_membership_required": (
        403,
        "해당 길드의 활성 멤버만 경매 경기 라인업을 확인하거나 변경할 수 있습니다.",
    ),
    "operator_required": (403, "경매 경기 라인업을 변경하려면 현재 길드 운영자 권한이 필요합니다."),
    "auction_match_not_found": (404, "요청한 경매 경기를 찾을 수 없습니다."),
    "auction_match_teams_pending": (409, "양 팀이 확정된 뒤에만 이 경기를 편성할 수 있습니다."),
    "auction_match_tournament_not_ready": (
        409,
        "대진이 확정된 진행 중 경매 내전에서만 라인업을 변경할 수 있습니다.",
    ),
    "auction_match_revision_conflict": (
        409,
        "다른 운영자가 경매 경기 라인업을 변경했습니다. 최신 상태를 확인해 주세요.",
    ),
    "auction_match_lineup_not_editable": (
        409,
        "라인업 편성 대기 상태에서만 이 작업을 수행할 수 있습니다.",
    ),
    "auction_match_lineup_locked": (409, "이 경기는 이미 라인업이 확정되어 변경할 수 없습니다."),
    "auction_match_lineup_unlock_unavailable": (
        409,
        "게임 결과를 입력하기 전 확정된 라인업만 다시 편성할 수 있습니다.",
    ),
    "auction_match_lineup_result_artifacts_present": (
        409,
        "게임 입력 또는 결과 기록이 있어 라인업 확정을 해제할 수 없습니다.",
    ),
    "auction_match_lineup_team_not_found": (422, "선택한 팀은 이 경기에 배정되어 있지 않습니다."),
    "auction_match_lineup_member_not_found": (
        422,
        "선택한 선수는 해당 팀의 활성 roster에 없습니다.",
    ),
    "auction_match_lineup_player_assigned": (
        409,
        "같은 선수는 경매 경기 라인업의 한 라인에만 배정할 수 있습니다.",
    ),
    "auction_match_lineup_incomplete": (
        409,
        "양 팀의 TOP, JUNGLE, MID, BOTTOM, SUPPORT를 모두 배정해야 라인업을 확정할 수 있습니다.",
    ),
    "idempotency_conflict": (
        409,
        "같은 Idempotency-Key가 다른 경매 경기 라인업 요청에 사용되었습니다.",
    ),
    "idempotency_in_progress": (409, "같은 경매 경기 라인업 요청을 처리하고 있습니다."),
}


def error(code: str, details: dict[str, Any] | None = None) -> ApiError:
    status, message = {**BOT, **ERRORS}[code]
    return ApiError(status, code, message, details)
