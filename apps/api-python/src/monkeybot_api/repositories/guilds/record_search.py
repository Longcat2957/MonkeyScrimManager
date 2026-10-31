"""Database reads for the guild record search projection."""

from typing import Any
from uuid import UUID

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.repositories.guilds import record_search_queries as sql
from monkeybot_api.schemas.guilds.record_search import Cursor


async def begin_snapshot(connection: AsyncConnection) -> None:
    await connection.execute(text("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY"))


async def rows(
    connection: AsyncConnection, query: str, parameters: dict[str, Any]
) -> list[dict[str, Any]]:
    return [dict(row) for row in (await connection.execute(text(query), parameters)).mappings()]


async def seasons(connection: AsyncConnection, guild_id: str) -> list[dict[str, Any]]:
    return await rows(connection, sql.SEASONS, {"guild_id": guild_id})


async def statistics(
    connection: AsyncConnection, guild_id: str, season_id: UUID, user_id: str
) -> tuple[dict[str, Any], list[dict[str, Any]], list[dict[str, Any]]]:
    parameters = {"guild_id": guild_id, "season_id": season_id, "user_id": user_id}
    summary = (await rows(connection, sql.SUMMARY, parameters))[0]
    lanes = await rows(connection, sql.LANE_MMR, parameters)
    champions = await rows(connection, sql.CHAMPIONS, parameters)
    return summary, lanes, champions


async def heads(
    connection: AsyncConnection,
    guild_id: str,
    season_id: UUID,
    user_id: str,
    cursor: Cursor | None,
    limit: int,
) -> list[dict[str, Any]]:
    return await rows(
        connection,
        sql.HEADS,
        {
            "guild_id": guild_id,
            "season_id": season_id,
            "user_id": user_id,
            "cursor_played_at": cursor.database_timestamp if cursor else None,
            "cursor_id": cursor.id if cursor else None,
            "cursor_kind": cursor.kind if cursor else None,
            "page_size": limit + 1,
        },
    )


async def details(
    connection: AsyncConnection, guild_id: str, user_id: str, heads: list[dict[str, Any]]
) -> tuple[
    list[dict[str, Any]],
    list[dict[str, Any]],
    list[dict[str, Any]],
    list[dict[str, Any]],
    list[dict[str, Any]],
]:
    parameters = {
        "guild_id": guild_id,
        "user_id": user_id,
        "scopes": [head["scope"] for head in heads],
        "ids": [head["id"] for head in heads],
    }
    games = await rows(connection, sql.GAMES, parameters)
    parameters["game_ids"] = [game["id"] for game in games]
    lineups = await rows(connection, sql.LINEUPS, parameters)
    bans = await rows(connection, sql.BANS, parameters)
    tournaments = [head for head in heads if head["scope"] == "TOURNAMENT"]
    if not tournaments:
        return games, lineups, bans, [], []
    if any(head["team_id"] is None for head in tournaments):
        raise ValueError("missing tournament team")
    parameters["tournament_ids"] = [head["id"] for head in tournaments]
    parameters["team_ids"] = [head["team_id"] for head in tournaments]
    meta = await rows(connection, sql.SERIES_META, parameters)
    rosters = await rows(connection, sql.ROSTERS, parameters)
    return games, lineups, bans, meta, rosters
