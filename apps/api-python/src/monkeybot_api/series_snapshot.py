from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.context import timestamp
from monkeybot_api.entry_snapshot import compact_tier
from monkeybot_api.errors import ApiError
from monkeybot_api.profile import LANES
from monkeybot_api.series_errors import error
from monkeybot_api.series_queries import CHAMPIONS, TEAMS


async def rows(c: AsyncConnection, sql: str, p: dict[str, Any]) -> list[dict[str, Any]]:
    return [dict(r) for r in (await c.execute(text(sql), p)).mappings()]


async def operator(c: AsyncConnection, p: dict[str, Any]) -> bool:
    return bool(
        await c.scalar(
            text("""SELECT EXISTS (
        SELECT 1 FROM guild_members m JOIN guild_config g USING (guild_id)
        JOIN guild_member_discord_roles r ON r.guild_id=m.guild_id AND r.user_id=m.user_id
            AND r.role_id=g.operator_role_id
        WHERE m.guild_id=:guild AND m.user_id=:actor_id AND m.status='ACTIVE')"""),
            p,
        )
    )


def winner(preset: str, side: str) -> str:
    if preset not in {"DEFAULT", "SWAPPED"} or side not in {"BLUE", "RED"}:
        raise ApiError.from_status(500)
    return "TEAM_1" if (preset == "DEFAULT") == (side == "BLUE") else "TEAM_2"


async def games(c: AsyncConnection, p: dict[str, Any]) -> list[dict[str, Any]]:
    return await rows(
        c,
        "SELECT * FROM games WHERE guild_id=:guild AND series_id=:id "
        "AND status='CONFIRMED' ORDER BY game_number",
        p,
    )


def score(records: list[dict[str, Any]]) -> dict[str, int]:
    a = sum(winner(g["side_preset"], g["winning_side"]) == "TEAM_1" for g in records)
    return {"team1": a, "team2": len(records) - a}


async def teams(c: AsyncConnection, p: dict[str, Any]) -> list[dict[str, Any]]:
    q = {"p1": p["guild"], "p2": p["id"]}
    players = await rows(c, TEAMS, q)
    champions: dict[str, list[str]] = {}
    for r in await rows(c, CHAMPIONS, q):
        champions.setdefault(r["user_id"], []).append(r["champion_id"])
    result = []
    for i, team in enumerate(("TEAM_1", "TEAM_2"), 1):
        members = [
            {
                "id": r["id"],
                "displayName": r["display_name"],
                "lane": r["lane"],
                "mmr": r["mmr"],
                "riotTier": compact_tier(r["solo_rank_tier"], r["solo_rank_division"]),
                "preferredChampionIds": champions.get(r["id"], []),
            }
            for r in players
            if r["team"] == team
        ]
        if (
            len(members) != 5
            or {m["lane"] for m in members} != set(LANES)
            or any(m["mmr"] is None for m in members)
        ):
            raise ApiError.from_status(500)
        result.append({"team": team, "name": f"{i}팀", "players": members})
    if len(players) != 10 or len({r["id"] for r in players}) != 10:
        raise ApiError.from_status(500)
    return result


async def input_snapshot(
    c: AsyncConnection, p: dict[str, Any], draft: dict[str, Any] | None, number: int, ids: set[str]
) -> dict[str, Any]:
    result: dict[str, Any] = {
        "setNumber": number,
        "sidePreset": "DEFAULT",
        "sideSelected": False,
        "winner": None,
        "picks": dict.fromkeys(sorted(ids)),
        "bans": {"BLUE": [None] * 5, "RED": [None] * 5},
        "patchVersion": None,
    }
    if draft is None:
        return result
    if draft["side_preset"] not in {"DEFAULT", "SWAPPED"}:
        raise ApiError.from_status(500)
    result.update(
        sidePreset=draft["side_preset"],
        sideSelected=draft["side_selected"] or draft["winning_side"] is not None,
        winner=draft["winning_side"],
        patchVersion=draft["patch_version"],
    )
    v = {**p, "input": draft["id"]}
    for r in await rows(
        c, "SELECT * FROM game_input_picks WHERE guild_id=:guild AND game_input_id=:input", v
    ):
        if r["user_id"] not in ids:
            raise ApiError.from_status(500)
        result["picks"][r["user_id"]] = r["champion_id"]
    for r in await rows(
        c, "SELECT * FROM game_input_bans WHERE guild_id=:guild AND game_input_id=:input", v
    ):
        if r["side"] not in {"BLUE", "RED"} or not 1 <= r["slot"] <= 5:
            raise ApiError.from_status(500)
        result["bans"][r["side"]][r["slot"] - 1] = r["champion_id"]
    return result


async def snapshot(
    c: AsyncConnection, p: dict[str, Any], *, result_page: bool = False
) -> dict[str, Any]:
    found = await rows(
        c, "SELECT * FROM series WHERE guild_id=:guild AND id=:id AND deleted_at IS NULL", p
    )
    if not found:
        raise ApiError(404, "series_not_found", "요청한 시리즈를 찾을 수 없습니다.")
    s = found[0]
    if s["status"] != ("COMPLETED" if result_page else "IN_PROGRESS"):
        raise error(
            "series_result_unavailable" if result_page else "series_not_in_progress",
            {"currentStatus": s["status"]},
        )
    ts = await teams(c, p)
    ids = {m["id"] for t in ts for m in t["players"]}
    records = await games(c, p)
    confirmed = []
    for g in records:
        draft = (
            await rows(
                c,
                "SELECT * FROM game_inputs WHERE guild_id=:guild AND id=:input",
                {**p, "input": g["source_input_id"]},
            )
        )[0]
        if result_page:
            draft.update(
                side_preset=g["side_preset"],
                winning_side=g["winning_side"],
                patch_version=g["patch_version"],
                side_selected=True,
            )
        item = await input_snapshot(c, p, draft, g["game_number"], ids)
        if result_page:
            item.pop("sideSelected")
            item.update(id=str(g["id"]), durationSeconds=g["duration_seconds"], mmrChanges=[])
            changes = await rows(
                c,
                "SELECT * FROM mmr_ledger_entries WHERE guild_id=:guild "
                "AND game_id=:game AND source_type='GAME_RESULT'",
                {**p, "game": g["id"]},
            )
            item["mmrChanges"] = [
                {
                    "userId": r["user_id"],
                    "lane": r["lane"],
                    "delta": r["delta"],
                    "mmrBefore": r["mmr_before"],
                    "mmrAfter": r["mmr_after"],
                }
                for r in sorted(changes, key=lambda r: (LANES.index(r["lane"]), r["user_id"]))
            ]
        confirmed.append(item)
    op = await operator(c, p)
    data = {
        "id": str(s["id"]),
        "displayNumber": s["display_number"],
        "title": s["title"],
        "bestOf": s["best_of"],
        "hardFearless": s["hard_fearless"],
        "status": s["status"],
        "revision": s["revision"],
        "sourceRecruitmentId": str(s["source_recruitment_id"]),
    }
    if result_page:
        data.update(
            winningTeam=s["winning_team"],
            terminalReason=s["terminal_reason"],
            startedAt=timestamp(s["started_at"]),
            endedAt=timestamp(s["ended_at"]) if s["ended_at"] else None,
        )
        return {
            "series": data,
            "teams": ts,
            "games": confirmed,
            "score": score(records),
            "capabilities": {"canRevertLastGame": op, "canDelete": op},
        }
    number = len(records) + 1
    if number > s["best_of"]:
        raise ApiError.from_status(500)
    drafts = await rows(
        c,
        "SELECT * FROM game_inputs WHERE guild_id=:guild AND series_id=:id "
        "AND set_number=:number AND status='DRAFT'",
        {**p, "number": number},
    )
    data["updatedAt"] = timestamp(s["updated_at"])
    return {
        "series": data,
        "teams": ts,
        "games": confirmed,
        "currentGame": await input_snapshot(c, p, drafts[0] if drafts else None, number, ids),
        "capabilities": {
            "canEdit": op,
            "canSubmit": op,
            "canRevertLastGame": op and bool(records),
            "canEarlyComplete": op and s["best_of"] in {3, 5} and bool(records),
            "canRevert": op,
            "canDelete": op,
        },
    }
