from typing import Any

from monkeybot_api.errors import ApiError

ERRORS = {
    "series_source_not_ready": (
        409,
        "모집 마감 후 엔트리 편성이 완료된 상태에서만 시리즈를 시작할 수 있습니다.",
    ),
    "series_not_in_progress": (409, "진행 중인 시리즈에서만 이 작업을 수행할 수 있습니다."),
    "series_revision_conflict": (409, "시리즈 상태가 변경되었습니다. 최신 상태를 확인해 주세요."),
    "series_source_not_converted": (
        409,
        "원본 모집 상태가 변경되어 시리즈 상태를 변경할 수 없습니다.",
    ),
    "series_no_current_set": (409, "더 기록할 세트가 없습니다."),
    "series_set_not_current": (
        409,
        "현재 진행 중인 세트만 기록할 수 있습니다. 최신 상태를 확인해 주세요.",
    ),
    "series_game_side_required": (
        422,
        "픽밴을 입력하기 전에 현재 세트의 블루/레드 사이드를 먼저 선택해 주세요.",
    ),
    "series_game_side_mismatch": (
        409,
        "이미 선택된 세트 사이드와 다른 사이드로 결과를 기록할 수 없습니다.",
    ),
    "series_pickban_player_not_in_roster": (422, "고정된 시리즈 참가자만 픽을 지정할 수 있습니다."),
    "series_no_confirmed_games": (409, "되돌릴 확정 게임이 없습니다."),
    "series_early_completion_unavailable": (
        409,
        "BO3 또는 BO5에서 1경기 이상 확정한 뒤에만 조기 종료할 수 있습니다.",
    ),
    "series_game_reversal_not_safe": (
        409,
        "이후 MMR 변경이 있어 게임 결과를 안전하게 되돌릴 수 없습니다.",
    ),
    "series_result_unavailable": (409, "완료된 시리즈 결과만 확인할 수 있습니다."),
}


def error(code: str, details: dict[str, Any] | None = None) -> ApiError:
    status, message = ERRORS[code]
    return ApiError(status, code, message, details)
