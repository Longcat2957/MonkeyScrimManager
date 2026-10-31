"""Bracket snapshots and commands against the existing auction schema."""

import uuid
from typing import Any

from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError
from monkeybot_api.db.queries import params
from monkeybot_api.repositories.auctions import bracket_commands as storage
from monkeybot_api.repositories.auctions.bracket import snapshot
from monkeybot_api.repositories.auctions.recruitments import rules, shape
from monkeybot_api.repositories.effects import audit, complete, invalidate
from monkeybot_api.repositories.guild_access import require_operator
from monkeybot_api.repositories.idempotency import acquire
from monkeybot_api.schemas.auctions.bracket import SLOTS, error


async def apply(
    c: AsyncConnection,
    p: dict[str, Any],
    body: dict[str, Any],
    tournament: dict[str, Any],
    teams: list[dict[str, Any]],
    members: list[dict[str, Any]],
    slots: list[dict[str, Any]],
) -> None:
    _, team_count = shape(tournament["format"])
    configured = rules(tournament)
    allowed = SLOTS[:team_count]
    kind = body["kind"]
    if kind in {"ASSIGN_TEAM", "CLEAR_SLOT"}:
        slot = body["slotKey"]
        if slot not in allowed:
            raise error("auction_tournament_bracket_slot_unavailable")
        team_id = uuid.UUID(body["teamId"]) if kind == "ASSIGN_TEAM" else None
        if team_id is not None and team_id not in {t["id"] for t in teams}:
            raise error("auction_tournament_bracket_team_not_found")
        v = {**p, "slot": slot, "team": team_id}
        await storage.clear_slot_assignment(c, v)
        if team_id is not None:
            changed = await storage.assign_slot_team(c, v)
            if changed.rowcount != 1:
                raise ApiError.from_status(500)
    elif kind == "LOCK_BRACKET":
        team_ids = {t["id"] for t in teams}
        if (
            len(teams) != team_count
            or len(team_ids) != len(teams)
            or any(m["team_id"] not in team_ids for m in members)
            or any(sum(m["team_id"] == t for m in members) != 5 for t in team_ids)
        ):
            raise error("auction_tournament_bracket_roster_incomplete")
        assigned = set()
        for slot in slots:
            if slot["slot_key"] in allowed:
                if slot["team_id"] not in team_ids or slot["team_id"] in assigned:
                    raise error("auction_tournament_bracket_slots_incomplete")
                assigned.add(slot["team_id"])
            elif slot["team_id"] is not None:
                raise error("auction_tournament_bracket_slot_unavailable")
        if assigned != team_ids:
            raise error("auction_tournament_bracket_slots_incomplete")
        by_slot = {s["slot_key"]: s["team_id"] for s in slots}
        match_configs = [("FINAL", configured["finalBestOf"], by_slot["A_0"], by_slot["A_1"])]
        if team_count == 4:
            match_configs = [
                ("SEMIFINAL_A", configured["semifinalBestOf"], by_slot["A_0"], by_slot["A_1"]),
                ("SEMIFINAL_B", configured["semifinalBestOf"], by_slot["B_0"], by_slot["B_1"]),
                ("FINAL", configured["finalBestOf"], None, None),
            ]
        for index, (key, best_of, left, right) in enumerate(match_configs):
            await storage.create_match(
                c,
                {
                    **p,
                    "match": p["match_ids"][index],
                    "season": tournament["season_id"],
                    "key": key,
                    "best_of": best_of,
                    "fearless": configured["hardFearless"],
                    "left": left,
                    "right": right,
                },
            )
    else:
        if await storage.has_result_artifacts(c, params(p["guild"], p["id"])):
            raise error("auction_tournament_bracket_recovery_unavailable")
        changed = await storage.cancel_active_matches(c, p)
        if changed.rowcount == 0:
            raise error("auction_tournament_bracket_recovery_unavailable")


async def mutate(c: AsyncConnection, p: dict[str, Any], body: dict[str, Any]) -> dict[str, Any]:
    status = await storage.lock_guild(c, params(p["guild"]))
    if status is None:
        raise error("not_found")
    if status != "ACTIVE":
        raise error("guild_inactive", {"currentStatus": status})
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
    tournament = await storage.lock_tournament(c, p["guild"], p["id"])
    if tournament is None:
        raise error("auction_tournament_not_found")
    teams = await storage.lock_teams(c, p["guild"], p["id"])
    members = await storage.lock_members(c, p["guild"], p["id"])
    for slot in SLOTS:
        await storage.ensure_slot(c, {**p, "slot": slot})
    slots = await storage.lock_slots(c, p["guild"], p["id"])
    matches = await storage.lock_active_matches(c, p["guild"], p["id"])
    if tournament["revision"] != body["expectedRevision"]:
        raise error(
            "auction_tournament_revision_conflict",
            {
                "expectedRevision": body["expectedRevision"],
                "actualRevision": tournament["revision"],
            },
        )
    kind, status = body["kind"], tournament["status"]
    locked = tournament["bracket_locked_at"] is not None
    if kind == "UNLOCK_BRACKET":
        if status not in {"BRACKET_SETUP", "IN_GAME"} or not locked or not matches:
            raise error("auction_tournament_bracket_recovery_unavailable")
    elif status != "BRACKET_SETUP":
        raise error("auction_tournament_bracket_not_editable", {"currentStatus": status})
    elif locked or matches:
        raise error("auction_tournament_bracket_locked")
    assigned = [s["team_id"] for s in slots if s["team_id"] is not None]
    if (
        len(slots) != 4
        or {s["slot_key"] for s in slots} != set(SLOTS)
        or len(set(assigned)) != len(assigned)
    ):
        raise ApiError.from_status(500)
    before = await snapshot(c, p["guild"], p["id"], p["actor_id"])
    await apply(c, p, body, tournament, teams, members, slots)
    revision = await storage.update_bracket(c, p, kind=kind)
    result = await snapshot(c, p["guild"], p["id"], p["actor_id"])
    if revision != result["tournament"]["revision"]:
        raise ApiError.from_status(500)
    await audit(c, p, "AUCTION_BRACKET_" + kind, "AUCTION_TOURNAMENT", str(p["id"]), before, result)
    await invalidate(
        c, p, "AUCTION_TOURNAMENT", str(p["id"]), revision, "AUCTION_TOURNAMENT_BRACKET_UPDATED"
    )
    await complete(c, p, result, 200)
    return result
