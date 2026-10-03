"""Project cached identities without exposing account or Discord linkage."""

from typing import Any

from monkeybot_api.core.errors import hidden_guild
from monkeybot_api.db.database import Database
from monkeybot_api.repositories import search_suggestions as repository
from monkeybot_api.schemas.search_suggestions import Suggestion, SuggestionsData


async def public_riot_accounts(
    database: Database, platform: str, prefix: str, limit: int
) -> dict[str, Any]:
    async with database.connection() as connection:
        rows = await repository.public_riot_accounts(connection, platform, prefix, limit)
    return SuggestionsData(
        suggestions=[
            Suggestion(
                kind="riotAccount",
                value=f"{row['game_name']}#{row['tag_line']}",
                label=f"{row['game_name']}#{row['tag_line']}",
                description=row["platform"],
                profileIconId=row["profile_icon_id"],
            )
            for row in rows
        ]
    ).model_dump()


async def guild_members(
    database: Database, viewer_id: str, guild_id: str, prefix: str, limit: int
) -> dict[str, Any]:
    async with database.connection() as connection:
        rows = await repository.guild_members(connection, viewer_id, guild_id, prefix, limit)
    if not rows or not rows[0]["has_access"]:
        raise hidden_guild()
    return SuggestionsData(
        suggestions=[
            Suggestion(
                kind="guildMember",
                value=row["display_name"],
                label=row["display_name"],
                description=(
                    f"{row['game_name']}#{row['tag_line']}"
                    if row["game_name"] is not None and row["tag_line"] is not None
                    else None
                ),
                profileIconId=row["profile_icon_id"],
            )
            for row in rows
            if row["display_name"] is not None
        ]
    ).model_dump()
