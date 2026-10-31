import uuid
from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

import monkeybot_api.repositories.auctions.recruitment_queries as auction_queries
import monkeybot_api.repositories.auctions.tournament_queries as tournament_queries
from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.serialization import mapped
from monkeybot_api.db.queries import one, params
from monkeybot_api.repositories.auctions.recruitments import rows, rules
from monkeybot_api.schemas.auctions.bracket import SLOTS, error

TOURNAMENT = """
    SELECT id,season_id,display_number,title,format,semifinal_best_of,final_best_of,
        hard_fearless,ended_early,status,revision,bracket_locked_at
    FROM auction_tournaments WHERE guild_id=:p1 AND id=:p2 AND deleted_at IS NULL
"""


SLOT_ROWS = """
    SELECT slot_key,team_id FROM auction_bracket_slots
    WHERE guild_id=:p1 AND tournament_id=:p2 ORDER BY slot_key
"""


ACTIVE_MATCHES = """
    SELECT id FROM auction_matches WHERE guild_id=:p1 AND tournament_id=:p2
        AND deleted_at IS NULL AND status <> 'CANCELLED' ORDER BY id FOR UPDATE
"""


RESULT_ARTIFACTS = """
    SELECT EXISTS (SELECT 1 FROM auction_matches m
        WHERE m.guild_id=:p1 AND m.tournament_id=:p2
            AND m.deleted_at IS NULL AND m.status <> 'CANCELLED'
            AND (EXISTS (SELECT 1 FROM games g
                         WHERE g.guild_id=m.guild_id AND g.auction_match_id=m.id)
                 OR EXISTS (SELECT 1 FROM game_inputs i
                            WHERE i.guild_id=m.guild_id AND i.auction_match_id=m.id)))
"""


MATCHES = """
    SELECT m.id,m.match_key,m.best_of,m.hard_fearless,m.left_team_id,m.right_team_id,
        m.status,m.ended_early,m.revision,m.winner_team_id,m.lineups_locked_at,
        count(g.id) FILTER (WHERE (g.side_preset='DEFAULT' AND g.winning_side='BLUE')
            OR (g.side_preset='SWAPPED' AND g.winning_side='RED')) AS left_score,
        count(g.id) FILTER (WHERE (g.side_preset='DEFAULT' AND g.winning_side='RED')
            OR (g.side_preset='SWAPPED' AND g.winning_side='BLUE')) AS right_score
    FROM auction_matches m LEFT JOIN games g
        ON g.guild_id=m.guild_id AND g.auction_match_id=m.id AND g.status='CONFIRMED'
    WHERE m.guild_id=:p1 AND m.tournament_id=:p2
        AND m.deleted_at IS NULL AND m.status <> 'CANCELLED'
    GROUP BY m.id,m.match_key,m.best_of,m.hard_fearless,m.left_team_id,m.right_team_id,
        m.status,m.ended_early,m.revision,m.winner_team_id,m.lineups_locked_at
    ORDER BY CASE m.match_key WHEN 'SEMIFINAL_A' THEN 0 WHEN 'SEMIFINAL_B' THEN 1
             WHEN 'FINAL' THEN 2 ELSE 3 END,m.id
"""


async def snapshot(
    c: AsyncConnection, guild: str, identifier: uuid.UUID, viewer: str
) -> dict[str, Any]:
    tournament = await one(c, TOURNAMENT, guild, identifier)
    if tournament is None:
        raise error("auction_tournament_not_found")
    status = tournament["status"]
    if status not in {"BRACKET_SETUP", "IN_GAME", "COMPLETED", "CANCELLED"}:
        raise error("auction_tournament_bracket_unavailable", {"currentStatus": status})
    configured_rules = rules(tournament)
    teams = await rows(c, tournament_queries.TEAMS, guild, identifier)
    members = await rows(c, tournament_queries.MEMBERS, guild, identifier)
    slots = await rows(c, SLOT_ROWS, guild, identifier)
    slots_by_key = {r["slot_key"]: r["team_id"] for r in slots}
    if len(slots_by_key) != len(slots) or set(slots_by_key) - set(SLOTS):
        raise ApiError.from_status(500)
    matches = await rows(c, MATCHES, guild, identifier)
    for match in matches:
        if (
            match["match_key"] not in {"SEMIFINAL_A", "SEMIFINAL_B", "FINAL"}
            or match["status"]
            not in {"LINEUP_PENDING", "READY", "IN_PROGRESS", "COMPLETED", "CANCELLED"}
            or match["left_score"] < 0
            or match["right_score"] < 0
        ):
            raise ApiError.from_status(500)
    operator = bool(await c.scalar(text(auction_queries.OPERATOR), params(guild, viewer)))
    artifacts = await c.scalar(text(RESULT_ARTIFACTS), params(guild, identifier))
    revertible = await c.scalar(text(tournament_queries.REVERTIBLE), params(guild, identifier))
    locked = tournament["bracket_locked_at"] is not None
    editable = operator and status == "BRACKET_SETUP" and not locked and not matches
    return {
        "tournament": {
            **mapped(tournament, "id title display_number format status revision"),
            "rules": configured_rules,
            "href": f"/g/{guild}/auction/tournaments/{identifier}/bracket",
        },
        "teams": [
            {
                **mapped(t, "id team_index team_name initial_points current_points"),
                "members": [
                    {
                        "teamMemberId": str(r["id"]),
                        **mapped(
                            r,
                            "user_id display_name declared_lane mmr_at_acquisition "
                            "snapshot_origin acquired_via roster_slot",
                        ),
                    }
                    for r in members
                    if r["team_id"] == t["id"]
                ],
            }
            for t in teams
        ],
        "bracket": {
            "locked": locked,
            "slots": [
                {
                    "slotKey": key,
                    "teamId": str(slots_by_key[key]) if slots_by_key.get(key) is not None else None,
                }
                for key in SLOTS
            ],
            "matches": [
                {
                    **mapped(
                        m,
                        "id match_key best_of hard_fearless left_team_id right_team_id "
                        "status revision left_score right_score winner_team_id",
                    ),
                    "lineupsLocked": m["lineups_locked_at"] is not None,
                }
                for m in matches
            ],
        },
        "permissions": {
            "canManageBracket": bool(editable),
            "canManageMatches": False,
            "canRecoverBracket": bool(
                operator
                and locked
                and matches
                and status in {"BRACKET_SETUP", "IN_GAME"}
                and not artifacts
            ),
            "canRevertDraft": bool(editable and revertible),
            "canCancel": operator and status in {"BIDDING", "BRACKET_SETUP", "IN_GAME"},
        },
    }
