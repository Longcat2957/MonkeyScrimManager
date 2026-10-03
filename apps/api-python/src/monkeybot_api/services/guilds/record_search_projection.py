"""Assemble paged series and tournaments without exposing storage identities."""

import math
from collections import defaultdict
from typing import Any
from uuid import UUID

from monkeybot_api.core.time import timestamp
from monkeybot_api.schemas.guilds.record_search import (
    Game,
    Lineup,
    Series,
    SeriesDetails,
    Tournament,
)

STAGES = ("SEMIFINAL_A", "SEMIFINAL_B", "FINAL")


def participant_side(team: str, preset: str) -> str:
    match team, preset:
        case ("TEAM_1", "DEFAULT") | ("TEAM_2", "SWAPPED"):
            return "BLUE"
        case ("TEAM_2", "DEFAULT") | ("TEAM_1", "SWAPPED"):
            return "RED"
        case _:
            raise ValueError("invalid participant side")


def games_by_top(
    bases: list[dict[str, Any]],
    lineup_rows: list[dict[str, Any]],
    ban_rows: list[dict[str, Any]],
) -> dict[UUID, list[tuple[UUID, Game]]]:
    if len({row["id"] for row in bases}) != len(bases):
        raise ValueError("duplicate record game")
    lineups: dict[UUID, dict[str, list[Lineup]]] = defaultdict(lambda: {"BLUE": [], "RED": []})
    for row in lineup_rows:
        side = participant_side(row["team"], row["side_preset"])
        lineups[row["game_id"]][side].append(Lineup.model_validate(row))
    bans: dict[UUID, dict[str, list[str]]] = defaultdict(lambda: {"BLUE": [], "RED": []})
    for row in ban_rows:
        if not 1 <= row["slot"] <= 5 or row["side"] not in ("BLUE", "RED"):
            raise ValueError("invalid game ban")
        bans[row["game_id"]][row["side"]].append(row["champion_id"])
    grouped: dict[UUID, list[tuple[UUID, Game]]] = defaultdict(list)
    for row in bases:
        if (row["mmr_before"] is None) != (row["mmr_after"] is None) or row["parent_id"] is None:
            raise ValueError("incomplete game result")
        teams = lineups.pop(row["id"], {"BLUE": [], "RED": []})
        if (
            len(teams["BLUE"]) != 5
            or len(teams["RED"]) != 5
            or sum(member.is_target for team in teams.values() for member in team) != 1
        ):
            raise ValueError("invalid confirmed game lineup")
        game_bans = bans.pop(row["id"], {"BLUE": [], "RED": []})
        game = Game.model_validate(
            {
                **row,
                "id": str(row["id"]),
                "played_at": timestamp(row["played_at"]),
                "target": {
                    "side": participant_side(row["target_team"], row["side_preset"]),
                    "lane": row["target_lane"],
                    "champion_id": row["target_champion_id"],
                    "mmr_before": row["mmr_before"],
                    "mmr_after": row["mmr_after"],
                },
                "blue_team": teams["BLUE"],
                "red_team": teams["RED"],
                "blue_bans": game_bans["BLUE"],
                "red_bans": game_bans["RED"],
            }
        )
        grouped[row["top_id"]].append((row["parent_id"], game))
    if lineups or bans:
        raise ValueError("unmatched game details")
    return grouped


def aggregate(games: list[Game]) -> dict[str, Any]:
    wins = sum(game.winning_side == game.target.side for game in games)
    # Match Rust f64's sequential sum, including its negative-zero starting value.
    delta: float | None = -0.0
    for game in games:
        before, after = game.target.mmr_before, game.target.mmr_after
        if before is None or after is None:
            delta = None
            break
        assert delta is not None
        delta += after - before
    if delta is not None and not math.isfinite(delta):
        raise ValueError("nonfinite aggregate MMR delta")
    return {"wins": wins, "losses": len(games) - wins, "mmr_delta": delta}


def series_details(row: dict[str, Any], games: list[Game]) -> dict[str, Any]:
    return {
        **row,
        "id": str(row["id"]),
        "played_at": timestamp(row["played_at"]),
        **aggregate(games),
        "games": games,
    }


def assemble(
    heads: list[dict[str, Any]],
    bases: list[dict[str, Any]],
    lineups: list[dict[str, Any]],
    bans: list[dict[str, Any]],
    meta_rows: list[dict[str, Any]],
    roster_rows: list[dict[str, Any]],
) -> list[Series | Tournament]:
    grouped = games_by_top(bases, lineups, bans)
    metadata: dict[UUID, list[dict[str, Any]]] = defaultdict(list)
    rosters: dict[UUID, list[Lineup]] = defaultdict(list)
    for row in meta_rows:
        if row["stage"] not in STAGES:
            raise ValueError("invalid tournament stage")
        metadata[row["tournament_id"]].append(row)
    for row in roster_rows:
        rosters[row["tournament_id"]].append(Lineup.model_validate({**row, "champion_id": None}))
    records: list[Series | Tournament] = []
    for head in heads:
        game_pairs = grouped.pop(head["id"], [])
        if not game_pairs:
            raise ValueError("record has no confirmed games")
        games = [game for _, game in game_pairs]
        if head["kind"] == "SERIES" and head["scope"] in ("REGULAR", "AUCTION"):
            if head["scope"] == "AUCTION" and head["stage"] not in STAGES:
                raise ValueError("invalid auction series stage")
            records.append(
                Series.model_validate(
                    {
                        **series_details(head, games),
                        "source": head["scope"],
                        "participant_count": 10,
                    }
                )
            )
        elif head["kind"] == "TOURNAMENT" and head["scope"] == "TOURNAMENT":
            metas = metadata.pop(head["id"], [])
            if not metas:
                raise ValueError("tournament has no series")
            by_match: dict[UUID, list[Game]] = defaultdict(list)
            for parent, game in game_pairs:
                by_match[parent].append(game)
            series = []
            for meta in metas:
                match_games = by_match.pop(meta["id"], [])
                if not match_games:
                    raise ValueError("tournament series has no confirmed games")
                series.append(SeriesDetails.model_validate(series_details(meta, match_games)))
            if by_match:
                raise ValueError("unmatched tournament games")
            roster = rosters.pop(head["id"], [])
            if len(roster) != 5 or sum(member.is_target for member in roster) != 1:
                raise ValueError("invalid tournament roster")
            if (head["spent_points"] is not None and head["spent_points"] < 0) or (
                head["placement"] is not None and not 1 <= head["placement"] <= 3
            ):
                raise ValueError("invalid tournament result")
            records.append(
                Tournament.model_validate(
                    {
                        **head,
                        "id": str(head["id"]),
                        "played_at": timestamp(head["played_at"]),
                        "participant_count": 20,
                        **aggregate(games),
                        "roster": roster,
                        "series": series,
                    }
                )
            )
        else:
            raise ValueError("invalid record kind and scope")
    if grouped or metadata or rosters:
        raise ValueError("unmatched record details")
    return records
