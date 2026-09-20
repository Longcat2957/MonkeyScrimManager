"""Convert a closed regular roster into an auction without requiring lane assignments."""

from datetime import timedelta, timezone
from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api import auction_queries as sql
from monkeybot_api.auction_mutations import execute
from monkeybot_api.auction_snapshot import snapshot
from monkeybot_api.delivery_effects import audit, invalidate, message_update, notification
from monkeybot_api.errors import ApiError
from monkeybot_api.notification_delivery import params
from monkeybot_api.recruitment_lifecycle import Revision
from monkeybot_api.recruitment_snapshot import recruitment_snapshot
from monkeybot_api.series_mmr import lock_mmr


class Conversion(Revision):
    expectedStartAt: Any = None


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
    draft = await c.scalar(
        text(
            "SELECT revision FROM recruitment_entry_drafts "
            "WHERE guild_id=:guild AND recruitment_id=:id FOR UPDATE"
        ),
        p,
    )
    if draft is None:
        raise ApiError.from_status(500)
    if await c.scalar(
        text("""
        SELECT EXISTS (SELECT 1 FROM series WHERE guild_id=:guild
            AND source_recruitment_id=:id AND status <> 'CANCELLED' AND deleted_at IS NULL)
    """),
        p,
    ):
        raise error("entry_draft_already_converted_to_series")
    roster = (
        (
            await c.execute(
                text("""
        SELECT participant.user_id, assignment.lane
        FROM recruitment_participants AS participant
        LEFT JOIN recruitment_entry_assignments AS assignment
          ON assignment.guild_id=participant.guild_id
         AND assignment.recruitment_id=participant.recruitment_id
         AND assignment.user_id=participant.user_id
        WHERE participant.guild_id=:guild AND participant.recruitment_id=:id
          AND participant.status='JOINED'
        ORDER BY participant.joined_at, participant.user_id FOR UPDATE OF participant
    """),
                p,
            )
        )
        .mappings()
        .all()
    )
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
    number = await c.scalar(text(sql.COUNTER), params(p["guild"]))
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
    await c.execute(
        text("""
        INSERT INTO auction_recruitments (guild_id,id,season_id,display_number,title,
            format,target_count,semifinal_best_of,final_best_of,hard_fearless,status,
            revision,created_by,created_at,updated_by,updated_at,closed_at)
        VALUES (:guild,:auction_id,:season,:number,:title,'TEN_PERSON',10,NULL,:best_of,
            :fearless,'CLOSED',1,:actor_id,:now,:actor_id,:now,:now)
    """),
        values,
    )
    await execute(c, sql.INSERT_DRAFT, p["guild"], p["auction_id"], p["actor_id"], p["now"])
    for index in (1, 2):
        await execute(c, sql.INSERT_TEAM, p["guild"], p["auction_id"], index, f"팀 {index}", 1000)
    for index, r in enumerate(roster):
        await c.execute(
            text("""
            INSERT INTO auction_recruitment_participants
                (guild_id,recruitment_id,user_id,status,declared_lane,mmr_at_join,
                 snapshot_origin,joined_at)
            VALUES (:guild,:auction_id,:user,'JOINED',:lane,:mmr,:origin,:joined)
        """),
            {
                **p,
                "user": r["user_id"],
                "lane": r["lane"],
                "mmr": mmr[r["user_id"]]["mmr"] if r["lane"] is not None else None,
                "origin": "CAPTURED" if r["lane"] is not None else "UNAVAILABLE",
                "joined": p["now"] + timedelta(milliseconds=index),
            },
        )
    changed = await c.execute(
        text("""
        UPDATE recruitments SET status='CONVERTED',revision=revision+1,
            updated_by=:actor_id,updated_at=:now,converted_at=:now
        WHERE guild_id=:guild AND id=:id AND status='CLOSED'
    """),
        p,
    )
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
