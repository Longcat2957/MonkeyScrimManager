import math
from typing import Any

from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError
from monkeybot_api.domain.series.errors import error
from monkeybot_api.repositories.series import mmr as repository
from monkeybot_api.schemas.users import LANES


async def apply_mmr(
    c: AsyncConnection, p: dict[str, Any], roster: list[dict[str, Any]], winner: str
) -> None:
    locked = await repository.lock_mmr(c, p, roster)
    for lane in LANES:
        matchup = [
            next(r for r in roster if r["lane"] == lane and r["team"] == t)
            for t in ("TEAM_1", "TEAM_2")
        ]
        for player, opponent in (matchup, matchup[::-1]):
            current, other = locked[player["user_id"]], locked[opponent["user_id"]]
            exponent = (other["mmr"] - current["mmr"]) / 400.0
            try:
                expected = 1.0 / (1.0 + math.pow(10.0, exponent))
            except OverflowError:
                expected = 0.0
            won = int(player["team"] == winner)
            delta = 32.0 * (won - expected)
            await repository.write(
                c,
                p,
                current,
                after=current["mmr"] + delta,
                delta=delta,
                opponent=opponent["user_id"],
                won=won,
            )


async def revert_last(c: AsyncConnection, p: dict[str, Any]) -> dict[str, Any]:
    found = await repository.lock_last_game(c, p)
    if not found:
        raise error("series_no_confirmed_games")
    game = found[0]
    v = {**p, "season": game["season_id"], "game": game["id"]}
    ledger = await repository.game_ledger(c, v)
    if len(ledger) != 10:
        raise ApiError.from_status(500)
    locked = await repository.lock_mmr(
        c, v, [{"user_id": r["user_id"], "lane": r["lane"]} for r in ledger]
    )
    for r in ledger:
        current = locked[r["user_id"]]
        blocking = await repository.has_blocking_entry(c, {**v, **r})
        if (
            current["mmr"] != r["mmr_after"]
            or current["games_played"] < r["games_played_delta"]
            or current["wins"] < r["wins_delta"]
            or blocking
        ):
            raise error("series_game_reversal_not_safe")
    for r in ledger:
        await repository.write(
            c,
            v,
            locked[r["user_id"]],
            after=r["mmr_before"],
            delta=-r["delta"],
            opponent=r["opponent_user_id"],
            won=-r["wins_delta"],
            reverse=r,
        )
    await repository.void_game(c, v)
    await repository.supersede_input(c, {**v, "input": game["source_input_id"]})
    return game
