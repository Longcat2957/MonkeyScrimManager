"""Auction match lineup transactions, retaining the Rust lock and effect order."""

import uuid
from typing import Any

from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.serialization import mapped
from monkeybot_api.db.queries import params
from monkeybot_api.repositories.auctions import lineup_commands as storage
from monkeybot_api.repositories.auctions.matches import (
    STATUSES,
    complete_lineup,
    has_result_artifacts,
    snapshot,
)
from monkeybot_api.repositories.effects import audit, complete, invalidate, notification
from monkeybot_api.repositories.guild_access import require_operator
from monkeybot_api.repositories.idempotency import acquire
from monkeybot_api.schemas.auctions.lineups import error
from monkeybot_api.schemas.users import LANES


def validate_lineups(
    lineups: list[dict[str, Any]],
    members: list[dict[str, Any]],
    left: uuid.UUID,
    right: uuid.UUID,
    complete: bool = False,
) -> None:
    relevant = [r for r in members if r["team_id"] in {left, right}]
    member_users = {(r["team_id"], r["id"]): r["user_id"] for r in relevant}
    if (
        any(not 1 <= r["roster_slot"] <= 5 for r in relevant)
        or len(member_users) != len(relevant)
        or len({r["user_id"] for r in relevant}) != len(relevant)
        or len({(r["team_id"], r["roster_slot"]) for r in relevant}) != len(relevant)
    ):
        raise ApiError.from_status(500)
    if complete and any(sum(r["team_id"] == t for r in relevant) != 5 for t in (left, right)):
        raise error("auction_match_lineup_incomplete")
    if (
        any(
            r["team_id"] not in {left, right}
            or r["lane"] not in LANES
            or member_users.get((r["team_id"], r["team_member_id"])) != r["user_id"]
            for r in lineups
        )
        or len({(r["team_id"], r["lane"]) for r in lineups}) != len(lineups)
        or len({r["team_member_id"] for r in lineups}) != len(lineups)
        or len({r["user_id"] for r in lineups}) != len(lineups)
    ):
        raise ApiError.from_status(500)
    if complete and not complete_lineup(
        [mapped(r, "team_id lane team_member_id user_id") for r in lineups], left, right
    ):
        raise error("auction_match_lineup_incomplete")


def confirmation(guild: str, tournament: dict[str, Any], state: dict[str, Any]) -> dict[str, Any]:
    m = state["match"]
    teams = []
    for t in state["teams"]:
        players = []
        for lane in LANES:
            lineup = next(
                r for r in state["lineups"] if r["teamId"] == t["id"] and r["lane"] == lane
            )
            member = next(r for r in t["members"] if r["teamMemberId"] == lineup["teamMemberId"])
            players.append(
                {"id": lineup["userId"], "displayName": member["displayName"], "lane": lane}
            )
        teams.append({"teamId": t["id"], "name": t["teamName"], "players": players})
    return {
        "tournament": mapped(tournament, "id display_number title format hard_fearless"),
        "matchup": {key: m[key] for key in ("id", "matchKey", "label", "bestOf")},
        "teams": teams,
        "links": {"pickBan": f"/g/{guild}/auction/matches/{m['id']}"},
    }


async def mutate(c: AsyncConnection, p: dict[str, Any], body: dict[str, Any]) -> dict[str, Any]:
    guild_status = await storage.lock_guild(c, params(p["guild"]))
    if guild_status is None:
        raise error("not_found")
    if guild_status != "ACTIVE":
        raise error("guild_inactive", {"currentStatus": guild_status})
    role = await storage.lock_config(c, params(p["guild"]))
    if role is None:
        raise error("guild_setup_required")
    member = await storage.lock_member_status(c, p)
    if member != "ACTIVE":
        raise error("guild_membership_required")
    try:
        await require_operator(c, p, role, None)
        replay = await acquire(c, p, False)
    except ApiError as exc:
        raise error(exc.code, exc.details) from None
    if replay is not None:
        return replay
    tournament_id = await storage.find_tournament_id(c, p)
    if tournament_id is None:
        raise error("auction_match_not_found")
    tournament = await storage.lock_tournament(c, p["guild"], tournament_id)
    if tournament is None:
        raise error("auction_tournament_not_found")
    if tournament["status"] not in {"BRACKET_SETUP", "IN_GAME"}:
        raise error("auction_match_tournament_not_ready")
    m = await storage.lock_match(c, p["guild"], p["id"])
    if m is None:
        raise error("auction_match_not_found")
    if m["tournament_id"] != tournament_id:
        raise ApiError.from_status(500)
    if m["revision"] != body["expectedRevision"]:
        raise error(
            "auction_match_revision_conflict",
            {"expectedRevision": body["expectedRevision"], "actualRevision": m["revision"]},
        )
    if m["status"] not in STATUSES:
        raise ApiError.from_status(500)
    left, right = m["left_team_id"], m["right_team_id"]
    if left is None or right is None:
        raise error("auction_match_teams_pending")
    if left == right:
        raise ApiError.from_status(500)
    await storage.lock_teams(c, p["guild"], tournament_id)
    members = await storage.lock_members(c, p["guild"], tournament_id)
    lineups = await storage.lock_lineups(c, p["guild"], tournament_id, p["id"])
    validate_lineups(lineups, members, left, right)
    before = await snapshot(c, p["guild"], p["id"], p["actor_id"])
    kind = body["kind"]
    v = {**p, "tournament": tournament_id}
    if kind != "UNLOCK":
        if m["status"] != "LINEUP_PENDING":
            raise error("auction_match_lineup_not_editable", {"currentStatus": m["status"]})
        if m["lineups_locked_at"] is not None:
            raise error("auction_match_lineup_locked")
    if kind == "ASSIGN":
        team = uuid.UUID(body["teamId"])
        if team not in {left, right}:
            raise error("auction_match_lineup_team_not_found")
        v.update(team=team, lane=body["lane"])
        if body["teamMemberId"] is None:
            await storage.clear_assignment(c, v)
        else:
            team_member_id = uuid.UUID(body["teamMemberId"])
            found = next(
                (r for r in members if r["team_id"] == team and r["id"] == team_member_id), None
            )
            if found is None:
                raise error("auction_match_lineup_member_not_found")
            if any(
                r["user_id"] == found["user_id"]
                and (r["team_id"], r["lane"]) != (team, body["lane"])
                for r in lineups
            ):
                raise error("auction_match_lineup_player_assigned")
            await storage.assign_member(
                c, {**v, "member": team_member_id, "user": found["user_id"]}
            )
    elif kind == "LOCK":
        validate_lineups(lineups, members, left, right, True)
    else:
        if (
            m["status"] != "READY"
            or m["lineups_locked_at"] is None
            or m["lineups_locked_by"] is None
        ):
            raise error("auction_match_lineup_unlock_unavailable", {"currentStatus": m["status"]})
        validate_lineups(lineups, members, left, right, True)
        if await has_result_artifacts(c, p["guild"], p["id"]):
            raise error("auction_match_lineup_result_artifacts_present")
    revision = await storage.update_lineup_state(c, p, kind=kind)
    if kind == "LOCK":
        await storage.start_tournament(c, v)
    elif kind == "UNLOCK":
        await storage.reset_unstarted_tournament(c, v)
    result = await snapshot(c, p["guild"], p["id"], p["actor_id"])
    if revision != result["match"]["revision"]:
        raise ApiError.from_status(500)
    if kind == "LOCK":
        channel = await storage.entry_channel(c, p)
        if channel is not None:
            await notification(
                c,
                p,
                "AUCTION_MATCH_LINEUP_CONFIRMATION_CREATE",
                channel,
                f"auction-match:{p['id']}:lineup-confirmation:{revision}",
                {
                    "schemaVersion": 1,
                    "matchId": str(p["id"]),
                    "snapshot": confirmation(p["guild"], tournament, result),
                },
            )
    await audit(c, p, "AUCTION_MATCH_LINEUP_" + kind, "AUCTION_MATCH", str(p["id"]), before, result)
    await invalidate(c, p, "AUCTION_MATCH", str(p["id"]), revision, "AUCTION_MATCH_LINEUP_UPDATED")
    await complete(c, p, result, 200)
    return result
