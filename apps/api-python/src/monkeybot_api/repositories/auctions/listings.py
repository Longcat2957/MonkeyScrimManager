"""Cursor-filtered auction listing queries."""

import uuid
from datetime import datetime
from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.db.queries import params
from monkeybot_api.repositories.auctions import recruitment_queries, tournament_queries


async def recruitments(
    c: AsyncConnection,
    viewer: str,
    guild: str,
    cursor_time: datetime | None,
    cursor_id: uuid.UUID | None,
    limit: int,
) -> list[dict[str, Any]]:
    result = await c.execute(
        text(recruitment_queries.LIST), params(viewer, guild, cursor_time, cursor_id, limit)
    )
    return [dict(row) for row in result.mappings()]


async def tournaments(
    c: AsyncConnection,
    viewer: str | None,
    guild: str,
    state: str,
    season: uuid.UUID | None,
    cursor_time: datetime | None,
    cursor_id: uuid.UUID | None,
    limit: int,
) -> list[dict[str, Any]]:
    result = await c.execute(
        text(tournament_queries.LIST),
        params(viewer, guild, state, season, cursor_time, cursor_id, limit),
    )
    return [dict(row) for row in result.mappings()]
