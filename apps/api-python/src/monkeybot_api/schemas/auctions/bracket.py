from typing import Annotated, Any, Literal

from pydantic import Field, TypeAdapter

from monkeybot_api.core.errors import ApiError
from monkeybot_api.domain.auctions.recruitment_errors import BOT
from monkeybot_api.schemas.common import Revision
from monkeybot_api.schemas.notifications import Identifier

SLOTS = ("A_0", "A_1", "B_0", "B_1")


class Assign(Revision):
    kind: Literal["ASSIGN_TEAM"]
    slotKey: str
    teamId: Identifier


class Clear(Revision):
    kind: Literal["CLEAR_SLOT"]
    slotKey: str


class Lock(Revision):
    kind: Literal["LOCK_BRACKET", "UNLOCK_BRACKET"]


COMMAND: TypeAdapter[Any] = TypeAdapter(
    Annotated[Assign | Clear | Lock, Field(discriminator="kind")]
)

ERRORS = {
    "invalid_auction_tournament_bracket_command_request": (
        422,
        "경매 브래킷 명령 형식이 올바르지 않습니다.",
    ),
    "guild_membership_required": (
        403,
        "해당 길드의 활성 멤버만 경매 브래킷을 확인하거나 변경할 수 있습니다.",
    ),
    "operator_required": (403, "경매 브래킷을 변경하려면 현재 길드 운영자 권한이 필요합니다."),
    "auction_tournament_not_found": (404, "요청한 경매 내전을 찾을 수 없습니다."),
    "auction_tournament_revision_conflict": (
        409,
        "다른 운영자가 경매 브래킷을 변경했습니다. 최신 상태를 확인해 주세요.",
    ),
    "auction_tournament_bracket_unavailable": (
        409,
        "경매 드래프트가 완료된 후 브래킷을 확인할 수 있습니다.",
    ),
    "auction_tournament_bracket_not_editable": (
        409,
        "브래킷 설정 단계에서만 이 작업을 수행할 수 있습니다.",
    ),
    "auction_tournament_bracket_locked": (
        409,
        "브래킷이 확정되었거나 경기 기록이 있어 변경할 수 없습니다.",
    ),
    "auction_tournament_bracket_recovery_unavailable": (
        409,
        "게임 입력 또는 결과 기록이 있어 브래킷 확정을 해제할 수 없습니다.",
    ),
    "auction_tournament_bracket_slot_unavailable": (
        422,
        "현재 경매 형식에서는 선택한 브래킷 슬롯을 사용할 수 없습니다.",
    ),
    "auction_tournament_bracket_team_not_found": (
        422,
        "경매 내전에서 선택한 팀을 찾을 수 없습니다.",
    ),
    "auction_tournament_bracket_roster_incomplete": (
        409,
        "모든 팀의 활성 roster가 5명이어야 브래킷을 확정할 수 있습니다.",
    ),
    "auction_tournament_bracket_slots_incomplete": (
        409,
        "경매 형식에 필요한 모든 브래킷 슬롯에 팀을 배치해야 합니다.",
    ),
    "idempotency_conflict": (409, "같은 Idempotency-Key가 다른 경매 브래킷 요청에 사용되었습니다."),
    "idempotency_in_progress": (409, "같은 경매 브래킷 요청을 처리하고 있습니다."),
}


def error(code: str, details: dict[str, Any] | None = None) -> ApiError:
    status, message = {**BOT, **ERRORS}[code]
    return ApiError(status, code, message, details)


def invalid() -> ApiError:
    return error("invalid_auction_tournament_bracket_command_request")
