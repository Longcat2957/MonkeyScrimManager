"""Transactional auction recruitment commands, preserving Axum's lock and effect order."""

import json
from datetime import timedelta, timezone
from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api import auction_queries as sql
from monkeybot_api.auction_errors import error
from monkeybot_api.auction_snapshot import normalize_teams, rows, setup_snapshot, shape, snapshot
from monkeybot_api.delivery_effects import audit, complete, notification
from monkeybot_api.errors import ApiError
from monkeybot_api.notification_delivery import one, params
from monkeybot_api.recruitment_create import Actor, acquire, sync_actor
from monkeybot_api.recruitment_lifecycle import require_operator
from monkeybot_api.series_mmr import lock_mmr


async def execute(c: AsyncConnection, query: str, *values: Any) -> None:
    result = await c.execute(text(query), params(*values))
    if result.rowcount != 1:
        raise ApiError.from_status(500)


async def member(c: AsyncConnection, p: dict[str, Any], user: str, *, target: bool = False) -> None:
    status = await c.scalar(
        text("SELECT status FROM guild_members WHERE guild_id=:guild AND user_id=:user FOR UPDATE"),
        {**p, "user": user},
    )
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
    await c.execute(
        text("""
        INSERT INTO outbox_events (id,guild_id,topic,aggregate_type,aggregate_id,
            aggregate_revision,event_type,payload,occurred_at,available_at)
        VALUES (:event_id,:guild,:topic,'AUCTION_RECRUITMENT',:target,:revision,:event,
            CAST(:payload AS jsonb),:now,:now)
    """),
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
    channel = await c.scalar(
        text("SELECT recruitment_channel_id FROM guild_config WHERE guild_id=:guild"), p
    )
    if channel is None:
        raise error("recruitment_channel_not_configured", mode=p["mode"])
    season = await c.scalar(text(sql.SEASON), params(p["guild"]))
    if season is None:
        raise error(
            "guild_setup_required" if p["mode"] == "bot" else "no_active_season", mode=p["mode"]
        )
    number = await c.scalar(text(sql.COUNTER), params(p["guild"]))
    if number is None:
        raise ApiError.from_status(500)
    count, teams = shape(p["format"])
    start = p["start"].astimezone(timezone(timedelta(hours=9))).strftime("%Y-%m-%d %H:%M")
    title = f"{count}인 경매 내전 모집 #{number} · 예상 시작 {start} (KST)"
    rules = p["rules"]
    await execute(
        c,
        sql.INSERT_RECRUITMENT,
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
    await execute(c, sql.INSERT_DRAFT, p["guild"], p["id"], p["actor_id"], p["now"])
    for index in range(1, teams + 1):
        await execute(c, sql.INSERT_TEAM, p["guild"], p["id"], index, f"팀 {index}", 1000)
    result = await snapshot(c, p["guild"], p["id"], p["actor_id"])
    await effects(c, p, None, result, channel=channel)
    return result


async def setup(c: AsyncConnection, p: dict[str, Any], locked: dict[str, Any]) -> dict[str, Any]:
    revision = await c.scalar(text(sql.LOCK_DRAFT), params(p["guild"], p["id"]))
    if revision is None:
        raise ApiError.from_status(500)
    teams = await rows(c, sql.LOCK_TEAMS, p["guild"], p["id"])
    roster = await rows(c, sql.LOCK_PARTICIPANTS, p["guild"], p["id"])
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
        await execute(
            c,
            sql.UPDATE_TEAM,
            team["teamName"],
            team["captainUserId"],
            team["initialPoints"],
            p["guild"],
            p["id"],
            team["teamIndex"],
        )
    updated = await c.scalar(
        text(sql.UPDATE_DRAFT), params(p["actor_id"], p["now"], p["guild"], p["id"])
    )
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
    status = await c.scalar(text(sql.LOCK_GUILD), params(p["guild"]))
    if status is None:
        raise error("not_found", mode=mode)
    if status != "ACTIVE":
        raise error("guild_inactive", {"currentStatus": status}, mode=mode)
    role = await c.scalar(text(sql.LOCK_CONFIG), params(p["guild"]))
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
    locked = await one(c, sql.LOCK_RECRUITMENT, p["guild"], p["id"])
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
            lane = await c.scalar(text(sql.MAIN_LANE), params(p["guild"], p["target"]))
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
        status = await c.scalar(
            text(sql.PARTICIPANT_STATUS), params(p["guild"], p["id"], p["target"])
        )
        if status == "JOINED":
            raise error("already_participant", mode=mode)
        if count >= locked["target_count"]:
            raise error("recruitment_full", mode=mode)
        await execute(
            c, sql.JOIN, p["guild"], p["id"], p["target"], lane, mmr[p["target"]]["mmr"], p["now"]
        )
    elif action == "leave":
        changed = await c.execute(
            text(sql.LEAVE), params(p["now"], p["guild"], p["id"], p["target"])
        )
        if changed.rowcount != 1:
            raise error("not_participant", mode=mode)
        await c.execute(text(sql.CLEAR_CAPTAIN), params(p["guild"], p["id"], p["target"]))
    if action in {"join", "leave"}:
        for query in (sql.BUMP_RECRUITMENT, sql.BUMP_DRAFT):
            await execute(c, query, p["actor_id"], p["now"], p["guild"], p["id"])
    elif action == "close":
        if count != locked["target_count"]:
            raise error(
                "auction_recruitment_not_full",
                {"joinedCount": count, "targetCount": locked["target_count"]},
            )
        await execute(c, sql.CLOSE, p["actor_id"], p["now"], p["guild"], p["id"])
    elif action == "restore":
        await execute(
            c,
            sql.RESTORE,
            "CLOSED" if locked["closed_at"] is not None else "OPEN",
            p["actor_id"],
            p["now"],
            p["guild"],
            p["id"],
            p["expected"],
        )
    else:
        await execute(
            c,
            sql.REOPEN if action == "reopen" else sql.CANCEL,
            p["actor_id"],
            p["now"],
            p["guild"],
            p["id"],
            p["expected"],
        )
    result = await snapshot(c, p["guild"], p["id"], p["actor_id"])
    await effects(c, p, before, result)
    return result
