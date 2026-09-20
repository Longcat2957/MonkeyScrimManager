import math
import uuid
from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.errors import ApiError
from monkeybot_api.profile import LANES
from monkeybot_api.series_errors import error
from monkeybot_api.series_snapshot import rows


async def lock_mmr(
    c: AsyncConnection, p: dict[str, Any], roster: list[dict[str, Any]]
) -> dict[str, dict[str, Any]]:
    ordered = sorted(roster, key=lambda r: (r["user_id"], r["lane"]))
    for r in ordered:
        await c.execute(
            text("""INSERT INTO user_lane_mmr
            (guild_id,season_id,user_id,lane,mmr,games_played,wins,revision,updated_at)
            VALUES (:guild,:season,:user_id,:lane,1500,0,0,0,:now)
            ON CONFLICT (guild_id,season_id,user_id,lane) DO NOTHING"""),
            {**p, **r},
        )
    result = {}
    for r in ordered:
        result[r["user_id"]] = (
            await rows(
                c,
                "SELECT * FROM user_lane_mmr WHERE guild_id=:guild "
                "AND season_id=:season AND user_id=:user_id AND lane=:lane FOR UPDATE",
                {**p, **r},
            )
        )[0]
    return result


async def write(
    c: AsyncConnection,
    p: dict[str, Any],
    current: dict[str, Any],
    *,
    after: float,
    delta: float,
    opponent: str,
    won: int,
    reverse: dict[str, Any] | None = None,
) -> None:
    v = {
        **p,
        "entry": uuid.uuid7(),
        "user": current["user_id"],
        "lane": current["lane"],
        "revision": current["revision"] + 1,
        "before": current["mmr"],
        "after": after,
        "delta": delta,
        "opponent": opponent,
        "played": -1 if reverse else 1,
        "won": won,
        "kind": "GAME_REVERSAL" if reverse else "GAME_RESULT",
        "reverses": reverse["id"] if reverse else None,
        "reversed_kind": "GAME_RESULT" if reverse else None,
    }
    await c.execute(
        text("""INSERT INTO mmr_ledger_entries (guild_id,id,season_id,user_id,lane,
        lane_revision,source_type,source_id,game_id,opponent_user_id,mmr_before,mmr_after,delta,
        games_played_delta,wins_delta,reverses_entry_id,reversed_source_type,actor_type,actor_id,
        request_id,created_at) VALUES (:guild,:entry,:season,:user,:lane,:revision,:kind,:game,
        :game,:opponent,:before,:after,:delta,:played,:won,:reverses,:reversed_kind,'USER',:actor_id,
        :request_id,:now)"""),
        v,
    )
    await c.execute(
        text(
            "UPDATE user_lane_mmr SET mmr=:after,games_played=games_played+:played, "
            "wins=wins+:won,revision=:revision,updated_at=:now WHERE guild_id=:guild "
            "AND season_id=:season AND user_id=:user AND lane=:lane"
        ),
        v,
    )


async def apply_mmr(
    c: AsyncConnection, p: dict[str, Any], roster: list[dict[str, Any]], winner: str
) -> None:
    locked = await lock_mmr(c, p, roster)
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
            await write(
                c,
                p,
                current,
                after=current["mmr"] + delta,
                delta=delta,
                opponent=opponent["user_id"],
                won=won,
            )


async def revert_last(c: AsyncConnection, p: dict[str, Any]) -> dict[str, Any]:
    found = await rows(
        c,
        "SELECT * FROM games WHERE guild_id=:guild AND series_id=:id "
        "AND status='CONFIRMED' ORDER BY game_number DESC LIMIT 1 FOR UPDATE",
        p,
    )
    if not found:
        raise error("series_no_confirmed_games")
    game = found[0]
    v = {**p, "season": game["season_id"], "game": game["id"]}
    ledger = await rows(
        c,
        "SELECT * FROM mmr_ledger_entries WHERE guild_id=:guild "
        "AND game_id=:game AND source_type='GAME_RESULT' ORDER BY user_id,lane",
        v,
    )
    if len(ledger) != 10:
        raise ApiError.from_status(500)
    locked = await lock_mmr(c, v, [{"user_id": r["user_id"], "lane": r["lane"]} for r in ledger])
    for r in ledger:
        current = locked[r["user_id"]]
        blocking = await c.scalar(
            text("""SELECT EXISTS (SELECT 1 FROM mmr_ledger_entries later
            WHERE later.guild_id=:guild AND later.season_id=:season AND later.user_id=:user_id
            AND later.lane=:lane AND later.lane_revision>:lane_revision AND (
                later.source_type NOT IN ('GAME_RESULT','GAME_REVERSAL') OR (
                    later.source_type='GAME_RESULT' AND NOT EXISTS (SELECT 1 FROM
                    mmr_ledger_entries reversal
                        WHERE reversal.guild_id=later.guild_id AND
                        reversal.reverses_entry_id=later.id
                        AND reversal.source_type='GAME_REVERSAL'))))"""),
            {**v, **r},
        )
        if (
            current["mmr"] != r["mmr_after"]
            or current["games_played"] < r["games_played_delta"]
            or current["wins"] < r["wins_delta"]
            or blocking
        ):
            raise error("series_game_reversal_not_safe")
    for r in ledger:
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
    await c.execute(
        text(
            "UPDATE games SET status='VOIDED',mmr_status='REVERSED',voided_by=:actor_id, "
            "voided_by_type='USER',voided_at=:now,void_reason='OPERATOR_REVERT' "
            "WHERE guild_id=:guild AND id=:game"
        ),
        v,
    )
    await c.execute(
        text(
            "UPDATE game_inputs SET status='SUPERSEDED',updated_by=:actor_id,updated_at=:now "
            "WHERE guild_id=:guild AND id=:input"
        ),
        {**v, "input": game["source_input_id"]},
    )
    return game
