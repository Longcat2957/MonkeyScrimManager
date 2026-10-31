import re
from typing import Any

from monkeybot_api.core.errors import hidden_guild
from monkeybot_api.core.time import timestamp
from monkeybot_api.db.database import Database
from monkeybot_api.repositories.guilds import context as repository


def guild_summary(row: dict[str, Any]) -> dict[str, Any]:
    icon = row["icon_hash"]
    icon_url = None
    if icon and re.fullmatch(r"[0-9a-fA-F_]{1,128}", icon):
        icon_url = f"https://cdn.discordapp.com/icons/{row['id']}/{icon}.webp?size=128"
    return {
        "id": row["id"],
        "displayName": row["display_name"],
        "iconUrl": icon_url,
        "memberCount": row["member_count"],
        "currentUserRole": "OPERATOR" if row["is_operator"] else "MEMBER",
    }


async def list_guilds(database: Database, user_id: str) -> dict[str, Any]:
    async with database.connection() as connection:
        rows = await repository.list_guilds(connection, user_id)
    return {"guilds": [guild_summary(row) for row in rows]}


async def get_guild(database: Database, user_id: str, guild_id: str) -> dict[str, Any]:
    async with database.connection() as connection:
        row = await repository.get_guild(connection, user_id, guild_id)
    if row is None:
        raise hidden_guild()
    season = None
    if row["season_id"] is not None:
        season = {
            "id": str(row["season_id"]),
            "name": row["season_name"],
            "startedAt": timestamp(row["season_started_at"]),
        }
    return {
        **guild_summary(row),
        "isExternallyPublic": row["is_externally_public"],
        "currentSeason": season,
    }


async def set_default_guild(database: Database, user_id: str, guild_id: str) -> dict[str, Any]:
    async with database.connection(transaction=True) as connection:
        changed = await repository.set_default_guild(connection, user_id, guild_id)
    if changed is None:
        raise hidden_guild()
    return {"defaultGuildId": changed}
