from typing import Any

from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError
from monkeybot_api.domain.series.errors import error
from monkeybot_api.repositories.series import pickban as repository


async def apply(c: AsyncConnection, p: dict[str, Any], op: dict[str, Any], ids: set[str]) -> None:
    kind = op["type"]
    if kind == "SET_SIDE":
        await repository.set_side(c, {**p, "preset": op["sidePreset"]})
        return
    selected = await repository.lock_side_selected(c, p)
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
            await repository.set_pick(c, v)
        else:
            await repository.clear_pick(c, v)
    elif kind == "SET_BAN":
        await repository.set_ban(c, v)
    else:
        await repository.clear_ban(c, v)


async def validate_draft(c: AsyncConnection, p: dict[str, Any], fearless: bool) -> None:
    picks = await repository.draft_picks(c, p)
    bans = await repository.draft_bans(c, p)
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
        used = {r["champion_id"] for r in await repository.confirmed_champions(c, p)}
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
    await repository.set_result(c, {**p, **body})
    if body["picks"] is not None:
        if set(body["picks"]) != ids:
            raise ApiError(
                422,
                "invalid_series_game_result",
                "게임 결과의 승리 팀, 픽, 밴 또는 패치 버전 값이 올바르지 않습니다.",
            )
        await repository.clear_picks(c, p)
        for user, champ in body["picks"].items():
            if champ is not None:
                await apply(c, p, {"type": "SET_PICK", "playerId": user, "championId": champ}, ids)
    if body["bans"] is not None:
        await repository.clear_bans(c, p)
        for side, slots in body["bans"].items():
            for i, champ in enumerate(slots, 1):
                if champ is not None:
                    await apply(
                        c, p, {"type": "SET_BAN", "side": side, "slot": i, "championId": champ}, ids
                    )
