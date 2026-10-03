"""Authorize and project a coherent season statistics snapshot."""

import logging
from fractions import Fraction
from typing import Any
from uuid import UUID

from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError, hidden_guild
from monkeybot_api.db.database import Database
from monkeybot_api.repositories.guilds import statistics as repository
from monkeybot_api.schemas.guilds.statistics import (
    Auction,
    Bid,
    Captain,
    ChampionBan,
    ChampionPick,
    Champions,
    Coverage,
    Duo,
    Overview,
    Player,
    Players,
    Relationships,
    Scope,
    Season,
    StatisticsSnapshot,
)

logger = logging.getLogger(__name__)
TOP_ITEMS = 10
MINIMUM_WIN_RATE_GAMES = 3


def record(row: dict[str, Any]) -> dict[str, Any]:
    return {**row, "losses": losses(row["games"], row["wins"])}


def losses(games: int, wins: int) -> int:
    if games < 0 or not 0 <= wins <= games:
        raise ValueError("stored statistics record is inconsistent")
    return games - wins


async def get_statistics(
    database: Database,
    guild: str,
    actor: str,
    requested_season: UUID | None,
    scope: Scope,
    request_id: str,
) -> dict[str, Any]:
    async with database.connection(transaction=True) as connection:
        await repository.begin_snapshot(connection)
        if not await repository.has_access(connection, guild, actor):
            raise hidden_guild()
        try:
            seasons = [
                Season.model_validate({**row, "id": str(row["id"])})
                for row in await repository.list_seasons(connection, guild)
            ]
            selected: UUID | None
            if requested_season is not None:
                if str(requested_season) not in {season.id for season in seasons}:
                    raise ApiError(404, "season_not_found", "요청한 시즌을 찾을 수 없습니다.")
                selected = requested_season
            else:
                selected = next(
                    (UUID(season.id) for season in seasons if season.status == "ACTIVE"), None
                )
            snapshot = StatisticsSnapshot(
                seasons=seasons, selected_season_id=str(selected) if selected else None, scope=scope
            )
            if selected is not None:
                overview = await repository.overview(connection, guild, selected, scope)
                snapshot.overview = Overview.model_validate(
                    {
                        **overview,
                        "coverage": Coverage.model_validate(overview),
                    }
                )
                players = [
                    Player.model_validate(record(row))
                    for row in await repository.players(connection, guild, selected, scope)
                ]
                snapshot.players = Players(
                    minimum_win_rate_games=MINIMUM_WIN_RATE_GAMES,
                    most_active=sorted(players, key=lambda p: (-p.games, -p.wins, p.user_id))[
                        :TOP_ITEMS
                    ],
                    best_win_rates=sorted(
                        [p for p in players if p.games >= MINIMUM_WIN_RATE_GAMES],
                        key=lambda p: (-Fraction(p.wins, p.games), -p.games, -p.wins, p.user_id),
                    )[:TOP_ITEMS],
                )
                snapshot.relationships = Relationships(
                    best_duos=[
                        Duo.model_validate(record(row))
                        for row in await repository.duos(connection, guild, selected, scope)
                    ]
                )
                snapshot.champions = Champions(
                    picks=[
                        ChampionPick.model_validate(record(row))
                        for row in await repository.champion_picks(
                            connection, guild, selected, scope
                        )
                    ],
                    bans=[
                        ChampionBan.model_validate(row)
                        for row in await repository.champion_bans(
                            connection, guild, selected, scope
                        )
                    ],
                )
                if scope != "REGULAR":
                    snapshot.auction = await auction_statistics(connection, guild, selected)
            return snapshot.model_dump(by_alias=True)
        except ValueError:
            logger.exception("guild_statistics_projection_failed", extra={"request_id": request_id})
            raise ApiError.from_status(500) from None


async def auction_statistics(connection: AsyncConnection, guild: str, season: UUID) -> Auction:
    completed = await repository.completed_tournaments(connection, guild, season)
    captains = [
        Captain.model_validate(
            {
                **row,
                "match_losses": losses(row["matches"], row["match_wins"]),
                "game_losses": losses(row["games"], row["game_wins"]),
            }
        )
        for row in await repository.captains(connection, guild, season)
    ]
    bids = [
        Bid.model_validate({**record(row), "tournament_id": str(row["tournament_id"])})
        for row in await repository.top_bids(connection, guild, season)
    ]
    return Auction(completed_tournaments=completed, captains=captains, top_bids=bids)
