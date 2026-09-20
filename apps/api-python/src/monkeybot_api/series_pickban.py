import uuid
from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.errors import ApiError
from monkeybot_api.series_errors import error
from monkeybot_api.series_snapshot import rows


async def ensure_input(c: AsyncConnection, p: dict[str, Any], number: int) -> dict[str, Any]:
    v = {**p, "number": number}
    drafts = await rows(
        c,
        "SELECT * FROM game_inputs WHERE guild_id=:guild AND series_id=:id "
        "AND set_number=:number AND status='DRAFT' FOR UPDATE",
        v,
    )
    if drafts:
        return drafts[0]
    v["input"] = uuid.uuid7()
    return (
        await rows(
            c,
            """INSERT INTO game_inputs (guild_id,id,series_id,set_number,side_preset,
        side_selected,status,updated_by,updated_at) VALUES (:guild,:input,:id,:number,'DEFAULT',
        false,'DRAFT',:actor_id,:now) RETURNING *""",
            v,
        )
    )[0]


async def apply(c: AsyncConnection, p: dict[str, Any], op: dict[str, Any], ids: set[str]) -> None:
    kind = op["type"]
    if kind == "SET_SIDE":
        await c.execute(
            text(
                "UPDATE game_inputs SET side_preset=:preset, side_selected=true "
                "WHERE guild_id=:guild AND id=:input"
            ),
            {**p, "preset": op["sidePreset"]},
        )
        return
    selected = await c.scalar(
        text(
            "SELECT side_selected FROM game_inputs WHERE guild_id=:guild "
            "AND id=:input AND status='DRAFT' FOR UPDATE"
        ),
        p,
    )
    if not selected:
        raise error("series_game_side_required")
    if kind == "APPLY_BULK":
        for item in op["items"]:
            await apply(c, p, item, ids)
        return
    v = {**p, **op}
    if "playerId" in op:
        if op["playerId"] not in ids:
            raise error("series_pickban_player_not_in_roster")
        if kind == "SET_PICK":
            await c.execute(
                text("""INSERT INTO game_input_picks
                (guild_id,game_input_id,user_id,champion_id) VALUES
                (:guild,:input,:playerId,:championId)
                ON CONFLICT (guild_id,game_input_id,user_id) DO UPDATE
                SET champion_id=EXCLUDED.champion_id"""),
                v,
            )
        else:
            await c.execute(
                text(
                    "DELETE FROM game_input_picks WHERE guild_id=:guild "
                    "AND game_input_id=:input AND user_id=:playerId"
                ),
                v,
            )
    elif kind == "SET_BAN":
        await c.execute(
            text("""INSERT INTO game_input_bans
            (guild_id,game_input_id,side,slot,champion_id) VALUES
            (:guild,:input,:side,:slot,:championId)
            ON CONFLICT (guild_id,game_input_id,side,slot) DO UPDATE
            SET champion_id=EXCLUDED.champion_id"""),
            v,
        )
    else:
        await c.execute(
            text(
                "DELETE FROM game_input_bans WHERE guild_id=:guild "
                "AND game_input_id=:input AND side=:side AND slot=:slot"
            ),
            v,
        )


async def validate_draft(c: AsyncConnection, p: dict[str, Any], fearless: bool) -> None:
    picks = await rows(
        c,
        "SELECT pick.champion_id FROM game_input_picks pick JOIN game_inputs i "
        "ON i.guild_id=pick.guild_id AND i.id=pick.game_input_id WHERE i.guild_id=:guild "
        "AND i.series_id=:id AND i.status='DRAFT' ORDER BY pick.champion_id,pick.user_id",
        p,
    )
    bans = await rows(
        c,
        "SELECT ban.champion_id FROM game_input_bans ban JOIN game_inputs i "
        "ON i.guild_id=ban.guild_id AND i.id=ban.game_input_id WHERE i.guild_id=:guild "
        "AND i.series_id=:id AND i.status='DRAFT' ORDER BY ban.champion_id,ban.side,ban.slot",
        p,
    )
    seen = set()
    for r in picks + bans:
        champ = r["champion_id"]
        if champ in seen:
            raise ApiError(
                422,
                "duplicate_series_game_champion",
                "같은 세트에서 픽과 밴은 같은 챔피언을 중복할 수 없습니다.",
                {"championId": champ},
            )
        seen.add(champ)
    if fearless:
        used = {
            r["champion_id"]
            for r in await rows(
                c,
                "SELECT DISTINCT gp.champion_id "
                "FROM game_participants gp JOIN games g "
                "ON g.guild_id=gp.guild_id AND g.id=gp.game_id "
                "WHERE g.guild_id=:guild AND g.series_id=:id AND g.status='CONFIRMED'",
                p,
            )
        }
        for r in picks:
            if r["champion_id"] in used:
                raise ApiError(
                    422,
                    "hard_fearless_champion_used",
                    "하드 피어리스에서는 이전 세트에서 픽한 챔피언을 다시 픽할 수 없습니다.",
                    {"championId": r["champion_id"]},
                )


async def replace(
    c: AsyncConnection,
    p: dict[str, Any],
    draft: dict[str, Any],
    body: dict[str, Any],
    ids: set[str],
) -> None:
    if draft["side_selected"] and draft["side_preset"] != body["sidePreset"]:
        raise error(
            "series_game_side_mismatch",
            {"expectedSidePreset": draft["side_preset"], "actualSidePreset": body["sidePreset"]},
        )
    await c.execute(
        text(
            "UPDATE game_inputs SET side_preset=:sidePreset,side_selected=true, "
            "winning_side=:winner,patch_version=:patchVersion WHERE guild_id=:guild AND id=:input"
        ),
        {**p, **body},
    )
    if body["picks"] is not None:
        if set(body["picks"]) != ids:
            raise ApiError(
                422,
                "invalid_series_game_result",
                "게임 결과의 승리 팀, 픽, 밴 또는 패치 버전 값이 올바르지 않습니다.",
            )
        await c.execute(
            text("DELETE FROM game_input_picks WHERE guild_id=:guild AND game_input_id=:input"), p
        )
        for user, champ in body["picks"].items():
            if champ is not None:
                await apply(c, p, {"type": "SET_PICK", "playerId": user, "championId": champ}, ids)
    if body["bans"] is not None:
        await c.execute(
            text("DELETE FROM game_input_bans WHERE guild_id=:guild AND game_input_id=:input"), p
        )
        for side, slots in body["bans"].items():
            for i, champ in enumerate(slots, 1):
                if champ is not None:
                    await apply(
                        c, p, {"type": "SET_BAN", "side": side, "slot": i, "championId": champ}, ids
                    )
