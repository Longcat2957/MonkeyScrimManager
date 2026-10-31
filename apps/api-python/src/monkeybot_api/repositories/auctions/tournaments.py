import math
import re
import uuid
from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

import monkeybot_api.repositories.auctions.recruitment_queries as auction_queries
import monkeybot_api.repositories.auctions.tournament_queries as sql
from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.serialization import mapped
from monkeybot_api.core.time import timestamp
from monkeybot_api.db.queries import one, params
from monkeybot_api.domain.auctions.tournament_errors import error
from monkeybot_api.integrations.riot.cache import validated
from monkeybot_api.repositories.auctions.recruitments import rows, rules
from monkeybot_api.repositories.recruitments.entries import compact_tier
from monkeybot_api.schemas.users import LANES


async def profile(
    c: AsyncConnection, guild: str, season: uuid.UUID, target: dict[str, Any]
) -> dict[str, Any]:
    user = target["userId"]
    row = await one(c, sql.PROFILE, guild, user)
    if row is None:
        raise ApiError.from_status(500)
    found = await rows(c, sql.LANE_MMR, guild, season, user)
    lane_rows = {r["lane"]: r for r in found}
    if len(lane_rows) != len(found) or any(
        not math.isfinite(r["mmr"]) or not 0 <= r["wins"] <= r["games_played"] for r in found
    ):
        raise ApiError.from_status(500)
    lanes = [
        {
            "lane": lane,
            "mmr": lane_rows.get(lane, {}).get("mmr", 1500.0),
            "games": lane_rows.get(lane, {}).get("games_played", 0),
            "wins": lane_rows.get(lane, {}).get("wins", 0),
        }
        for lane in LANES
    ]
    games, wins = sum(r["games"] for r in lanes), sum(r["wins"] for r in lanes)
    if games >= 2**31 or wins >= 2**31:
        raise ApiError.from_status(500)
    lane = target["declaredLane"] or row["main_lane"]
    champions = [r["champion_id"] for r in await rows(c, sql.CHAMPIONS, user, lane)] if lane else []
    records = await rows(c, sql.CHAMPION_RECORDS, guild, season, user)
    recent = await rows(c, sql.RECENT_GAMES, guild, season, user)
    for r in [*records, *recent]:
        champion = r["champion_id"]
        if champion is not None and not 1 <= len(champion.encode()) <= 64:
            raise ApiError.from_status(500)
    for r in records:
        if not 0 <= r["wins"] <= r["games"] < 2**31:
            raise ApiError.from_status(500)
    riot = None
    cached = validated(row["riot_snapshot"])
    if cached is not None and row["riot_refreshed_at"] is not None:
        targets = [
            next(p for p in g.participants if p.puuid == g.targetPuuid)
            for g in cached.recentMatches
        ]
        riot = {
            "games": len(targets),
            "wins": sum(p.win for p in targets),
            "championIds": [r.championName for r in cached.championAggregates[:3]],
            "refreshedAt": timestamp(row["riot_refreshed_at"]),
        }
    avatar = row["avatar_hash"]
    return {
        "avatarUrl": f"https://cdn.discordapp.com/avatars/{user}/{avatar}.webp?size=128"
        if avatar is not None and re.fullmatch(r"[0-9a-fA-F_]{1,128}", avatar)
        else None,
        "profileIconId": row["profile_icon_id"],
        "riotTier": compact_tier(row["solo_rank_tier"], row["solo_rank_division"]),
        "mainLane": row["main_lane"],
        "summary": {"games": games, "wins": wins, "losses": games - wins},
        "laneMmr": lanes,
        "championIds": champions,
        "championRecords": [
            {
                "championId": r["champion_id"],
                "games": r["games"],
                "wins": r["wins"],
                "losses": r["games"] - r["wins"],
            }
            for r in records
        ],
        "recentGames": [
            {
                "id": str(r["id"]),
                "playedAt": timestamp(r["played_at"]),
                "result": "WIN" if r["won"] else "LOSS",
                "lane": r["lane"],
                "championId": r["champion_id"],
            }
            for r in recent
        ],
        "riot": riot,
    }


async def snapshot(
    c: AsyncConnection, guild: str, identifier: uuid.UUID, viewer: str
) -> dict[str, Any]:
    tournament = await one(c, sql.TOURNAMENT, guild, identifier)
    if tournament is None:
        raise error("auction_tournament_not_found")
    members = await rows(c, sql.MEMBERS, guild, identifier)
    teams = []
    for team in await rows(c, sql.TEAMS, guild, identifier):
        teams.append(
            {
                **mapped(
                    team, "id team_index team_name initial_points current_points point_revision"
                ),
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
                    if r["team_id"] == team["id"]
                ],
            }
        )
    candidates = [
        mapped(
            r,
            "user_id display_name declared_lane mmr_at_start "
            "snapshot_origin queue_position state draw_count",
        )
        for r in await rows(c, sql.CANDIDATES, guild, identifier)
    ]
    active = [r for r in candidates if r["state"] == "ACTIVE"]
    active_lots = await rows(c, sql.ACTIVE_LOTS, guild, identifier)
    if (
        len(active) > 1
        or len(active_lots) > 1
        or ([r["userId"] for r in active] != [r["target_user_id"] for r in active_lots])
    ):
        raise ApiError.from_status(500)
    recent = await c.scalar(text(sql.RECENT_ACTION), params(guild, identifier))
    operator = bool(await c.scalar(text(auction_queries.OPERATOR), params(guild, viewer)))
    artifacts = await c.scalar(text(sql.ARTIFACTS), params(guild, identifier))
    revertible = await c.scalar(text(sql.REVERTIBLE), params(guild, identifier))
    status = tournament["status"]
    target = active[0] if active else None
    return {
        "tournament": {
            **mapped(
                tournament, "id source_recruitment_id display_number title format status revision"
            ),
            "rules": rules(tournament),
            "href": f"/g/{guild}/auction/tournaments/{identifier}/draft",
        },
        "teams": teams,
        "candidates": candidates,
        "draft": {
            "currentTarget": target,
            "currentTargetProfile": await profile(c, guild, tournament["season_id"], target)
            if target
            else None,
            "unsold": [r for r in candidates if r["state"] == "UNSOLD"],
            "recentAction": {"kind": recent} if recent else None,
        },
        "permissions": {
            "canManageDraft": operator and status == "BIDDING" and not artifacts,
            "canManageBracket": False,
            "canRevertLast": bool(
                operator
                and revertible
                and not artifacts
                and (
                    status == "BIDDING"
                    or (status == "BRACKET_SETUP" and tournament["bracket_locked_at"] is None)
                )
            ),
            "canCancel": operator and status in {"BIDDING", "BRACKET_SETUP", "IN_GAME"},
        },
    }
