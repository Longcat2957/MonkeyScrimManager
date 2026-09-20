import json
import uuid
from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api import auction_queries as source_sql
from monkeybot_api import tournament_queries as sql
from monkeybot_api.auction_snapshot import normalize_teams, rows, shape
from monkeybot_api.auction_snapshot import snapshot as source_snapshot
from monkeybot_api.delivery_effects import audit, complete, notification
from monkeybot_api.errors import ApiError
from monkeybot_api.notification_delivery import one, params
from monkeybot_api.recruitment_create import acquire
from monkeybot_api.recruitment_lifecycle import require_operator
from monkeybot_api.tournament_commands import apply
from monkeybot_api.tournament_errors import error
from monkeybot_api.tournament_snapshot import snapshot
from monkeybot_api.tournament_state import change, randomize, validate


async def event(
    c: AsyncConnection,
    p: dict[str, Any],
    identifier: uuid.UUID,
    kind: str,
    revision: int,
    name: str,
) -> None:
    await c.execute(
        text("""INSERT INTO outbox_events
        (id,guild_id,topic,aggregate_type,aggregate_id,aggregate_revision,
         event_type,
         payload,occurred_at,available_at)
        VALUES (:event,:guild,:topic,:kind,:target,:revision,:name,
                CAST(:payload AS jsonb),:now,:now)"""),
        {
            **p,
            "event": uuid.uuid7(),
            "topic": f"guild:{p['guild']}:{kind.lower().replace('_', '-')}:{identifier}",
            "kind": kind,
            "target": str(identifier),
            "revision": revision,
            "name": name,
            "payload": json.dumps(
                {"type": "invalidate", "resourceId": str(identifier), "revision": revision}
            ),
        },
    )


async def create(c: AsyncConnection, p: dict[str, Any], body: dict[str, Any]) -> dict[str, Any]:
    source = uuid.UUID(body["sourceRecruitmentId"])
    recruitment = await one(c, source_sql.LOCK_RECRUITMENT, p["guild"], source)
    if recruitment is None:
        raise error("auction_recruitment_not_found", create=True)
    if recruitment["status"] != "CLOSED":
        raise error(
            "auction_tournament_source_not_ready",
            {"currentStatus": recruitment["status"]},
            create=True,
        )
    revision = await c.scalar(text(source_sql.LOCK_DRAFT), params(p["guild"], source))
    if revision is None:
        raise ApiError.from_status(500)
    stored = await rows(c, source_sql.LOCK_TEAMS, p["guild"], source)
    roster = await rows(c, source_sql.LOCK_PARTICIPANTS, p["guild"], source)
    if len(roster) != recruitment["target_count"]:
        raise error(
            "auction_recruitment_not_full",
            {"joinedCount": len(roster), "targetCount": recruitment["target_count"]},
            create=True,
        )
    if revision != body["expectedRevision"]:
        raise error(
            "auction_setup_draft_revision_conflict",
            {"expectedRevision": body["expectedRevision"], "actualRevision": revision},
            create=True,
        )
    count, team_count = shape(recruitment["format"])
    if len(stored) != team_count:
        raise error(
            "invalid_auction_setup_team_count",
            {"expectedTeamCount": team_count, "actualTeamCount": len(stored)},
            create=True,
        )
    configured = sum(t["captain_user_id"] is not None for t in stored)
    if configured != len(stored):
        raise error(
            "auction_setup_incomplete",
            {"configuredTeamCount": configured, "requiredTeamCount": team_count},
            create=True,
        )
    try:
        teams = normalize_teams(
            [
                {
                    "teamIndex": r["team_index"],
                    "teamName": r["team_name"],
                    "captainUserId": r["captain_user_id"],
                    "initialPoints": r["initial_points"],
                }
                for r in stored
            ],
            team_count,
            {r["user_id"] for r in roster},
        )
    except ApiError as exc:
        if exc.code == "invalid_auction_setup_team_name":
            raise ApiError(422, exc.code, "팀 이름이 올바르지 않습니다.") from None
        raise
    if await c.scalar(text(sql.EXISTING), params(p["guild"], source)):
        raise error("auction_tournament_already_exists", create=True)
    number = await c.scalar(text(sql.COUNTER), params(p["guild"]))
    if number is None:
        raise ApiError.from_status(500)
    await change(
        c,
        sql.CREATE,
        p["guild"],
        p["id"],
        recruitment["season_id"],
        source,
        number,
        f"{count}인 경매 내전 #{number}",
        recruitment["format"],
        recruitment["semifinal_best_of"],
        recruitment["final_best_of"],
        recruitment["hard_fearless"],
        p["now"],
        p["actor_id"],
    )
    captains = set()
    for i, team in enumerate(teams):
        captain = next(r for r in roster if r["user_id"] == team["captainUserId"])
        captains.add(captain["user_id"])
        team_id = p["team_ids"][i]
        await change(
            c,
            sql.CREATE_TEAM,
            p["guild"],
            p["id"],
            team_id,
            team["teamIndex"],
            team["teamName"],
            team["initialPoints"],
            p["now"],
        )
        await change(
            c,
            sql.CAPTAIN,
            p["guild"],
            p["id"],
            team_id,
            p["captain_ids"][i],
            captain["user_id"],
            captain["declared_lane"],
            captain["mmr_at_join"],
            captain["snapshot_origin"],
            p["now"],
        )
        await change(
            c,
            sql.INITIAL_LEDGER,
            p["guild"],
            p["id"],
            p["initial_ledger_ids"][i],
            team_id,
            team["initialPoints"],
            p["actor_id"],
            p["now"],
        )
    order = await randomize(c, [r["user_id"] for r in roster if r["user_id"] not in captains])
    for position, user in enumerate(order):
        r = next(r for r in roster if r["user_id"] == user)
        await change(
            c,
            sql.CREATE_CANDIDATE,
            p["guild"],
            p["id"],
            user,
            r["declared_lane"],
            r["mmr_at_join"],
            r["snapshot_origin"],
            position,
            p["now"],
        )
    for slot in ("A_0", "A_1", "B_0", "B_1"):
        await change(c, sql.CREATE_SLOT, p["guild"], p["id"], slot, p["now"])
    await change(c, sql.CONVERT, p["actor_id"], p["now"], p["guild"], source)
    result = await snapshot(c, p["guild"], p["id"], p["actor_id"])
    converted = await source_snapshot(c, p["guild"], source, p["actor_id"])
    record = converted["recruitment"]
    if record["channelId"] is not None and record["messageId"] is not None:
        await notification(
            c,
            p,
            "AUCTION_RECRUITMENT_MESSAGE_UPDATE",
            record["channelId"],
            f"auction-recruitment:{source}:message:update:{record['revision']}",
            {"schemaVersion": 2, "auctionRecruitmentId": str(source), "snapshot": converted},
        )
    await audit(
        c, p, "AUCTION_TOURNAMENT_CREATED", "AUCTION_TOURNAMENT", str(p["id"]), None, result
    )
    await event(
        c, p, source, "AUCTION_RECRUITMENT", record["revision"], "AUCTION_RECRUITMENT_CONVERTED"
    )
    await event(c, p, p["id"], "AUCTION_TOURNAMENT", 0, "AUCTION_TOURNAMENT_CREATED")
    await complete(c, p, result, 201)
    return result


async def mutate(
    c: AsyncConnection, p: dict[str, Any], body: dict[str, Any], creating: bool
) -> dict[str, Any]:
    status = await c.scalar(text(source_sql.LOCK_GUILD), params(p["guild"]))
    if status is None:
        raise error("not_found", create=creating)
    if status != "ACTIVE":
        raise error("guild_inactive", {"currentStatus": status}, create=creating)
    role = await c.scalar(text(source_sql.LOCK_CONFIG), params(p["guild"]))
    if role is None:
        raise error("guild_setup_required", create=creating)
    member = await c.scalar(
        text(
            "SELECT status FROM guild_members WHERE guild_id=:guild "
            "AND user_id=:actor_id FOR UPDATE"
        ),
        p,
    )
    if member != "ACTIVE":
        raise error("guild_membership_required", create=creating)
    try:
        await require_operator(c, p, role, None)
        replay = await acquire(c, p, False)
    except ApiError as exc:
        raise error(exc.code, exc.details, create=creating) from None
    if replay is not None:
        return replay
    if creating:
        return await create(c, p, body)
    tournament = await one(c, sql.LOCK_TOURNAMENT, p["guild"], p["id"])
    if tournament is None:
        raise error("auction_tournament_not_found")
    teams, members, candidates, lots, actions, ledger = [
        await rows(c, q, p["guild"], p["id"])
        for q in (
            sql.LOCK_TEAMS,
            sql.LOCK_MEMBERS,
            sql.LOCK_CANDIDATES,
            sql.LOCK_LOTS,
            sql.ACTIONS,
            sql.LEDGER,
        )
    ]
    if tournament["revision"] != body["expectedRevision"]:
        raise error(
            "auction_tournament_revision_conflict",
            {
                "expectedRevision": body["expectedRevision"],
                "actualRevision": tournament["revision"],
            },
        )
    if await c.scalar(text(sql.ARTIFACTS), params(p["guild"], p["id"])):
        raise error("auction_tournament_draft_locked")
    if not (
        tournament["status"] == "BIDDING"
        or (
            tournament["status"] == "BRACKET_SETUP"
            and tournament["bracket_locked_at"] is None
            and body["kind"] in {"REVERT_LAST", "RESET_TO_INITIAL"}
        )
    ):
        raise error(
            "auction_tournament_draft_not_editable", {"currentStatus": tournament["status"]}
        )
    validate(teams, members, candidates, lots)
    before = await snapshot(c, p["guild"], p["id"], p["actor_id"])
    status = await apply(c, p, body, tournament, teams, members, candidates, lots, actions, ledger)
    revision = await c.scalar(
        text(sql.BUMP), params(status, p["actor_id"], p["now"], p["guild"], p["id"])
    )
    result = await snapshot(c, p["guild"], p["id"], p["actor_id"])
    if result["tournament"]["revision"] != revision:
        raise ApiError.from_status(500)
    await audit(
        c, p, "AUCTION_DRAFT_" + body["kind"], "AUCTION_TOURNAMENT", str(p["id"]), before, result
    )
    await event(c, p, p["id"], "AUCTION_TOURNAMENT", revision, "AUCTION_TOURNAMENT_DRAFT_UPDATED")
    await complete(c, p, result, 200)
    return result
