from typing import Any

from monkeybot_api.core.errors import ApiError
from monkeybot_api.schemas.series import Bans, UserId
from monkeybot_api.schemas.series import Submit as SeriesSubmit
from monkeybot_api.schemas.series import validate as validate_series


class Submit(SeriesSubmit):
    picks: dict[UserId, str | None]
    bans: Bans


MESSAGES = {
    "invalid_auction_match_game_request": "경매 경기 결과 입력 요청 형식이 올바르지 않습니다.",
    "guild_membership_required": (
        "해당 길드의 활성 멤버만 경매 경기 결과를 확인하거나 변경할 수 있습니다."
    ),
    "operator_required": "경매 경기 결과를 변경하려면 현재 길드 운영자 권한이 필요합니다.",
    "auction_match_not_found": "요청한 경매 경기를 찾을 수 없습니다.",
    "auction_match_teams_pending": "양 팀이 확정된 뒤에만 이 경기의 결과를 입력할 수 있습니다.",
    "auction_match_tournament_not_ready": (
        "대진이 확정된 진행 중 경매 내전에서만 경기 결과를 변경할 수 있습니다."
    ),
    "auction_final_bo1_completion_unavailable": (
        "BO3 또는 BO5 경매 결승의 첫 세트가 1:0으로 확정된 경우에만 BO1로 조기 종료할 수 있습니다."
    ),
    "auction_match_semifinal_lineups_incomplete": (
        "20인 경매 내전은 두 준결승의 라인업을 모두 확정한 뒤 세트 결과를 입력할 수 있습니다."
    ),
    "auction_match_revision_conflict": (
        "다른 운영자가 경매 경기 결과를 변경했습니다. 최신 상태를 확인해 주세요."
    ),
    "auction_match_result_not_editable": (
        "현재 경기 상태에서는 세트 결과를 입력하거나 되돌릴 수 없습니다."
    ),
    "auction_match_set_not_current": (
        "현재 진행 중인 세트만 기록할 수 있습니다. 최신 상태를 확인해 주세요."
    ),
    "duplicate_auction_match_game_champion": (
        "같은 세트에서 픽과 밴은 같은 챔피언을 중복할 수 없습니다."
    ),
    "auction_match_hard_fearless_champion_used": (
        "하드 피어리스에서는 이 경기의 이전 세트에서 픽한 챔피언을 다시 픽할 수 없습니다."
    ),
    "auction_match_no_confirmed_games": "되돌릴 확정 세트가 없습니다.",
    "auction_match_game_reversal_not_safe": (
        "이후 MMR 변경이 있어 이 세트 결과를 안전하게 되돌릴 수 없습니다."
    ),
    "auction_match_downstream_work_exists": (
        "결승의 라인업 또는 게임 기록이 있어 준결승 결과를 되돌릴 수 없습니다."
    ),
    "auction_tournament_result_delivery_in_progress": (
        "결과 메시지를 Discord에 전송 중이라 지금은 결과를 "
        "되돌릴 수 없습니다. 잠시 후 다시 시도해 주세요."
    ),
    "auction_tournament_result_delivery_unresolved": (
        "Discord 결과 메시지의 전송 여부를 확인할 수 없어 결과를 "
        "되돌릴 수 없습니다. 운영 로그에서 전송 상태를 확인해 주세요."
    ),
    "idempotency_conflict": "같은 Idempotency-Key가 다른 경매 경기 결과 요청에 사용되었습니다.",
    "idempotency_in_progress": "같은 경매 경기 결과 요청을 처리하고 있습니다.",
}


def error(code: str, details: dict[str, Any] | None = None) -> ApiError:
    status = 409
    if code in {"guild_membership_required", "operator_required"}:
        status = 403
    elif code == "auction_match_not_found":
        status = 404
    elif code in {
        "invalid_auction_match_game_request",
        "duplicate_auction_match_game_champion",
        "auction_match_hard_fearless_champion_used",
    }:
        status = 422
    return ApiError(status, code, MESSAGES[code], details)


def validate(body: dict[str, Any], action: str) -> None:
    try:
        validate_series(body, action)
    except ApiError:
        raise error("invalid_auction_match_game_request") from None
