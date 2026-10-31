"""Resolve a member and project their season history from one consistent snapshot."""

import logging
from typing import Any
from uuid import UUID

from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.time import timestamp
from monkeybot_api.db.database import Database
from monkeybot_api.repositories.guilds import record_search as repository
from monkeybot_api.schemas.guilds.record_search import Cursor, LaneMmr, SearchData, Season, Summary
from monkeybot_api.schemas.users import LANES
from monkeybot_api.services.guilds.record_profile import load_identity, resolve_member
from monkeybot_api.services.guilds.record_search_projection import assemble

logger = logging.getLogger(__name__)


def lane_snapshot(rows: list[dict[str, Any]]) -> list[LaneMmr]:
    lanes = {}
    for row in rows:
        if row["games_played"] < 0 or not 0 <= row["wins"] <= row["games_played"]:
            raise ValueError("invalid lane MMR counts")
        lanes[row["lane"]] = LaneMmr.model_validate({**row, "games": row["games_played"]})
    return [
        lanes.get(lane)
        or LaneMmr.model_validate({"lane": lane, "mmr": 1500.0, "games": 0, "wins": 0})
        for lane in LANES
    ]


async def search(
    database: Database,
    guild_id: str,
    viewer_id: str,
    query: str,
    requested_season: UUID | None,
    cursor: Cursor | None,
    limit: int,
    request_id: str,
) -> dict[str, Any]:
    async with database.connection(transaction=True) as connection:
        await repository.begin_snapshot(connection)
        try:
            member = await resolve_member(connection, guild_id, viewer_id, query)
            identity = await load_identity(connection, member)
            rows = await repository.seasons(connection, guild_id)
            for row in rows:
                if (row["status"] == "ACTIVE") != (row["ended_at"] is None):
                    raise ValueError("inconsistent season status")
            seasons = [
                Season.model_validate(
                    {
                        **row,
                        "id": str(row["id"]),
                        "started_at": timestamp(row["started_at"]),
                        "ended_at": timestamp(row["ended_at"]) if row["ended_at"] else None,
                    }
                )
                for row in rows
            ]
            selected = requested_season
            if selected is not None:
                if not any(row["id"] == selected for row in rows):
                    raise ApiError(404, "season_not_found", "요청한 시즌을 찾을 수 없습니다.")
            else:
                selected = next((row["id"] for row in rows if row["status"] == "ACTIVE"), None)
            summary = Summary(games=0, wins=0, losses=0)
            champions: list[dict[str, Any]] = []
            lanes, records, next_cursor = lane_snapshot([]), [], None
            if selected is not None:
                summary_row, lane_rows, champions = await repository.statistics(
                    connection, guild_id, selected, member["user_id"]
                )
                summary = Summary(**summary_row, losses=summary_row["games"] - summary_row["wins"])
                lanes = lane_snapshot(lane_rows)
                heads = await repository.heads(
                    connection, guild_id, selected, member["user_id"], cursor, limit
                )
                if len(heads) > limit:
                    heads.pop()
                    next_cursor = Cursor.from_head(heads[-1]).encode()
                if heads:
                    records = assemble(
                        heads,
                        *await repository.details(connection, guild_id, member["user_id"], heads),
                    )
            return SearchData.model_validate(
                {
                    **identity,
                    "seasons": seasons,
                    "selected_season_id": str(selected) if selected else None,
                    "summary": summary,
                    "lane_mmr": lanes,
                    "champion_records": champions,
                    "records": records,
                    "next_cursor": next_cursor,
                }
            ).model_dump(by_alias=True)
        except ValueError:
            logger.exception(
                "guild_record_search_projection_failed", extra={"request_id": request_id}
            )
            raise ApiError.from_status(500) from None
