import uuid
from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

import monkeybot_api.repositories.auctions.recruitment_queries as sql
from monkeybot_api.db.queries import one, params
from monkeybot_api.schemas.auctions.cancellation import error


async def preview(
    c: AsyncConnection, guild: str, identifier: uuid.UUID, viewer: str | None
) -> dict[str, Any]:
    bot = viewer is None
    if viewer is not None and not await c.scalar(text(sql.OPERATOR), params(guild, viewer)):
        raise error("operator_required")
    row = await one(
        c,
        """SELECT tournament.id, tournament.display_number, tournament.title,
                  tournament.status,tournament.revision
           FROM auction_tournaments tournament JOIN guilds guild
             ON guild.guild_id=tournament.guild_id AND guild.status='ACTIVE'
           WHERE tournament.guild_id=:p1 AND tournament.id=:p2
             AND tournament.deleted_at IS NULL""",
        guild,
        identifier,
    )
    if row is None:
        raise error("auction_tournament_not_found", bot=bot)
    return {
        "tournament": {
            "id": str(row["id"]),
            "displayNumber": row["display_number"],
            "title": row["title"],
            "status": row["status"],
            "revision": row["revision"],
        }
    }
