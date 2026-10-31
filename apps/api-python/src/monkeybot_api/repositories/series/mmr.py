import uuid
from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.repositories.series.snapshots import rows


async def lock_mmr(
    c: AsyncConnection, p: dict[str, Any], roster: list[dict[str, Any]]
) -> dict[str, dict[str, Any]]:
    ordered = sorted(roster, key=lambda r: (r["user_id"], r["lane"]))
    for r in ordered:
        await c.execute(
            text("""
        INSERT INTO user_lane_mmr
        (guild_id,season_id,user_id,lane,mmr,games_played,wins,revision,updated_at) VALUES
        (:guild,:season,:user_id,:lane,1500,0,0,0,:now) ON CONFLICT
        (guild_id,season_id,user_id,lane) DO NOTHING
    """),
            {**p, **r},
        )
    result = {}
    for r in ordered:
        result[r["user_id"]] = (
            await rows(
                c,
                """
        SELECT * FROM user_lane_mmr WHERE guild_id=:guild AND season_id=:season AND
        user_id=:user_id AND lane=:lane FOR UPDATE
    """,
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
        text("""
        INSERT INTO mmr_ledger_entries (guild_id,id,season_id,user_id,lane,
        lane_revision,source_type,source_id,game_id,opponent_user_id,mmr_before,mmr_after,delta,
        games_played_delta,wins_delta,reverses_entry_id,reversed_source_type,actor_type,actor_id,
        request_id,created_at) VALUES
        (:guild,:entry,:season,:user,:lane,:revision,:kind,:game,
        :game,:opponent,:before,:after,:delta,:played,:won,:reverses,:reversed_kind,'USER',:actor_id,
        :request_id,:now)
    """),
        v,
    )
    await c.execute(
        text(
            """
        UPDATE user_lane_mmr SET mmr=:after,games_played=games_played+:played,
        wins=wins+:won,revision=:revision,updated_at=:now WHERE guild_id=:guild AND
        season_id=:season AND user_id=:user AND lane=:lane
    """
        ),
        v,
    )


async def lock_last_game(c: AsyncConnection, p: dict[str, Any]) -> list[dict[str, Any]]:
    return await rows(
        c,
        """
        SELECT * FROM games WHERE guild_id=:guild AND series_id=:id AND status='CONFIRMED'
        ORDER BY game_number DESC LIMIT 1 FOR UPDATE
    """,
        p,
    )


async def game_ledger(c: AsyncConnection, p: dict[str, Any]) -> list[dict[str, Any]]:
    return await rows(
        c,
        """
        SELECT * FROM mmr_ledger_entries WHERE guild_id=:guild AND game_id=:game AND
        source_type='GAME_RESULT' ORDER BY user_id,lane
    """,
        p,
    )


async def has_blocking_entry(c: AsyncConnection, p: dict[str, Any]) -> bool | None:
    value: bool | None = await c.scalar(
        text("""
        SELECT EXISTS (SELECT 1 FROM mmr_ledger_entries later WHERE later.guild_id=:guild
        AND later.season_id=:season AND later.user_id=:user_id AND later.lane=:lane AND
        later.lane_revision>:lane_revision AND ( later.source_type NOT IN
        ('GAME_RESULT','GAME_REVERSAL') OR ( later.source_type='GAME_RESULT' AND NOT EXISTS
        (SELECT 1 FROM mmr_ledger_entries reversal WHERE reversal.guild_id=later.guild_id
        AND reversal.reverses_entry_id=later.id AND reversal.source_type='GAME_REVERSAL'))))
    """),
        p,
    )
    return value


async def void_game(c: AsyncConnection, p: dict[str, Any]) -> None:
    await c.execute(
        text("""
        UPDATE games SET status='VOIDED',mmr_status='REVERSED',voided_by=:actor_id,
        voided_by_type='USER',voided_at=:now,void_reason='OPERATOR_REVERT' WHERE
        guild_id=:guild AND id=:game
    """),
        p,
    )


async def supersede_input(c: AsyncConnection, p: dict[str, Any]) -> None:
    await c.execute(
        text("""
        UPDATE game_inputs SET status='SUPERSEDED',updated_by=:actor_id,updated_at=:now
        WHERE guild_id=:guild AND id=:input
    """),
        p,
    )
