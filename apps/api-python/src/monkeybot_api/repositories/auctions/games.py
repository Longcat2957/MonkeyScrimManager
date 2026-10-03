from typing import Any

from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError
from monkeybot_api.repositories.series.snapshots import rows


async def games(c: AsyncConnection, p: dict[str, Any]) -> list[dict[str, Any]]:
    return await rows(
        c,
        "SELECT * FROM games WHERE guild_id=:guild AND auction_match_id=:id "
        "AND status='CONFIRMED' ORDER BY game_number",
        p,
    )


async def final_match(c: AsyncConnection, p: dict[str, Any]) -> dict[str, Any]:
    found = await rows(
        c,
        "SELECT * FROM auction_matches WHERE guild_id=:guild "
        "AND tournament_id=:tournament AND match_key='FINAL' AND status<>'CANCELLED' "
        "AND deleted_at IS NULL FOR UPDATE",
        p,
    )
    if len(found) != 1:
        raise ApiError.from_status(500)
    return found[0]
