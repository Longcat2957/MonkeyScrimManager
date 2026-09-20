import uuid
from typing import Any

from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api import tournament_queries as sql
from monkeybot_api.errors import ApiError
from monkeybot_api.notification_delivery import one
from monkeybot_api.tournament_errors import error
from monkeybot_api.tournament_state import (
    action,
    change,
    close_lot,
    current,
    find,
    lot,
    points,
    shuffle,
    transition,
)


async def revert(
    c: AsyncConnection,
    p: dict[str, Any],
    teams: list[dict[str, Any]],
    candidates: list[dict[str, Any]],
    lots: list[dict[str, Any]],
    actions: list[dict[str, Any]],
    ledger: list[dict[str, Any]],
) -> None:
    reversed_ids = {r["reverses_action_id"] for r in actions if r["kind"] == "REVERT_LAST"}
    original = next(
        (
            r
            for r in reversed(actions)
            if r["kind"] != "REVERT_LAST" and r["id"] not in reversed_ids
        ),
        None,
    )
    if original is None:
        raise error("auction_draft_no_revertable_action")
    user, kind, old_lot, team_id = (
        original["target_user_id"],
        original["kind"],
        original["lot_id"],
        original["team_id"],
    )
    if user is None or old_lot is None:
        raise ApiError.from_status(500)
    candidate = find(candidates, "user_id", user, "auction_draft_candidate_not_found")
    target = current(candidates, lots)
    team = debit = None
    if kind in {"DRAW_NEXT", "RESTORE_UNSOLD"}:
        if (
            target is None
            or target["target_user_id"] != user
            or target["id"] != old_lot
            or candidate["state"] != "ACTIVE"
        ):
            raise error("auction_draft_current_target_mismatch")
        await action(c, p, actions, "REVERT_LAST", user, original=original)
        await close_lot(c, p, old_lot, "REVERTED")
        await transition(c, p, user, "ACTIVE", "QUEUED" if kind == "DRAW_NEXT" else "UNSOLD")
        return
    if kind == "AWARD_BID":
        team = find(teams, "id", team_id, "auction_draft_team_not_found")
        debit = next(
            (
                r
                for r in ledger
                if r["team_id"] == team_id
                and r["reason"] == "BID_DEBIT"
                and r["source_action_id"] == original["id"]
            ),
            None,
        )
        if debit is None:
            raise ApiError.from_status(500)
        if (
            target is not None
            or candidate["state"] != "ASSIGNED"
            or team["point_revision"] != debit["point_revision"]
            or team["current_points"] != debit["points_after"]
            or original["points"] != debit["points_before"] - debit["points_after"]
        ):
            raise error("auction_draft_revert_unavailable")
    elif kind in {"MARK_UNSOLD", "ASSIGN_CURRENT", "ASSIGN_UNSOLD"}:
        if target is not None or candidate["state"] != (
            "UNSOLD" if kind == "MARK_UNSOLD" else "ASSIGNED"
        ):
            raise error("auction_draft_candidate_state_mismatch")
        if kind == "MARK_UNSOLD" and not any(
            r["id"] == old_lot and r["status"] == "UNSOLD" for r in lots
        ):
            raise ApiError.from_status(500)
    else:
        raise ApiError.from_status(500)
    new_lot = None
    if kind != "ASSIGN_UNSOLD":
        await lot(c, p, lots, user)
        new_lot = p["lot_id"]
    await action(c, p, actions, "REVERT_LAST", user, team_id, new_lot, original=original)
    if kind != "MARK_UNSOLD":
        released = await one(c, sql.RELEASE_MEMBER_READ, p["guild"], p["id"], original["id"], user)
        if released is None:
            raise ApiError.from_status(500)
        await change(c, sql.RELEASE_MEMBER, p["now"], p["guild"], p["id"], released["id"])
        if released["team_id"] != team_id or released["acquired_via"] != (
            "BID" if kind == "AWARD_BID" else "MANUAL"
        ):
            raise ApiError.from_status(500)
    await change(c, sql.REVERT_LOT, p["guild"], p["id"], old_lot)
    await transition(
        c, p, user, candidate["state"], "UNSOLD" if kind == "ASSIGN_UNSOLD" else "ACTIVE"
    )
    if kind == "AWARD_BID":
        assert team is not None and debit is not None
        await points(c, p, team, debit["points_before"], debit)


async def apply(
    c: AsyncConnection,
    p: dict[str, Any],
    body: dict[str, Any],
    tournament: dict[str, Any],
    teams: list[dict[str, Any]],
    members: list[dict[str, Any]],
    candidates: list[dict[str, Any]],
    lots: list[dict[str, Any]],
    actions: list[dict[str, Any]],
    ledger: list[dict[str, Any]],
) -> str:
    kind = body["kind"]
    target = current(candidates, lots)
    if kind == "RESHUFFLE_CANDIDATES":
        await shuffle(c, p, candidates)
        return "BIDDING"
    if kind == "RESET_TO_INITIAL":
        await change(c, sql.CLEAR_SLOTS, p["actor_id"], p["now"], p["guild"], p["id"], count=None)
        for query in (
            sql.RESET_REVERSALS,
            sql.RESET_DEBITS,
            sql.RESET_MEMBERS,
            sql.RESET_REVERSAL_ACTIONS,
            sql.RESET_ACTIONS,
            sql.RESET_LOTS,
            sql.RESET_POINTS,
        ):
            await change(c, query, p["guild"], p["id"], count=None)
        await shuffle(c, p, candidates, True)
        return "BIDDING"
    if kind == "REVERT_LAST":
        if tournament["status"] == "BRACKET_SETUP":
            await change(
                c, sql.CLEAR_SLOTS, p["actor_id"], p["now"], p["guild"], p["id"], count=None
            )
        await revert(c, p, teams, candidates, lots, actions, ledger)
        return "BIDDING"
    if kind in {"DRAW_NEXT", "ASSIGN_UNSOLD", "RESTORE_UNSOLD"} and target is not None:
        raise error("auction_draft_current_target_exists")
    if kind == "DRAW_NEXT":
        candidate = next((r for r in candidates if r["state"] == "QUEUED"), None)
        if candidate is None:
            raise error("auction_draft_no_queued_candidate")
        user = candidate["user_id"]
        await lot(c, p, lots, user)
        await change(c, sql.DRAW, p["now"], p["guild"], p["id"], user)
        await action(c, p, actions, kind, user, lot=p["lot_id"])
        return "BIDDING"
    user = body["memberId"]
    if kind in {"MARK_UNSOLD", "AWARD_BID", "ASSIGN_CURRENT"}:
        if target is None:
            raise error("auction_draft_current_target_required")
        if target["target_user_id"] != user:
            raise error("auction_draft_current_target_mismatch")
    candidate = find(candidates, "user_id", user, "auction_draft_candidate_not_found")
    if kind in {"MARK_UNSOLD", "ASSIGN_UNSOLD", "RESTORE_UNSOLD"}:
        if candidate["state"] != ("ACTIVE" if kind == "MARK_UNSOLD" else "UNSOLD"):
            raise error("auction_draft_candidate_state_mismatch")
    if kind in {"ASSIGN_UNSOLD", "RESTORE_UNSOLD"}:
        allowed = {"UNSOLD", "REVERTED"} if kind == "RESTORE_UNSOLD" else {"UNSOLD"}
        old_lot = next(
            (r for r in reversed(lots) if r["target_user_id"] == user and r["status"] in allowed),
            None,
        )
        if old_lot is None:
            raise error("auction_draft_unsold_lot_not_found")
        target = old_lot
    if kind == "RESTORE_UNSOLD":
        await lot(c, p, lots, user)
        await action(c, p, actions, kind, user, lot=p["lot_id"])
        await transition(c, p, user, "UNSOLD", "ACTIVE")
        return "BIDDING"
    assert target is not None
    if kind == "MARK_UNSOLD":
        await action(c, p, actions, kind, user, lot=target["id"])
        await close_lot(c, p, target["id"], "UNSOLD")
        await transition(c, p, user, "ACTIVE", "UNSOLD")
        return "BIDDING"
    team_id = uuid.UUID(body["teamId"])
    team = find(teams, "id", team_id, "auction_draft_team_not_found")
    occupied = {r["roster_slot"] for r in members if r["team_id"] == team_id}
    slot = next((slot for slot in range(1, 6) if slot not in occupied), None)
    if slot is None:
        raise error("auction_draft_team_full")
    bid: Any = body.get("points")
    if kind == "AWARD_BID" and not 0 <= bid <= team["current_points"]:
        raise error(
            "invalid_auction_draft_bid_points",
            {"currentPoints": team["current_points"], "submittedPoints": bid},
        )
    await action(c, p, actions, kind, user, team_id, target["id"], bid)
    await change(
        c,
        sql.MEMBER,
        p["guild"],
        p["id"],
        team_id,
        p["member_id"],
        user,
        slot,
        "BID" if kind == "AWARD_BID" else "MANUAL",
        candidate["declared_lane"],
        candidate["mmr_at_start"],
        candidate["snapshot_origin"],
        p["action_id"],
        kind,
        p["now"],
    )
    if kind == "ASSIGN_UNSOLD":
        await change(
            c,
            "UPDATE auction_lots SET status='ASSIGNED' WHERE guild_id=:p1 "
            "AND tournament_id=:p2 AND id=:p3 AND status='UNSOLD'",
            p["guild"],
            p["id"],
            target["id"],
        )
    else:
        await close_lot(c, p, target["id"], "AWARDED" if kind == "AWARD_BID" else "ASSIGNED")
    await transition(c, p, user, "UNSOLD" if kind == "ASSIGN_UNSOLD" else "ACTIVE", "ASSIGNED")
    if kind == "AWARD_BID":
        await points(c, p, team, team["current_points"] - bid)
    full = bool(teams) and all(
        sum(r["team_id"] == t["id"] for r in members) + int(t["id"] == team_id) == 5 for t in teams
    )
    return "BRACKET_SETUP" if full else "BIDDING"
