import logging
from typing import Any, Literal

from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.time import timestamp
from monkeybot_api.db.database import Database
from monkeybot_api.repositories.guild_access import require_guild_access
from monkeybot_api.repositories.guilds.operations import fetch_operations
from monkeybot_api.schemas.guilds.operations import Operation, OperationsData

logger = logging.getLogger(__name__)


async def list_operations(
    database: Database, guild: str, actor: str, request_id: str
) -> dict[str, Any]:
    async with database.connection() as connection:
        await require_guild_access(connection, actor, guild)
        rows = await fetch_operations(connection, guild, actor)
    try:
        return OperationsData(
            operations=[project_operation(row, guild) for row in rows]
        ).model_dump()
    except ValueError:
        logger.exception("operation_projection_failed", extra={"request_id": request_id})
        raise ApiError.from_status(500) from None


def project_operation(row: dict[str, Any], guild: str) -> Operation:
    entity = row["entity_id"]
    first, second, third = row["first_count"], row["second_count"], row["third_count"]
    base = f"/g/{guild}"
    kind: Literal["ENTRY", "PICK_BAN", "AUCTION"]
    priority: Literal["urgent", "normal"]
    match row["source_kind"]:
        case "ENTRY":
            operation_id, kind, priority = f"entry:{entity}", "ENTRY", "urgent"
            status = "편성 필요"
            detail = f"{first}/{third}명 · {max(first - second, 0)}명 미배정"
            href = f"{base}/recruitments/{entity}/entry"
        case "PICK_BAN":
            operation_id, kind, priority = f"series:{entity}", "PICK_BAN", "urgent"
            status = "픽밴/결과 입력"
            detail = (
                f"{first}세트 완료 · {first + 1}세트 입력 대기"
                if first + 1 <= second
                else f"{first}세트 완료 · 완료 처리 대기"
            )
            href = f"{base}/series/{entity}/pick-ban"
        case "AUCTION_RECRUITMENT":
            operation_id, kind, priority = f"auction-recruitment:{entity}", "AUCTION", "normal"
            status = "팀장 설정"
            detail = f"{first}/{third}명 · {second}팀"
            href = f"{base}/auction/recruitments/{entity}/draft"
        case "AUCTION_TOURNAMENT":
            operation_id, kind, priority = f"auction-tournament:{entity}", "AUCTION", "normal"
            tournament = f"{base}/auction/tournaments/{entity}"
            match row["source_status"]:
                case "BIDDING":
                    status, detail, href = (
                        "입찰 진행",
                        f"{second}팀 · {first}명 편성",
                        f"{tournament}/draft",
                    )
                case "BRACKET_SETUP":
                    status, detail, href = (
                        "대진 설정",
                        f"{second}팀 · 대진 확정 대기",
                        f"{tournament}/bracket",
                    )
                case "IN_GAME":
                    detail = f"{third}경기 진행 또는 대기"
                    if row["auxiliary_status"] in {"READY", "IN_PROGRESS"}:
                        if row["auxiliary_id"] is None:
                            raise ValueError("auction operation has no next match id")
                        status, href = "결과 입력", f"{base}/auction/matches/{row['auxiliary_id']}"
                    else:
                        status, href = "대진 진행", f"{tournament}/bracket"
                case _:
                    raise ValueError("auction operation status is not active")
        case _:
            raise ValueError("operation kind is not canonical")
    return Operation(
        id=operation_id,
        kind=kind,
        title=row["title"],
        status=status,
        detail=detail,
        updatedAt=timestamp(row["updated_at"]),
        href=href,
        priority=priority,
    )
