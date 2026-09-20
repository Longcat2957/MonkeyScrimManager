from typing import Any

from monkeybot_api.errors import ApiError

BOT = {
    "invalid_auction_recruitment_best_of": (
        422,
        "경매 모집 시리즈 형식은 BO1, BO3, BO5만 선택할 수 있습니다.",
    ),
    "auction_semifinal_best_of_unsupported": (
        422,
        "10인 경매 모집에는 4강 시리즈 형식을 설정할 수 없습니다.",
    ),
    "auction_semifinal_best_of_required": (422, "20인 경매 모집에는 4강 시리즈 형식이 필요합니다."),
    "not_found": (404, "요청한 대상을 찾을 수 없습니다."),
    "guild_inactive": (409, "현재 길드에서는 command를 실행할 수 없습니다."),
    "guild_setup_required": (409, "길드 초기 설정을 먼저 완료해야 합니다."),
    "recruitment_channel_not_configured": (
        409,
        "Discord 모집 채널을 먼저 설정해야 내전을 만들 수 있습니다.",
    ),
    "operator_required": (403, "길드 운영자 권한이 필요합니다."),
    "member_inactive": (403, "비활성화된 길드 멤버는 이 작업을 수행할 수 없습니다."),
    "guild_member_not_found": (404, "길드에 소속된 사용자를 찾을 수 없습니다."),
    "guild_member_inactive": (409, "비활성화된 길드 멤버는 모집에 추가할 수 없습니다."),
    "guild_membership_required": (403, "해당 길드의 활성 멤버만 이 작업을 수행할 수 있습니다."),
    "invalid_recruitment_state": (409, "현재 모집 상태에서는 이 작업을 수행할 수 없습니다."),
    "participant_confirmation_dm_unavailable": (
        409,
        "마감된 모집에서만 참가 확정 DM을 발송할 수 있습니다.",
    ),
    "recruitment_revision_conflict": (
        409,
        "모집 상태가 변경되었습니다. 최신 상태를 확인해 주세요.",
    ),
    "recruitment_full": (422, "모집 정원이 가득 찼습니다."),
    "auction_declared_lane_required": (
        422,
        "주 라인을 먼저 설정하거나 모집 메시지에서 참가 라인을 선택해 주세요.",
    ),
    "already_participant": (422, "이미 참가 중인 사용자입니다."),
    "not_participant": (422, "현재 모집 참가자가 아닙니다."),
    "idempotency_conflict": (409, "같은 interaction ID가 다른 요청에 사용되었습니다."),
    "idempotency_in_progress": (409, "같은 interaction 요청을 처리하고 있습니다."),
}

WEB = {
    "invalid_auction_recruitment_request": (422, "경매 모집 요청 형식이 올바르지 않습니다."),
    "guild_membership_required": (
        403,
        "해당 길드의 활성 멤버만 경매 모집을 만들거나 변경할 수 있습니다.",
    ),
    "operator_required": (403, "경매 모집을 변경하려면 현재 길드 운영자 권한이 필요합니다."),
    "no_active_season": (409, "길드에 진행 중인 시즌이 없어 경매 모집을 만들 수 없습니다."),
    "auction_recruitment_not_found": (404, "요청한 경매 모집을 찾을 수 없습니다."),
    "auction_recruitment_not_open": (409, "모집 중인 경매에서만 이 작업을 수행할 수 있습니다."),
    "auction_recruitment_restore_unavailable": (409, "취소된 경매 모집만 복구할 수 있습니다."),
    "auction_recruitment_reopen_unavailable": (
        409,
        "팀장 선택 단계의 경매 모집만 모집 단계로 되돌릴 수 있습니다.",
    ),
    "recruitment_revision_conflict": (
        409,
        "경매 모집 상태가 변경되었습니다. 최신 상태를 확인해 주세요.",
    ),
    "auction_recruitment_not_full": (422, "정원을 채운 뒤 모집을 마감할 수 있습니다."),
    "idempotency_conflict": (409, "같은 Idempotency-Key가 다른 요청에 사용되었습니다."),
    "idempotency_in_progress": (409, "같은 경매 모집 요청을 처리하고 있습니다."),
}

SETUP = {
    "guild_membership_required": (
        403,
        "해당 길드의 활성 멤버만 경매 팀장 설정을 변경할 수 있습니다.",
    ),
    "operator_required": (403, "경매 팀장 설정을 변경하려면 현재 길드 운영자 권한이 필요합니다."),
    "auction_setup_draft_not_found": (404, "요청한 경매 팀장 설정을 찾을 수 없습니다."),
    "auction_setup_draft_revision_conflict": (
        409,
        "경매 팀장 설정이 변경되었습니다. 최신 상태를 확인해 주세요.",
    ),
    "auction_recruitment_not_full": (422, "정원을 모두 채운 뒤 팀장 설정을 저장할 수 있습니다."),
    "invalid_auction_setup_team_count": (422, "경매 형식에 맞는 팀 수를 입력해야 합니다."),
    "invalid_auction_setup_team_index": (422, "팀 순서는 1번부터 연속해서 입력해야 합니다."),
    "invalid_auction_setup_team_name": (
        422,
        "팀 이름은 비어 있지 않은 64바이트 이하의 값이어야 합니다.",
    ),
    "invalid_auction_setup_initial_points": (422, "초기 포인트는 1 이상이어야 합니다."),
    "duplicate_auction_setup_captain": (
        422,
        "같은 참가자를 여러 팀의 팀장으로 지정할 수 없습니다.",
    ),
    "auction_setup_captain_not_joined": (422, "팀장은 현재 모집에 참가한 길드 멤버여야 합니다."),
    "idempotency_conflict": (409, "같은 Idempotency-Key가 다른 요청에 사용되었습니다."),
    "idempotency_in_progress": (409, "같은 경매 팀장 설정 요청을 처리하고 있습니다."),
}

DM = {
    "guild_membership_required": (
        403,
        "해당 길드의 활성 멤버만 참가 확정 DM을 발송할 수 있습니다.",
    ),
    "operator_required": (403, "참가 확정 DM 발송에는 현재 길드 운영자 권한이 필요합니다."),
    "recruitment_not_found": (404, "요청한 모집을 찾을 수 없습니다."),
    "participant_confirmation_dm_unavailable": (
        409,
        "마감된 모집에서만 참가 확정 DM을 발송할 수 있습니다.",
    ),
    "idempotency_conflict": (409, "같은 Idempotency-Key가 다른 요청에 사용되었습니다."),
    "idempotency_in_progress": (409, "같은 참가 확정 DM 요청을 처리하고 있습니다."),
}


def error(code: str, details: dict[str, Any] | None = None, *, mode: str = "web") -> ApiError:
    mapping = {**BOT, **({"web": WEB, "setup": SETUP, "dm": DM}.get(mode, {}))}
    status, message = mapping[code]
    return ApiError(status, code, message, details)
