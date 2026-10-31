"""Auction result mutations, serialized tournament -> match -> locked lineup."""

import uuid
from typing import Any

from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError
from monkeybot_api.db.queries import params
from monkeybot_api.repositories.auctions import game_commands as storage
from monkeybot_api.repositories.auctions.games import final_match, games
from monkeybot_api.repositories.auctions.matches import (
    downstream_blocks_revert,
    semifinal_lineups_ready,
    snapshot,
)
from monkeybot_api.repositories.auctions.recruitments import snapshot as source_snapshot
from monkeybot_api.repositories.effects import audit, complete, invalidate, notification
from monkeybot_api.repositories.guild_access import require_operator
from monkeybot_api.repositories.idempotency import acquire
from monkeybot_api.repositories.series.snapshots import score, winner
from monkeybot_api.schemas.auctions.bracket import error as guild_error
from monkeybot_api.schemas.auctions.games import error
from monkeybot_api.schemas.auctions.lineups import error as lineup_error
from monkeybot_api.services.auctions.lineups import validate_lineups
from monkeybot_api.services.auctions.mmr import void_game
from monkeybot_api.services.auctions.notifications import reconcile, result_notification
from monkeybot_api.services.series.mmr import apply_mmr
from monkeybot_api.services.series.pickban import replace


async def submit(
    c: AsyncConnection,
    p: dict[str, Any],
    m: dict[str, Any],
    roster: list[dict[str, Any]],
    body: dict[str, Any],
) -> uuid.UUID | None:
    number = len(await games(c, p)) + 1
    if number > m["best_of"]:
        raise error("auction_match_result_not_editable", {"currentStatus": "COMPLETED"})
    if number != body["setNumber"]:
        raise error(
            "auction_match_set_not_current",
            {
                "expectedSetNumber": number,
                "actualSetNumber": body["setNumber"],
            },
        )
    v = {**p, "number": number, "season": m["season_id"]}
    drafts = await storage.lock_draft_input(c, v)
    if not drafts:
        drafts = await storage.create_draft_input(c, {**v, "input": uuid.uuid7()})
    draft = drafts[0]
    v["input"] = draft["id"]
    try:
        await replace(c, v, draft, body, {r["user_id"] for r in roster})
    except ApiError as exc:
        if exc.code == "invalid_series_game_result":
            raise error("invalid_auction_match_game_request") from None
        raise
    picks = await storage.draft_picks(c, p)
    bans = await storage.draft_bans(c, p)
    seen = set()
    for r in picks + bans:
        champion = r["champion_id"]
        if champion in seen:
            raise error("duplicate_auction_match_game_champion", {"championId": champion})
        seen.add(champion)
    if m["hard_fearless"]:
        used = {r["champion_id"] for r in await storage.confirmed_champions(c, p)}
        for r in picks:
            if r["champion_id"] in used:
                raise error(
                    "auction_match_hard_fearless_champion_used",
                    {
                        "championId": r["champion_id"],
                    },
                )
    v.update(game=uuid.uuid7(), **body)
    await storage.create_confirmed_game(c, v)
    for r in roster:
        await storage.add_game_participant(c, {**v, **r, "champion": body["picks"][r["user_id"]]})
    for side, slots in body["bans"].items():
        for slot, champion in enumerate(slots, 1):
            if champion is not None:
                await storage.add_game_ban(
                    c, {**v, "side": side, "slot": slot, "champion": champion}
                )
    await apply_mmr(c, v, roster, winner(body["sidePreset"], body["winner"]))
    await storage.submit_input(c, v)
    sc = score(await games(c, p))
    winning = (
        m["left_team_id"]
        if sc["team1"] >= m["best_of"] // 2 + 1
        else (m["right_team_id"] if sc["team2"] >= m["best_of"] // 2 + 1 else None)
    )
    await storage.update_match_result(
        c,
        {
            **p,
            "status": "COMPLETED" if winning else "IN_PROGRESS",
            "winning": winning,
            "ended": p["now"] if winning else None,
        },
    )
    return winning


async def propagate(
    c: AsyncConnection,
    p: dict[str, Any],
    m: dict[str, Any],
    winning: uuid.UUID,
) -> None:
    final = await final_match(c, p)
    column = "left_team_id" if m["match_key"] == "SEMIFINAL_A" else "right_team_id"
    v = {**p, "final": final["id"], "winning": winning}
    existing = await storage.count_final_lineups(c, v)
    if (
        final["status"] != "LINEUP_PENDING"
        or final["lineups_locked_at"] is not None
        or (final[column] is not None or existing)
    ):
        raise error("auction_match_downstream_work_exists")
    copied = await storage.copy_winner_lineups(c, v)
    if copied.rowcount != 5:
        raise ApiError.from_status(500)
    await storage.set_final_team(c, v, column=column)


async def undo_propagation(c: AsyncConnection, p: dict[str, Any], m: dict[str, Any]) -> None:
    final = await final_match(c, p)
    column = "left_team_id" if m["match_key"] == "SEMIFINAL_A" else "right_team_id"
    if final[column] != m["winner_team_id"] or m["winner_team_id"] is None:
        raise ApiError.from_status(500)
    if await downstream_blocks_revert(
        c, p["guild"], p["tournament"], m["match_key"], m["winner_team_id"]
    ):
        raise error("auction_match_downstream_work_exists")
    v = {**p, "final": final["id"], "winning": m["winner_team_id"]}
    removed = await storage.remove_final_lineups(c, v)
    if removed.rowcount != 5:
        raise ApiError.from_status(500)
    await storage.clear_final_team(c, v, column=column)


async def mutate(
    c: AsyncConnection,
    p: dict[str, Any],
    action: str,
    body: dict[str, Any],
) -> dict[str, Any]:
    state = await storage.lock_guild(c, params(p["guild"]))
    if state is None:
        raise guild_error("not_found")
    if state != "ACTIVE":
        raise guild_error("guild_inactive", {"currentStatus": state})
    role = await storage.lock_config(c, params(p["guild"]))
    if role is None:
        raise guild_error("guild_setup_required")
    member = await storage.lock_member_status(c, p)
    if member != "ACTIVE":
        raise error("guild_membership_required")
    try:
        await require_operator(c, p, role, None)
        replay = await acquire(c, p, False)
    except ApiError as exc:
        if exc.code in {"operator_required", "idempotency_conflict", "idempotency_in_progress"}:
            raise error(exc.code, exc.details) from None
        raise
    if replay is not None:
        return replay
    tournament_id = await storage.find_tournament_id(c, p)
    if tournament_id is None:
        raise error("auction_match_not_found")
    p = {**p, "tournament": tournament_id}
    tournaments = await storage.lock_tournament(c, p)
    if not tournaments:
        raise guild_error("auction_tournament_not_found")
    t = tournaments[0]
    allowed = {"IN_GAME", "COMPLETED"} if action == "last" else {"IN_GAME"}
    if t["status"] not in allowed or t["ended_early"]:
        raise error("auction_match_tournament_not_ready")
    source = None
    if action == "complete":
        sources = await storage.lock_source_recruitment(
            c, {**p, "source": t["source_recruitment_id"]}
        )
        if not sources:
            raise ApiError.from_status(500)
        source = sources[0]
    matches = await storage.lock_match(c, p)
    if not matches:
        raise error("auction_match_not_found")
    m = matches[0]
    if m["revision"] != body["expectedRevision"]:
        raise error(
            "auction_match_revision_conflict",
            {
                "expectedRevision": body["expectedRevision"],
                "actualRevision": m["revision"],
            },
        )
    left, right = m["left_team_id"], m["right_team_id"]
    if left is None or right is None:
        raise error("auction_match_teams_pending")
    if left == right:
        raise ApiError.from_status(500)
    if m["lineups_locked_at"] is None:
        raise lineup_error("auction_match_lineup_incomplete")
    if action == "submit" and t["format"] == "TWENTY_PERSON" and m["match_key"] != "FINAL":
        await storage.lock_active_matches(c, p)
        if not await semifinal_lineups_ready(c, p["guild"], tournament_id):
            raise error("auction_match_semifinal_lineups_incomplete")
    allowed_match = (
        {"READY", "IN_PROGRESS"}
        if action == "submit"
        else ({"READY", "IN_PROGRESS", "COMPLETED"} if action == "last" else {"IN_PROGRESS"})
    )
    if m["status"] not in allowed_match:
        raise error("auction_match_result_not_editable", {"currentStatus": m["status"]})
    members = await storage.lock_members(c, p["guild"], tournament_id)
    lineups = await storage.lock_lineups(c, p["guild"], tournament_id, p["id"])
    validate_lineups(lineups, members, left, right, complete=True)
    roster = [
        {
            "user_id": r["user_id"],
            "lane": r["lane"],
            "team": "TEAM_1" if r["team_id"] == left else "TEAM_2",
        }
        for r in lineups
    ]
    before = await snapshot(c, p["guild"], p["id"], p["actor_id"])
    completed_revision = None
    source_revision = None
    if action == "submit":
        winning = await submit(c, p, m, roster, body)
        if winning is not None:
            if m["match_key"] == "FINAL":
                completed_revision = await storage.complete_tournament(c, {**p, "winning": winning})
                if completed_revision is None:
                    raise error("auction_match_tournament_not_ready")
            elif t["format"] == "TWENTY_PERSON" and m["match_key"] in {
                "SEMIFINAL_A",
                "SEMIFINAL_B",
            }:
                await propagate(c, p, m, winning)
            else:
                raise ApiError.from_status(500)
    elif action == "last":
        was_completed = m["status"] == "COMPLETED"
        if was_completed and m["match_key"] == "FINAL":
            await reconcile(c, p)
        records = await storage.lock_latest_confirmed_game(c, p)
        if not records:
            raise error("auction_match_no_confirmed_games")
        await void_game(c, p, records[0])
        remaining = len(await games(c, p))
        await storage.reopen_match(c, {**p, "status": "IN_PROGRESS" if remaining else "READY"})
        if was_completed:
            if m["match_key"] == "FINAL":
                reopened = await storage.reopen_tournament(c, p)
                if reopened.rowcount != 1:
                    raise error("auction_match_tournament_not_ready")
            else:
                await undo_propagation(c, p, m)
    else:
        records = await games(c, p)
        sc = score(records)
        if (
            source is None
            or m["match_key"] != "FINAL"
            or m["best_of"] not in {3, 5}
            or len(records) != 1
            or (sc["team1"], sc["team2"]) not in {(1, 0), (0, 1)}
            or t["final_best_of"] != m["best_of"]
            or source["final_best_of"] != m["best_of"]
            or source["status"] != "CONVERTED"
        ):
            raise error("auction_final_bo1_completion_unavailable")
        winning = left if sc["team1"] == 1 else right
        v = {**p, "winning": winning, "source": t["source_recruitment_id"]}
        source_revision = await storage.set_source_final_best_of_one(c, v)
        await storage.complete_match_as_best_of_one(c, v)
        completed_revision = await storage.complete_tournament_as_best_of_one(c, v)
    event = {
        "submit": "AUCTION_MATCH_GAME_CONFIRMED",
        "last": "AUCTION_MATCH_GAME_REVERTED",
        "complete": "AUCTION_FINAL_COMPLETED_AS_BO1",
    }[action]
    if completed_revision is not None:
        await result_notification(c, p, completed_revision)
    if source_revision is not None:
        source_result = await source_snapshot(
            c, p["guild"], t["source_recruitment_id"], p["actor_id"]
        )
        r = source_result["recruitment"]
        if r["channelId"] is not None and r["messageId"] is not None:
            await notification(
                c,
                p,
                "AUCTION_RECRUITMENT_MESSAGE_UPDATE",
                r["channelId"],
                f"auction-recruitment:{r['id']}:message:update:{r['revision']}",
                {
                    "schemaVersion": 2,
                    "auctionRecruitmentId": r["id"],
                    "snapshot": source_result,
                },
            )
        await invalidate(
            c, p, "AUCTION_RECRUITMENT", str(t["source_recruitment_id"]), source_revision, event
        )
    result = await snapshot(c, p["guild"], p["id"], p["actor_id"])
    await audit(c, p, event, "AUCTION_MATCH_GAME", str(p["id"]), before, result)
    await invalidate(c, p, "AUCTION_MATCH", str(p["id"]), result["match"]["revision"], event)
    if completed_revision is not None:
        await invalidate(c, p, "AUCTION_TOURNAMENT", str(tournament_id), completed_revision, event)
    await complete(c, p, result, 201 if action == "submit" else 200)
    return result
