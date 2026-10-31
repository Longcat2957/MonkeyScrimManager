"""Transactional auction recruitment commands, preserving Axum's lock and effect order."""

import json
from datetime import timedelta, timezone
from typing import Any

from sqlalchemy.ext.asyncio import AsyncConnection

import monkeybot_api.repositories.auctions.recruitment_commands as repository
from monkeybot_api.core.errors import ApiError
from monkeybot_api.domain.auctions.recruitment_errors import error
from monkeybot_api.repositories.auctions.recruitments import (
    normalize_teams,
    setup_snapshot,
    shape,
    snapshot,
)
from monkeybot_api.repositories.effects import audit, complete, notification
from monkeybot_api.repositories.guild_access import require_operator
from monkeybot_api.repositories.idempotency import acquire
from monkeybot_api.repositories.members import sync_actor
from monkeybot_api.repositories.series.mmr import lock_mmr
from monkeybot_api.schemas.bot import Actor


async def member(c: AsyncConnection, p: dict[str, Any], user: str, *, target: bool = False) -> None:
    status = await repository.lock_membership(c, {**p, "user": user})
    if status != "ACTIVE":
        code = "guild_membership_required"
        if p["mode"] == "bot" and p["action"] != "create":
            code = "member_inactive"
        elif target:
            code = "guild_member_not_found" if status is None else "guild_member_inactive"
        raise error(code, mode=p["mode"])


async def operator(c: AsyncConnection, p: dict[str, Any], role: str, actor: Actor | None) -> None:
    await member(c, p, p["actor_id"])
    try:
        await require_operator(c, p, role, actor)
    except ApiError as exc:
        raise error(exc.code, exc.details, mode=p["mode"]) from None


async def effects(
    c: AsyncConnection,
    p: dict[str, Any],
    before: dict[str, Any] | None,
    result: dict[str, Any],
    *,
    channel: str | None = None,
) -> None:
    action = p["action"]
    audit_action, event = {
        "create": ("CREATE", "CREATED"),
        "join": ("JOIN", "PARTICIPANT_JOINED")
        if p["mode"] == "bot"
        else ("MEMBER_ADDED", "MEMBER_ADDED"),
        "leave": ("LEAVE", "PARTICIPANT_LEFT")
        if p["mode"] == "bot"
        else ("MEMBER_REMOVED", "MEMBER_REMOVED"),
        "close": ("CLOSED", "CLOSED"),
        "reopen": ("REOPENED", "REOPENED"),
        "cancel": ("CANCELLED", "CANCELLED"),
        "restore": ("RESTORED", "RESTORED"),
    }[action]
    record = result["recruitment"]
    await audit(
        c,
        p,
        "AUCTION_RECRUITMENT_" + audit_action,
        "AUCTION_RECRUITMENT",
        str(p["id"]),
        before,
        result,
    )
    await repository.insert_event(
        c,
        {
            **p,
            "topic": f"guild:{p['guild']}:auction-recruitment:{p['id']}",
            "target": str(p["id"]),
            "revision": record["revision"],
            "event": "AUCTION_RECRUITMENT_" + event,
            "payload": json.dumps(
                {"type": "invalidate", "resourceId": str(p["id"]), "revision": record["revision"]}
            ),
        },
    )
    channel = channel if action == "create" else record["channelId"]
    if channel is not None and (action in {"create", "restore"} or record["messageId"] is not None):
        verb, kind = {
            "create": ("initial-message", "CREATE"),
            "restore": (f"message:restore:{record['revision']}", "CREATE"),
            "cancel": (f"message:delete:{record['revision']}", "DELETE"),
        }.get(action, (f"message:update:{record['revision']}", "UPDATE"))
        await notification(
            c,
            p,
            "AUCTION_RECRUITMENT_MESSAGE_" + kind,
            channel,
            f"auction-recruitment:{p['id']}:{verb}",
            {"schemaVersion": 2, "auctionRecruitmentId": str(p["id"]), "snapshot": result},
        )
    await complete(c, p, result, 201 if action == "create" else 200)


async def create(c: AsyncConnection, p: dict[str, Any]) -> dict[str, Any]:
    channel = await repository.recruitment_channel(c, p)
    if channel is None:
        raise error("recruitment_channel_not_configured", mode=p["mode"])
    season = await repository.active_season(c, p["guild"])
    if season is None:
        raise error(
            "guild_setup_required" if p["mode"] == "bot" else "no_active_season", mode=p["mode"]
        )
    number = await repository.next_display_number(c, p["guild"])
    if number is None:
        raise ApiError.from_status(500)
    count, teams = shape(p["format"])
    start = p["start"].astimezone(timezone(timedelta(hours=9))).strftime("%Y-%m-%d %H:%M")
    title = f"{count}인 경매 내전 모집 #{number} · 예상 시작 {start} (KST)"
    rules = p["rules"]
    await repository.insert_recruitment(
        c,
        p["guild"],
        p["id"],
        season,
        number,
        title,
        p["format"],
        count,
        rules["semifinalBestOf"],
        rules["finalBestOf"],
        rules["hardFearless"],
        p["actor_id"],
        p["now"],
    )
    await repository.insert_draft(c, p["guild"], p["id"], p["actor_id"], p["now"])
    for index in range(1, teams + 1):
        await repository.insert_team(c, p["guild"], p["id"], index, f"팀 {index}", 1000)
    result = await snapshot(c, p["guild"], p["id"], p["actor_id"])
    await effects(c, p, None, result, channel=channel)
    return result


async def setup(c: AsyncConnection, p: dict[str, Any], locked: dict[str, Any]) -> dict[str, Any]:
    revision = await repository.lock_draft(c, p["guild"], p["id"])
    if revision is None:
        raise ApiError.from_status(500)
    teams = await repository.lock_teams(c, p["guild"], p["id"])
    roster = await repository.lock_participants(c, p["guild"], p["id"])
    if len(roster) != locked["target_count"]:
        raise error(
            "auction_recruitment_not_full",
            {"joinedCount": len(roster), "targetCount": locked["target_count"]},
            mode="setup",
        )
    revision_check(p, revision, "auction_setup_draft_revision_conflict")
    _, count = shape(locked["format"])
    normalized = normalize_teams(p["teams"], count, {r["user_id"] for r in roster})
    if len(teams) != count:
        raise ApiError.from_status(500)
    before = await setup_snapshot(c, p["guild"], p["id"], p["actor_id"])
    for team in normalized:
        await repository.update_team(
            c,
            team["teamName"],
            team["captainUserId"],
            team["initialPoints"],
            p["guild"],
            p["id"],
            team["teamIndex"],
        )
    updated = await repository.update_draft(c, p["actor_id"], p["now"], p["guild"], p["id"])
    if updated != revision + 1:
        raise ApiError.from_status(500)
    result = await setup_snapshot(c, p["guild"], p["id"], p["actor_id"])
    await audit(
        c, p, "AUCTION_SETUP_DRAFT_UPDATED", "AUCTION_SETUP_DRAFT", str(p["id"]), before, result
    )
    await complete(c, p, result, 200)
    return result


def revision_check(
    p: dict[str, Any], actual: int, code: str = "recruitment_revision_conflict"
) -> None:
    if actual != p["expected"]:
        raise error(
            code, {"expectedRevision": p["expected"], "actualRevision": actual}, mode=p["mode"]
        )


async def dispatch_dms(
    c: AsyncConnection, p: dict[str, Any], result: dict[str, Any]
) -> dict[str, Any]:
    record = result["recruitment"]
    queued, existing = 0, 0
    for participant in result["participants"]:
        user = participant["id"]
        if user.startswith("dmy_"):
            continue
        inserted = await notification(
            c,
            p,
            "AUCTION_RECRUITMENT_PARTICIPANT_CONFIRMATION_DM_CREATE",
            user,
            f"auction-recruitment:{p['id']}:participant-confirmation-dm:{record['revision']}:{user}",
            {
                "schemaVersion": 1,
                "guildId": p["guild"],
                "recipientUserId": user,
                "recruitmentId": str(p["id"]),
                "recruitmentTitle": record["title"],
                "recruitmentHref": result["links"]["draft"] or record["href"],
            },
        )
        queued += int(inserted)
        existing += int(not inserted)
    response = {"queuedRecipientCount": queued, "alreadyQueuedRecipientCount": existing}
    await audit(
        c,
        p,
        "AUCTION_RECRUITMENT_PARTICIPANT_CONFIRMATION_DMS_REQUESTED",
        "AUCTION_RECRUITMENT",
        str(p["id"]),
        None,
        response,
    )
    await complete(c, p, response, 200)
    return response


async def mutate(
    c: AsyncConnection, p: dict[str, Any], actor: Actor | None, participant: Actor | None
) -> dict[str, Any]:
    action, mode = p["action"], p["mode"]
    status = await repository.lock_guild(c, p["guild"])
    if status is None:
        raise error("not_found", mode=mode)
    if status != "ACTIVE":
        raise error("guild_inactive", {"currentStatus": status}, mode=mode)
    role = await repository.lock_config(c, p["guild"])
    if role is None:
        raise error("guild_setup_required", mode=mode)
    if action == "setup":
        await operator(c, p, role, None)
    try:
        replay = await acquire(c, p, mode == "bot")
    except ApiError as exc:
        raise error(exc.code, exc.details, mode=mode) from None
    if replay is not None:
        return replay
    if actor is not None:
        await sync_actor(c, actor, p)
        await member(c, p, p["actor_id"])
    if action == "create":
        await member(c, p, p["actor_id"])
        return await create(c, p)
    if action in {"join", "leave"} and actor is not None:
        if p["target"] != actor.id:
            await operator(c, p, role, actor)
    elif action in {"join", "leave", "close", "reopen", "dm"}:
        await operator(c, p, role, None)
    locked = await repository.lock_recruitment(c, p["guild"], p["id"])
    if locked is None:
        code = {
            "bot": "not_found",
            "web": "auction_recruitment_not_found",
            "setup": "auction_setup_draft_not_found",
            "dm": "recruitment_not_found",
        }[mode]
        raise error(code, mode=mode)
    if action in {"cancel", "restore"}:
        await member(c, p, p["actor_id"])
        if locked["created_by"] != p["actor_id"]:
            await operator(c, p, role, None)
    if action in {"cancel", "restore", "reopen"}:
        revision_check(p, locked["revision"])
    allowed = {
        "join": {"OPEN"},
        "leave": {"OPEN"},
        "close": {"OPEN"},
        "reopen": {"CLOSED"},
        "cancel": {"OPEN", "CLOSED"},
        "restore": {"CANCELLED"},
        "setup": {"CLOSED"},
        "dm": {"CLOSED"},
    }
    if locked["status"] not in allowed[action]:
        code = {
            "reopen": "auction_recruitment_reopen_unavailable",
            "cancel": "invalid_recruitment_state",
            "restore": "auction_recruitment_restore_unavailable",
            "setup": "auction_setup_draft_not_found",
            "dm": "participant_confirmation_dm_unavailable",
        }.get(
            action, "invalid_recruitment_state" if mode == "bot" else "auction_recruitment_not_open"
        )
        raise error(
            code, None if action == "setup" else {"currentStatus": locked["status"]}, mode=mode
        )
    if action == "setup":
        return await setup(c, p, locked)
    if action == "join":
        if participant is not None and participant.id != p["actor_id"]:
            await sync_actor(c, participant, {**p, "actor_id": participant.id})
            await member(c, p, participant.id)
        elif actor is None:
            await member(c, p, p["target"], target=True)
        lane = p["lane"]
        if lane is None:
            lane = await repository.main_lane(c, p["guild"], p["target"])
        if lane is None:
            raise error("auction_declared_lane_required", mode=mode)
        mmr = await lock_mmr(
            c, {**p, "season": locked["season_id"]}, [{"user_id": p["target"], "lane": lane}]
        )
    before = await snapshot(c, p["guild"], p["id"], p["actor_id"])
    if action == "dm":
        return await dispatch_dms(c, p, before)
    count = before["recruitment"]["joinedParticipantCount"]
    if action == "join":
        status = await repository.participant_status(c, p["guild"], p["id"], p["target"])
        if status == "JOINED":
            raise error("already_participant", mode=mode)
        if count >= locked["target_count"]:
            raise error("recruitment_full", mode=mode)
        await repository.join_participant(
            c, p["guild"], p["id"], p["target"], lane, mmr[p["target"]]["mmr"], p["now"]
        )
    elif action == "leave":
        changed = await repository.withdraw_participant(
            c, p["now"], p["guild"], p["id"], p["target"]
        )
        if changed.rowcount != 1:
            raise error("not_participant", mode=mode)
        await repository.clear_captain(c, p["guild"], p["id"], p["target"])
    if action in {"join", "leave"}:
        await repository.bump_membership_revisions(c, p["actor_id"], p["now"], p["guild"], p["id"])
    elif action == "close":
        if count != locked["target_count"]:
            raise error(
                "auction_recruitment_not_full",
                {"joinedCount": count, "targetCount": locked["target_count"]},
            )
        await repository.close(c, p["actor_id"], p["now"], p["guild"], p["id"])
    elif action == "restore":
        await repository.restore(
            c,
            "CLOSED" if locked["closed_at"] is not None else "OPEN",
            p["actor_id"],
            p["now"],
            p["guild"],
            p["id"],
            p["expected"],
        )
    else:
        await repository.reopen_or_cancel(
            c, p["actor_id"], p["now"], p["guild"], p["id"], p["expected"], action == "reopen"
        )
    result = await snapshot(c, p["guild"], p["id"], p["actor_id"])
    await effects(c, p, before, result)
    return result
