from datetime import timedelta, timezone
from typing import Any

from sqlalchemy.ext.asyncio import AsyncConnection

import monkeybot_api.repositories.recruitments.conversion as repository
from monkeybot_api.core.errors import ApiError
from monkeybot_api.db.queries import params
from monkeybot_api.repositories.auctions.recruitments import snapshot
from monkeybot_api.repositories.effects import audit, invalidate, message_update, notification
from monkeybot_api.repositories.recruitments.snapshots import recruitment_snapshot
from monkeybot_api.repositories.series.mmr import lock_mmr


def error(code: str, details: Any = None) -> ApiError:
    status, message = {
        "invalid_entry_auction_conversion_request": (
            422,
            "엔트리 경매 전환 요청 형식이 올바르지 않습니다.",
        ),
        "guild_membership_required": (
            403,
            "해당 길드의 활성 멤버만 엔트리를 경매 모집으로 전환할 수 있습니다.",
        ),
        "operator_required": (
            403,
            "엔트리를 경매 모집으로 전환하려면 현재 길드 운영자 권한이 필요합니다.",
        ),
        "recruitment_channel_not_configured": (
            409,
            "Discord 모집 채널을 먼저 설정해야 경매 모집으로 전환할 수 있습니다.",
        ),
        "idempotency_in_progress": (409, "같은 엔트리 경매 전환 요청을 처리하고 있습니다."),
        "recruitment_revision_conflict": (
            409,
            "모집 상태가 변경되었습니다. 최신 상태를 확인해 주세요.",
        ),
        "entry_draft_auction_source_not_ready": (
            409,
            "마감된 일반 엔트리 모집만 경매 모집으로 전환할 수 있습니다.",
        ),
        "entry_draft_already_converted_to_series": (
            409,
            "이미 일반 시리즈로 전환된 모집은 경매 모집으로 전환할 수 없습니다.",
        ),
        "entry_draft_incomplete": (422, "참가자 10명을 채운 뒤 경매 모집으로 전환할 수 있습니다."),
    }[code]
    return ApiError(status, code, message, details)


async def convert(c: AsyncConnection, p: dict[str, Any], source: Any) -> dict[str, Any]:
    if source["revision"] != p["expected"]:
        raise error(
            "recruitment_revision_conflict",
            {"expectedRevision": p["expected"], "actualRevision": source["revision"]},
        )
    if source["status"] != "CLOSED":
        raise error("entry_draft_auction_source_not_ready", {"currentStatus": source["status"]})
    draft = await repository.lock_entry_revision(c, p)
    if draft is None:
        raise ApiError.from_status(500)
    if await repository.series_exists(c, p):
        raise error("entry_draft_already_converted_to_series")
    roster = await repository.lock_roster(c, p)
    assigned = [dict(r) for r in roster if r["lane"] is not None]
    if source["target_count"] != 10 or len(roster) != 10:
        raise error(
            "entry_draft_incomplete",
            {
                "joinedCount": len(roster),
                "assignedCount": len(assigned),
                "targetCount": source["target_count"],
            },
        )
    mmr = await lock_mmr(c, {**p, "season": source["season_id"]}, assigned)
    number = await repository.next_auction_number(c, params(p["guild"]))
    if number is None:
        raise ApiError.from_status(500)
    start = p["start"].astimezone(timezone(timedelta(hours=9))).strftime("%Y-%m-%d %H:%M")
    values = {
        **p,
        "season": source["season_id"],
        "number": number,
        "title": f"10인 경매 내전 모집 #{number} · 예상 시작 {start} (KST)",
        "best_of": source["best_of"],
        "fearless": source["hard_fearless"],
    }
    await repository.insert_auction(c, values)
    await repository.insert_auction_draft(c, p)
    for index in (1, 2):
        await repository.insert_auction_team(
            c, {**p, "team_index": index, "team_name": f"팀 {index}"}
        )
    for index, r in enumerate(roster):
        await repository.insert_participant(
            c,
            {
                **p,
                "user": r["user_id"],
                "lane": r["lane"],
                "mmr": mmr[r["user_id"]]["mmr"] if r["lane"] is not None else None,
                "origin": "CAPTURED" if r["lane"] is not None else "UNAVAILABLE",
                "joined": p["now"] + timedelta(milliseconds=index),
            },
        )
    changed = await repository.mark_converted(c, p)
    if changed.rowcount != 1:
        raise ApiError.from_status(500)
    await message_update(c, p, await recruitment_snapshot(c, p["guild"], p["id"]))
    result = await snapshot(c, p["guild"], p["auction_id"], p["actor_id"])
    identifier = str(p["auction_id"])
    await notification(
        c,
        p,
        "AUCTION_RECRUITMENT_MESSAGE_CREATE",
        p["recruitment_channel"],
        f"auction-recruitment:{identifier}:initial-message",
        {"schemaVersion": 2, "auctionRecruitmentId": identifier, "snapshot": result},
    )
    await audit(
        c,
        p,
        "AUCTION_RECRUITMENT_CREATED_FROM_ENTRY_DRAFT",
        "AUCTION_RECRUITMENT",
        identifier,
        {
            "sourceRecruitmentId": str(p["id"]),
            "sourceRecruitmentRevision": source["revision"],
            "participantCount": len(roster),
            "declaredLaneCount": len(assigned),
        },
        result,
    )
    await invalidate(
        c, p, "RECRUITMENT", str(p["id"]), source["revision"] + 1, "RECRUITMENT_CONVERTED"
    )
    await invalidate(c, p, "AUCTION_RECRUITMENT", identifier, 1, "AUCTION_RECRUITMENT_CREATED")
    return result
