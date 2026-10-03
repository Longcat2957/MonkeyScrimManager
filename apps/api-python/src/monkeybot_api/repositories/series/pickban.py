import uuid
from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.repositories.series.snapshots import rows


async def ensure_input(c: AsyncConnection, p: dict[str, Any], number: int) -> dict[str, Any]:
    v = {**p, "number": number}
    drafts = await rows(
        c,
        """
        SELECT * FROM game_inputs WHERE guild_id=:guild AND series_id=:id AND
        set_number=:number AND status='DRAFT' FOR UPDATE
    """,
        v,
    )
    if drafts:
        return drafts[0]
    v["input"] = uuid.uuid7()
    return (
        await rows(
            c,
            """
        INSERT INTO game_inputs (guild_id,id,series_id,set_number,side_preset,
        side_selected,status,updated_by,updated_at) VALUES
        (:guild,:input,:id,:number,'DEFAULT', false,'DRAFT',:actor_id,:now) RETURNING *
    """,
            v,
        )
    )[0]


async def set_side(c: AsyncConnection, p: dict[str, Any]) -> None:
    await c.execute(
        text("""
        UPDATE game_inputs SET side_preset=:preset, side_selected=true WHERE guild_id=:guild
        AND id=:input
    """),
        p,
    )


async def lock_side_selected(c: AsyncConnection, p: dict[str, Any]) -> bool | None:
    value: bool | None = await c.scalar(
        text("""
        SELECT side_selected FROM game_inputs WHERE guild_id=:guild AND id=:input AND
        status='DRAFT' FOR UPDATE
    """),
        p,
    )
    return value


async def set_pick(c: AsyncConnection, p: dict[str, Any]) -> None:
    await c.execute(
        text("""
        INSERT INTO game_input_picks (guild_id,game_input_id,user_id,champion_id) VALUES
        (:guild,:input,:playerId,:championId) ON CONFLICT (guild_id,game_input_id,user_id)
        DO UPDATE SET champion_id=EXCLUDED.champion_id
    """),
        p,
    )


async def clear_pick(c: AsyncConnection, p: dict[str, Any]) -> None:
    await c.execute(
        text("""
        DELETE FROM game_input_picks WHERE guild_id=:guild AND game_input_id=:input AND
        user_id=:playerId
    """),
        p,
    )


async def set_ban(c: AsyncConnection, p: dict[str, Any]) -> None:
    await c.execute(
        text("""
        INSERT INTO game_input_bans (guild_id,game_input_id,side,slot,champion_id) VALUES
        (:guild,:input,:side,:slot,:championId) ON CONFLICT
        (guild_id,game_input_id,side,slot) DO UPDATE SET champion_id=EXCLUDED.champion_id
    """),
        p,
    )


async def clear_ban(c: AsyncConnection, p: dict[str, Any]) -> None:
    await c.execute(
        text("""
        DELETE FROM game_input_bans WHERE guild_id=:guild AND game_input_id=:input AND
        side=:side AND slot=:slot
    """),
        p,
    )


async def draft_picks(c: AsyncConnection, p: dict[str, Any]) -> list[dict[str, Any]]:
    return await rows(
        c,
        """
        SELECT pick.champion_id FROM game_input_picks pick JOIN game_inputs i ON
        i.guild_id=pick.guild_id AND i.id=pick.game_input_id WHERE i.guild_id=:guild AND
        i.series_id=:id AND i.status='DRAFT' ORDER BY pick.champion_id,pick.user_id
    """,
        p,
    )


async def draft_bans(c: AsyncConnection, p: dict[str, Any]) -> list[dict[str, Any]]:
    return await rows(
        c,
        """
        SELECT ban.champion_id FROM game_input_bans ban JOIN game_inputs i ON
        i.guild_id=ban.guild_id AND i.id=ban.game_input_id WHERE i.guild_id=:guild AND
        i.series_id=:id AND i.status='DRAFT' ORDER BY ban.champion_id,ban.side,ban.slot
    """,
        p,
    )


async def confirmed_champions(c: AsyncConnection, p: dict[str, Any]) -> list[dict[str, Any]]:
    return await rows(
        c,
        """
        SELECT DISTINCT gp.champion_id FROM game_participants gp JOIN games g ON
        g.guild_id=gp.guild_id AND g.id=gp.game_id WHERE g.guild_id=:guild AND
        g.series_id=:id AND g.status='CONFIRMED'
    """,
        p,
    )


async def set_result(c: AsyncConnection, p: dict[str, Any]) -> None:
    await c.execute(
        text("""
        UPDATE game_inputs SET side_preset=:sidePreset,side_selected=true,
        winning_side=:winner,patch_version=:patchVersion WHERE guild_id=:guild AND id=:input
    """),
        p,
    )


async def clear_picks(c: AsyncConnection, p: dict[str, Any]) -> None:
    await c.execute(
        text("DELETE FROM game_input_picks WHERE guild_id=:guild AND game_input_id=:input"), p
    )


async def clear_bans(c: AsyncConnection, p: dict[str, Any]) -> None:
    await c.execute(
        text("DELETE FROM game_input_bans WHERE guild_id=:guild AND game_input_id=:input"), p
    )
