"""Frozen auction match read model, shared by lineup and game commands."""

import uuid
from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

import monkeybot_api.repositories.auctions.recruitment_queries as auction_queries
import monkeybot_api.repositories.auctions.tournament_queries as tournament_queries
from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.serialization import mapped
from monkeybot_api.db.queries import one, params
from monkeybot_api.repositories.auctions.recruitments import rows, shape
from monkeybot_api.repositories.guild_access import require_guild_access
from monkeybot_api.repositories.series.snapshots import input_snapshot
from monkeybot_api.schemas.auctions.lineups import error
from monkeybot_api.schemas.users import LANES

LABELS = {"SEMIFINAL_A": "준결승 A", "SEMIFINAL_B": "준결승 B", "FINAL": "결승"}
STATUSES = {"LINEUP_PENDING", "READY", "IN_PROGRESS", "COMPLETED", "CANCELLED"}


def complete_lineup(lineups: list[dict[str, Any]], left: Any, right: Any) -> bool:
    return (
        len(lineups) == 10
        and {(r["teamId"], r["lane"]) for r in lineups}
        == {(str(t), lane) for t in (left, right) for lane in LANES}
        and len({r["teamMemberId"] for r in lineups}) == 10
        and len({r["userId"] for r in lineups}) == 10
    )


async def has_result_artifacts(c: AsyncConnection, guild: str, match_id: uuid.UUID) -> bool:
    return bool(
        await c.scalar(
            text("""
            SELECT EXISTS (SELECT 1 FROM games WHERE guild_id=:p1 AND auction_match_id=:p2)
                OR EXISTS (SELECT 1 FROM game_inputs WHERE guild_id=:p1 AND auction_match_id=:p2)
            """),
            params(guild, match_id),
        )
    )


async def semifinal_lineups_ready(c: AsyncConnection, guild: str, tournament: uuid.UUID) -> bool:
    result = await c.scalar(
        text("""
        SELECT CASE WHEN t.format <> 'TWENTY_PERSON' THEN TRUE ELSE (
            SELECT count(*)=2 AND count(*) FILTER (WHERE m.lineups_locked_at IS NOT NULL)=2
            FROM auction_matches m WHERE m.guild_id=t.guild_id AND m.tournament_id=t.id
                AND m.match_key IN ('SEMIFINAL_A','SEMIFINAL_B')
                AND m.status <> 'CANCELLED' AND m.deleted_at IS NULL)
        END FROM auction_tournaments t WHERE t.guild_id=:p1 AND t.id=:p2
            AND t.deleted_at IS NULL
        """),
        params(guild, tournament),
    )
    if result is None:
        raise error("auction_tournament_not_found")
    return bool(result)


async def downstream_blocks_revert(
    c: AsyncConnection, guild: str, tournament: uuid.UUID, match_key: str, winner: uuid.UUID | None
) -> bool:
    if match_key not in {"SEMIFINAL_A", "SEMIFINAL_B"}:
        return False
    if winner is None:
        return True
    format_ = await c.scalar(
        text(
            "SELECT format FROM auction_tournaments WHERE guild_id=:p1 AND id=:p2 "
            "AND deleted_at IS NULL"
        ),
        params(guild, tournament),
    )
    if format_ is None:
        raise error("auction_tournament_not_found")
    shape(format_)
    if format_ != "TWENTY_PERSON":
        return False
    final = await one(
        c,
        "SELECT id,status,left_team_id,right_team_id,lineups_locked_at FROM auction_matches "
        "WHERE guild_id=:p1 AND tournament_id=:p2 AND match_key='FINAL' "
        "AND status <> 'CANCELLED' AND deleted_at IS NULL",
        guild,
        tournament,
    )
    if final is None:
        return True
    target = final["left_team_id" if match_key == "SEMIFINAL_A" else "right_team_id"]
    if target is None or target != winner:
        return True
    if final["lineups_locked_at"] is not None or final["status"] != "LINEUP_PENDING":
        return True
    return bool(
        await c.scalar(
            text("""
        SELECT EXISTS (SELECT 1 FROM auction_match_lineups WHERE guild_id=:p1
            AND tournament_id=:p2 AND match_id=:p3 AND team_id <> :p4)
            OR EXISTS (SELECT 1 FROM games WHERE guild_id=:p1 AND auction_match_id=:p3)
        """),
            params(guild, tournament, final["id"], target),
        )
    )


async def confirmed_games(
    c: AsyncConnection, guild: str, match_id: uuid.UUID, player_ids: set[str]
) -> list[dict[str, Any]]:
    result = []
    for game in await rows(
        c,
        "SELECT * FROM games WHERE guild_id=:p1 AND auction_match_id=:p2 "
        "AND status='CONFIRMED' ORDER BY game_number",
        guild,
        match_id,
    ):
        draft = {**game, "id": game["source_input_id"], "side_selected": True}
        item = await input_snapshot(c, {"guild": guild}, draft, game["game_number"], player_ids)
        if set(item["picks"]) != player_ids or item["winner"] is None:
            raise ApiError.from_status(500)
        item.pop("sideSelected")
        result.append({"id": str(game["id"]), **item})
    return result


async def snapshot(
    c: AsyncConnection, guild: str, match_id: uuid.UUID, viewer: str
) -> dict[str, Any]:
    await require_guild_access(c, viewer, guild)
    m = await one(
        c,
        "SELECT * FROM auction_matches WHERE guild_id=:p1 AND id=:p2 AND deleted_at IS NULL",
        guild,
        match_id,
    )
    if m is None:
        raise error("auction_match_not_found")
    left, right = m["left_team_id"], m["right_team_id"]
    if left is None or right is None:
        raise error("auction_match_teams_pending")
    if (
        left == right
        or m["match_key"] not in LABELS
        or m["status"] not in STATUSES
        or m["best_of"] not in {1, 3, 5}
        or m["revision"] < 0
        or m["winner_team_id"] not in {None, left, right}
    ):
        raise ApiError.from_status(500)
    teams = [
        t
        for t in await rows(c, tournament_queries.TEAMS, guild, m["tournament_id"])
        if t["id"] in {left, right}
    ]
    teams.sort(key=lambda t: t["id"] != left)
    if [t["id"] for t in teams] != [left, right]:
        raise ApiError.from_status(500)
    members = [
        r
        for r in await rows(c, tournament_queries.MEMBERS, guild, m["tournament_id"])
        if r["team_id"] in {left, right}
    ]
    member_users = {(r["team_id"], r["id"]): r["user_id"] for r in members}
    if (
        any(not 1 <= r["roster_slot"] <= 5 for r in members)
        or len(member_users) != len(members)
        or len({r["user_id"] for r in members}) != len(members)
    ):
        raise ApiError.from_status(500)
    lineup_rows = await rows(
        c,
        "SELECT team_id,lane,team_member_id,user_id FROM "
        "auction_match_lineups WHERE guild_id=:p1 AND tournament_id=:p2 "
        "AND match_id=:p3",
        guild,
        m["tournament_id"],
        match_id,
    )
    if (
        any(
            r["team_id"] not in {left, right}
            or r["lane"] not in LANES
            or member_users.get((r["team_id"], r["team_member_id"])) != r["user_id"]
            for r in lineup_rows
        )
        or len({(r["team_id"], r["lane"]) for r in lineup_rows}) != len(lineup_rows)
        or len({r["team_member_id"] for r in lineup_rows}) != len(lineup_rows)
        or len({r["user_id"] for r in lineup_rows}) != len(lineup_rows)
    ):
        raise ApiError.from_status(500)
    lineup_rows.sort(key=lambda r: (r["team_id"] != left, LANES.index(r["lane"])))
    lineups = [mapped(r, "team_id lane team_member_id user_id") for r in lineup_rows]
    locked = m["lineups_locked_at"] is not None
    if locked and not complete_lineup(lineups, left, right):
        raise ApiError.from_status(500)
    games = await confirmed_games(c, guild, match_id, {r["userId"] for r in lineups})
    if games and (not locked or not complete_lineup(lineups, left, right)):
        raise ApiError.from_status(500)
    left_score = sum((g["sidePreset"] == "DEFAULT") == (g["winner"] == "BLUE") for g in games)
    right_score = len(games) - left_score
    ready = m["match_key"] == "FINAL" or await semifinal_lineups_ready(c, guild, m["tournament_id"])
    artifacts = await has_result_artifacts(c, guild, match_id)
    operator = bool(await c.scalar(text(auction_queries.OPERATOR), params(guild, viewer)))
    tournament = await one(
        c,
        "SELECT status,ended_early FROM auction_tournaments WHERE guild_id=:p1 AND id=:p2",
        guild,
        m["tournament_id"],
    )
    if tournament is None:
        raise ApiError.from_status(500)
    can_mutate = operator and not tournament["ended_early"]
    blocked = (
        can_mutate
        and m["status"] == "COMPLETED"
        and await downstream_blocks_revert(
            c, guild, m["tournament_id"], m["match_key"], m["winner_team_id"]
        )
    )
    return {
        "match": {
            **mapped(
                m,
                "id tournament_id match_key best_of hard_fearless ended_early status revision "
                "left_team_id right_team_id winner_team_id",
            ),
            "label": LABELS[m["match_key"]],
            "leftScore": left_score,
            "rightScore": right_score,
            "lineupsLocked": locked,
        },
        "teams": [
            {
                **mapped(t, "id team_name"),
                "members": [
                    {
                        "teamMemberId": str(r["id"]),
                        **mapped(
                            r,
                            "user_id display_name declared_lane "
                            "mmr_at_acquisition snapshot_origin acquired_via roster_slot",
                        ),
                    }
                    for r in members
                    if r["team_id"] == t["id"]
                ],
            }
            for t in teams
        ],
        "lineups": lineups,
        "games": games,
        "permissions": {
            "canFinishTournament": operator
            and tournament["status"] in {"BIDDING", "BRACKET_SETUP", "IN_GAME"},
            "canManageLineup": can_mutate and m["status"] == "LINEUP_PENDING" and not locked,
            "canEnterResults": can_mutate
            and locked
            and ready
            and m["status"] in {"READY", "IN_PROGRESS"},
            "canUnlockLineup": can_mutate
            and m["status"] == "READY"
            and locked
            and m["lineups_locked_by"] is not None
            and not artifacts,
            "canRevertLastGame": can_mutate
            and locked
            and bool(games)
            and not blocked
            and m["status"] in {"READY", "IN_PROGRESS", "COMPLETED"},
            "canCompleteFinalAsBo1": can_mutate
            and m["match_key"] == "FINAL"
            and m["status"] == "IN_PROGRESS"
            and m["best_of"] in {3, 5}
            and len(games) == 1
            and (left_score, right_score) in {(1, 0), (0, 1)},
        },
    }
