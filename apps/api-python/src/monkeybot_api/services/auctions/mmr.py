import uuid
from typing import Any

from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError
from monkeybot_api.repositories.auctions import mmr as storage
from monkeybot_api.repositories.series.mmr import lock_mmr, write
from monkeybot_api.schemas.auctions.games import error


async def void_game(
    c: AsyncConnection,
    p: dict[str, Any],
    game: dict[str, Any],
    *,
    actor_type: str = "USER",
    void_reason: str = "OPERATOR_REVERT",
) -> None:
    """Reverse a locked confirmed game, retaining the append-only ledger history."""
    v = {**p, "season": game["season_id"], "game": game["id"]}
    ledger = await storage.result_ledger(c, v)
    if len(ledger) != 10:
        raise ApiError.from_status(500)
    locked = await lock_mmr(c, v, [{"user_id": r["user_id"], "lane": r["lane"]} for r in ledger])
    for r in ledger:
        current = locked[r["user_id"]]
        blocking = await storage.has_blocking_ledger_entry(c, {**v, **r})
        if (
            current["mmr"] != r["mmr_after"]
            or current["games_played"] < r["games_played_delta"]
            or current["wins"] < r["wins_delta"]
            or blocking
        ):
            raise error("auction_match_game_reversal_not_safe")
    for r in ledger:
        # The shared writer is the exact same lane-Elo/append-only operation; cancellation
        # has a historical BOT actor contract, so its ledger insert is explicit below.
        if actor_type == "USER":
            await write(
                c,
                v,
                locked[r["user_id"]],
                after=r["mmr_before"],
                delta=-r["delta"],
                opponent=r["opponent_user_id"],
                won=-r["wins_delta"],
                reverse=r,
            )
        else:
            current = locked[r["user_id"]]
            values = {
                **v,
                **r,
                "entry": uuid.uuid7(),
                "actor_type": actor_type,
                "actor_id": p["actor_id"],
                "request_id": p["request_id"],
                "now": p["now"],
                "current": current["mmr"],
                "next_revision": current["revision"] + 1,
                "reversal_delta": -r["delta"],
                "reversal_played": -r["games_played_delta"],
                "reversal_wins": -r["wins_delta"],
            }
            await storage.append_reversal(c, values)
            await storage.restore_lane_mmr(c, values)
    v.update(actor_type=actor_type, void_reason=void_reason, input=game["source_input_id"])
    for operation in (storage.void_confirmed_game, storage.supersede_submitted_input):
        result = await operation(c, v)
        if result.rowcount != 1:
            raise ApiError.from_status(500)
