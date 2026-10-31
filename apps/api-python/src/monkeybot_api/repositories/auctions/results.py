"""Archived/current auction results, preserving the frozen Rust read contract."""

import uuid
from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

import monkeybot_api.repositories.auctions.recruitment_queries as auction_queries
import monkeybot_api.repositories.auctions.tournament_queries as tournament_queries
from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.serialization import mapped
from monkeybot_api.db.queries import one, params
from monkeybot_api.repositories.auctions.bracket import MATCHES as BRACKET_MATCHES
from monkeybot_api.repositories.auctions.bracket import (
    RESULT_ARTIFACTS as RESULT_ARTIFACTS,
)
from monkeybot_api.repositories.auctions.bracket import TOURNAMENT as BRACKET_TOURNAMENT
from monkeybot_api.repositories.auctions.matches import complete_lineup, confirmed_games
from monkeybot_api.repositories.auctions.recruitments import rows, rules, shape

MEMBERS = """
    SELECT m.team_id,m.id AS team_member_id,m.user_id,g.display_name,m.declared_lane,
           m.acquired_via,a.points AS acquisition_points,m.roster_slot
    FROM auction_team_members m JOIN guild_members g
        ON g.guild_id=m.guild_id AND g.user_id=m.user_id
    LEFT JOIN auction_actions a ON a.guild_id=m.guild_id
        AND a.tournament_id=m.tournament_id AND a.id=m.acquisition_action_id
    WHERE m.guild_id=:p1 AND m.tournament_id=:p2 AND m.released_at IS NULL
    ORDER BY m.team_id,m.roster_slot
"""
LINEUPS = """
    SELECT match_id,team_id,lane,team_member_id,user_id FROM auction_match_lineups
    WHERE guild_id=:p1 AND tournament_id=:p2 AND match_id=ANY(:p3)
    ORDER BY match_id,team_id,CASE lane WHEN 'TOP' THEN 1 WHEN 'JUNGLE' THEN 2
        WHEN 'MID' THEN 3 WHEN 'BOTTOM' THEN 4 WHEN 'SUPPORT' THEN 5 ELSE 6 END
"""
PLAYERS = """
    SELECT m.user_id,gm.display_name,m.team_id,
        count(p.game_id) FILTER (WHERE g.status='CONFIRMED') AS games,
        count(p.game_id) FILTER (WHERE g.status='CONFIRMED' AND (
            (g.side_preset='DEFAULT' AND g.winning_side='BLUE' AND p.team='TEAM_1') OR
            (g.side_preset='DEFAULT' AND g.winning_side='RED' AND p.team='TEAM_2') OR
            (g.side_preset='SWAPPED' AND g.winning_side='RED' AND p.team='TEAM_1') OR
            (g.side_preset='SWAPPED' AND g.winning_side='BLUE' AND p.team='TEAM_2')
        )) AS wins,
        sum(l.delta) FILTER (WHERE g.status='CONFIRMED' AND l.source_type='GAME_RESULT')
            AS mmr_delta
    FROM auction_team_members m JOIN guild_members gm
        ON gm.guild_id=m.guild_id AND gm.user_id=m.user_id
    LEFT JOIN game_participants p ON p.guild_id=m.guild_id AND p.user_id=m.user_id
    LEFT JOIN games g ON g.guild_id=p.guild_id AND g.id=p.game_id
        AND g.auction_match_id=ANY(:p3)
    LEFT JOIN mmr_ledger_entries l ON l.guild_id=g.guild_id AND l.game_id=g.id
        AND l.user_id=m.user_id AND l.lane=p.lane
    WHERE m.guild_id=:p1 AND m.tournament_id=:p2 AND m.released_at IS NULL
    GROUP BY m.user_id,gm.display_name,m.team_id ORDER BY m.team_id,m.user_id
"""


def invariant() -> ApiError:
    return ApiError(500, "internal_error", "경매 내전 결과를 불러오는 중 내부 오류가 발생했습니다.")


async def snapshot(
    c: AsyncConnection, guild: str, identifier: uuid.UUID, viewer: str
) -> dict[str, Any]:
    tournament = await one(
        c,
        BRACKET_TOURNAMENT.replace("SELECT id,", "SELECT champion_team_id,id,"),
        guild,
        identifier,
    )
    if tournament is None:
        raise ApiError(404, "not_found", "요청한 경매 내전을 찾을 수 없습니다.")
    status = tournament["status"]
    if status not in {"IN_GAME", "COMPLETED", "CANCELLED"}:
        raise ApiError(
            409,
            "auction_tournament_result_unavailable",
            "진행 중, 완료 또는 취소된 경매 내전에서만 기록을 확인할 수 있습니다.",
            {"currentStatus": status},
        )
    _, team_count = shape(tournament["format"])
    configured = rules(tournament)
    teams = await rows(c, tournament_queries.TEAMS, guild, identifier)
    if len(teams) != team_count or any(
        t["team_index"] != i or not 0 <= t["current_points"] <= t["initial_points"]
        for i, t in enumerate(teams, 1)
    ):
        raise invariant()
    team_ids = {t["id"] for t in teams}
    members = await rows(c, MEMBERS, guild, identifier)
    member_users: dict[tuple[uuid.UUID, uuid.UUID], str] = {}
    users = set()
    for m in members:
        key = (m["team_id"], m["team_member_id"])
        if (
            m["team_id"] not in team_ids
            or not 1 <= m["roster_slot"] <= 5
            or (m["acquisition_points"] is not None and m["acquisition_points"] < 0)
            or key in member_users
            or m["user_id"] in users
        ):
            raise invariant()
        member_users[key] = m["user_id"]
        users.add(m["user_id"])
    result_teams = []
    for t in teams:
        roster = [
            mapped(
                m,
                "team_member_id user_id display_name declared_lane acquired_via acquisition_points",
            )
            for m in members
            if m["team_id"] == t["id"]
        ]
        if len(roster) > 5:
            raise invariant()
        result_teams.append(
            {
                **mapped(t, "id team_index team_name initial_points current_points"),
                "spentPoints": t["initial_points"] - t["current_points"],
                "members": roster,
            }
        )
    matches = await rows(
        c,
        BRACKET_MATCHES.replace(
            "AND m.status <> 'CANCELLED'", "AND (:p3 OR m.status <> 'CANCELLED')"
        ),
        guild,
        identifier,
        status == "CANCELLED",
    )
    expected_keys = ["FINAL"] if team_count == 2 else ["SEMIFINAL_A", "SEMIFINAL_B", "FINAL"]
    if (
        status != "CANCELLED"
        and not tournament["ended_early"]
        and [m["match_key"] for m in matches] != expected_keys
    ):
        raise invariant()
    match_ids = [m["id"] for m in matches]
    lineups = await rows(c, LINEUPS, guild, identifier, match_ids)
    for lineup in lineups:
        if member_users.get((lineup["team_id"], lineup["team_member_id"])) != lineup["user_id"]:
            raise invariant()
    result_matches = []
    for m in matches:
        if (
            m["status"] not in {"LINEUP_PENDING", "READY", "IN_PROGRESS", "COMPLETED", "CANCELLED"}
            or m["best_of"] not in {1, 3, 5}
            or m["left_score"] < 0
            or m["right_score"] < 0
            or any(
                m[k] is not None and m[k] not in team_ids
                for k in ("left_team_id", "right_team_id", "winner_team_id")
            )
        ):
            raise invariant()
        assigned = [
            mapped(lineup, "team_id lane team_member_id user_id")
            for lineup in lineups
            if lineup["match_id"] == m["id"]
        ]
        locked = m["lineups_locked_at"] is not None
        if locked and (
            m["left_team_id"] is None
            or m["right_team_id"] is None
            or not complete_lineup(assigned, str(m["left_team_id"]), str(m["right_team_id"]))
        ):
            raise invariant()
        games = await confirmed_games(c, guild, m["id"], {lineup["userId"] for lineup in assigned})
        if games and not locked:
            raise invariant()
        result_matches.append(
            {
                **mapped(
                    m,
                    "id match_key best_of hard_fearless ended_early status "
                    "left_team_id right_team_id left_score right_score winner_team_id",
                ),
                "lineupsLocked": locked,
                "lineups": assigned,
                "games": games,
            }
        )
    players = await rows(c, PLAYERS, guild, identifier, match_ids)
    for p in players:
        if p["team_id"] not in team_ids or not 0 <= p["wins"] <= p["games"]:
            raise invariant()
    operator = bool(await c.scalar(text(auction_queries.OPERATOR), params(guild, viewer)))
    artifacts = await c.scalar(text(RESULT_ARTIFACTS), params(guild, identifier))
    return {
        "tournament": {
            **mapped(
                tournament,
                "id title display_number format ended_early status revision champion_team_id",
            ),
            "rules": configured,
            "href": f"/g/{guild}/auction/tournaments/{identifier}/result",
        },
        "teams": result_teams,
        "matches": result_matches,
        "players": [
            {
                **mapped(p, "user_id display_name team_id games wins mmr_delta"),
                "losses": p["games"] - p["wins"],
            }
            for p in players
        ],
        "permissions": {
            "canRevertResults": operator
            and not tournament["ended_early"]
            and status in {"IN_GAME", "COMPLETED"},
            "canRecoverBracket": bool(
                operator
                and tournament["bracket_locked_at"] is not None
                and matches
                and status in {"BRACKET_SETUP", "IN_GAME"}
                and not artifacts
            ),
            "canCancel": operator and status in {"BIDDING", "BRACKET_SETUP", "IN_GAME"},
        },
    }
