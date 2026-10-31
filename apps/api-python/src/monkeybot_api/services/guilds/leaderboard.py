"""Build an active-season leaderboard from one consistent member-visible snapshot."""

import logging
import math
from itertools import groupby
from typing import Any

from monkeybot_api.core.errors import ApiError, hidden_guild
from monkeybot_api.db.database import Database
from monkeybot_api.repositories.guilds import leaderboard as repository
from monkeybot_api.schemas.guilds.leaderboard import (
    Lane,
    LeaderboardPlayer,
    LeaderboardSeason,
    LeaderboardSnapshot,
    LeaderboardStat,
)
from monkeybot_api.schemas.users import LANES

logger = logging.getLogger(__name__)


def build_players(rows: list[dict[str, Any]], viewer_id: str) -> list[LeaderboardPlayer]:
    players = []
    for user_id, grouped in groupby(rows, key=lambda row: row["user_id"]):
        player_rows = list(grouped)
        first = player_rows[0]
        identity = ("name", "tag", "profile_icon_id", "main_lane")
        stats: dict[Lane, LeaderboardStat] = {}
        for row in player_rows:
            if any(row[key] != first[key] for key in identity):
                raise ValueError("inconsistent player identity across lanes")
            if row["lane"] not in LANES or row["lane"] in stats:
                raise ValueError("invalid or duplicate played lane")
            if row["wins"] > row["games"]:
                raise ValueError("wins exceed games played")
            stats[row["lane"]] = LeaderboardStat(
                mmr=row["mmr"],
                games=row["games"],
                wins=row["wins"],
                championId=row["champion_id"],
            )
        if first["tag"] is not None and not first["tag"].strip():
            raise ValueError("empty public Riot tag")
        configured_lane = first["main_lane"]
        if configured_lane is not None and configured_lane not in LANES:
            raise ValueError("invalid configured main lane")
        main_role = (
            configured_lane
            if configured_lane in stats
            else min(
                stats,
                key=lambda lane: (
                    -stats[lane].games,
                    -stats[lane].mmr,
                    -math.copysign(1.0, stats[lane].mmr),
                    LANES.index(lane),
                ),
            )
        )
        # Rust's f64 sum starts at -0.0, preserving signed-zero values and their ordering.
        # Keep sequential arithmetic rather than Python's compensated float sum.
        weighted_mmr = -0.0
        for stat in stats.values():
            weighted_mmr += stat.mmr * stat.games
        total_games = sum(stat.games for stat in stats.values())
        players.append(
            LeaderboardPlayer(
                id=user_id,
                name=first["name"],
                tag=first["tag"],
                profileIconId=first["profile_icon_id"],
                compositeMmr=weighted_mmr / total_games,
                mainRole=main_role,
                isMe=user_id == viewer_id,
                stats=stats,
            )
        )
    players.sort(
        key=lambda player: (
            -player.compositeMmr,
            -math.copysign(1.0, player.compositeMmr),
            player.id,
        )
    )
    return players


async def get_leaderboard(
    database: Database, guild_id: str, viewer_id: str, request_id: str
) -> dict[str, Any]:
    async with database.connection(transaction=True) as connection:
        await repository.begin_snapshot(connection)
        access = await repository.load_access(connection, guild_id, viewer_id)
        if access is None:
            raise hidden_guild()
        try:
            season_id, season_name = access["season_id"], access["season_name"]
            if (season_id is None) != (season_name is None):
                raise ValueError("incomplete active season identity")
            season = None
            players = []
            if season_id is not None:
                season = LeaderboardSeason(id=str(season_id), name=season_name)
                players = build_players(
                    await repository.load_rows(connection, guild_id, season_id), viewer_id
                )
            return LeaderboardSnapshot(season=season, players=players).model_dump()
        except ValueError:
            logger.exception(
                "guild_leaderboard_projection_failed", extra={"request_id": request_id}
            )
            raise ApiError.from_status(500) from None
