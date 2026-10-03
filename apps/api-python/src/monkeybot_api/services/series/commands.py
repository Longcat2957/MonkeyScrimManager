import hashlib
import json
import uuid
from datetime import UTC, datetime, timedelta
from typing import Any

from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError
from monkeybot_api.db.database import Database
from monkeybot_api.db.transactions import mutation
from monkeybot_api.domain.series.errors import error
from monkeybot_api.repositories.effects import audit, complete, invalidate, notification
from monkeybot_api.repositories.guild_access import require_operator
from monkeybot_api.repositories.idempotency import acquire
from monkeybot_api.repositories.series import commands as repository
from monkeybot_api.repositories.series.pickban import ensure_input
from monkeybot_api.repositories.series.snapshots import (
    games,
    input_snapshot,
    score,
    snapshot,
    winner,
)
from monkeybot_api.schemas.series import validate
from monkeybot_api.schemas.users import LANES
from monkeybot_api.services.series.mmr import apply_mmr, revert_last
from monkeybot_api.services.series.pickban import apply, replace, validate_draft


async def result_notification(
    c: AsyncConnection, p: dict[str, Any], s: dict[str, Any], revision: int
) -> None:
    channel = await repository.result_channel(c, p)
    if channel is None:
        return
    # Result cards use frozen game rows, not editable input rows or current MMR.
    roster = await repository.result_roster(c, p)
    ts: list[dict[str, Any]] = [
        {
            "team": t,
            "name": f"{i}팀",
            "players": [
                {"id": r["id"], "displayName": r["display_name"], "lane": r["lane"]}
                for r in sorted(roster, key=lambda r: LANES.index(r["lane"]))
                if r["team"] == t
            ],
        }
        for i, t in enumerate(("TEAM_1", "TEAM_2"), 1)
    ]
    records = await games(c, p)
    gs = []
    for g in records:
        v = {**p, "game": g["id"]}
        picks = {r["user_id"]: r for r in await repository.game_participants(c, v)}
        bans: dict[str, list[Any]] = {"blue": [None] * 5, "red": [None] * 5}
        for b in await repository.game_bans(c, v):
            bans[b["side"].lower()][b["slot"] - 1] = b["champion_id"]
        gt = [
            {
                "team": t["team"],
                "name": t["name"],
                "picks": [{**r, "championId": picks[r["id"]]["champion_id"]} for r in t["players"]],
            }
            for t in ts
        ]
        gs.append(
            {
                "setNumber": g["game_number"],
                "winner": winner(g["side_preset"], g["winning_side"]),
                "sidePreset": g["side_preset"],
                "team1": gt[0],
                "team2": gt[1],
                "bans": bans,
                "patchVersion": g["patch_version"],
            }
        )
    current = (await repository.series_state(c, p))[0]
    data = {
        "series": {
            "id": str(p["id"]),
            "displayNumber": s["display_number"],
            "title": s["title"],
            "bestOf": s["best_of"],
            "hardFearless": s["hard_fearless"],
            "winningTeam": current["winning_team"],
            "terminalReason": current["terminal_reason"],
        },
        "teams": ts,
        "score": score(records),
        "games": gs,
        "links": {"result": f"/g/{p['guild']}/series/{p['id']}/result"},
    }
    await notification(
        c,
        p,
        "SERIES_RESULT_CREATE",
        channel,
        f"series:{p['id']}:result:{revision}",
        {"schemaVersion": 2, "seriesId": str(p["id"]), "snapshot": data},
    )


async def mutate(
    c: AsyncConnection, p: dict[str, Any], action: str, body: dict[str, Any]
) -> dict[str, Any]:
    state = await repository.lock_guild_status(c, p)
    if state is None:
        raise ApiError(404, "not_found", "요청한 대상을 찾을 수 없습니다.")
    if state != "ACTIVE":
        raise ApiError(
            409,
            "guild_inactive",
            "현재 길드에서는 command를 실행할 수 없습니다.",
            {"currentStatus": state},
        )
    role = await repository.lock_operator_role(c, p)
    if role is None:
        raise ApiError(409, "guild_setup_required", "길드 초기 설정을 먼저 완료해야 합니다.")
    member = await repository.lock_member_status(c, p)
    if member != "ACTIVE":
        message = "해당 길드의 활성 멤버만 이 작업을 수행할 수 있습니다."
        if action == "pickban":
            message = "해당 길드의 활성 멤버만 픽밴 상태를 확인할 수 있습니다."
        if action in {"revert", "delete"}:
            message = "해당 길드의 활성 멤버만 시리즈 상태를 변경할 수 있습니다."
        raise ApiError(403, "guild_membership_required", message)
    await require_operator(c, p, role, None)
    if action != "pickban":
        try:
            replay = await acquire(c, p, False)
        except ApiError as e:
            if e.code in {"idempotency_conflict", "idempotency_in_progress"}:
                label = (
                    "게임 결과"
                    if action in {"submit", "last"}
                    else "시리즈 조기 종료"
                    if action == "complete"
                    else "시리즈 상태 변경"
                )
                message = f"같은 {label} 요청을 처리하고 있습니다."
                if e.code == "idempotency_conflict":
                    message = (
                        "같은 Idempotency-Key가 다른 요청에 사용되었습니다."
                        if action in {"revert", "delete"}
                        else f"같은 Idempotency-Key가 다른 {label} 요청에 사용되었습니다."
                    )
                raise ApiError(409, e.code, message) from None
            raise
        if replay is not None:
            return replay
    result: dict[str, Any]
    recruitment = None
    if action in {"revert", "delete"}:
        source_id = await repository.source_recruitment(c, p)
        if source_id is None:
            raise ApiError(404, "not_found", "요청한 대상을 찾을 수 없습니다.")
        recruitment = (await repository.lock_recruitment(c, {**p, "source": source_id}))[0]
    ss = await repository.lock_series(c, p)
    if not ss:
        raise ApiError(404, "not_found", "요청한 대상을 찾을 수 없습니다.")
    s = ss[0]
    if s["revision"] != body["expectedRevision"]:
        raise error(
            "series_revision_conflict",
            {"expectedRevision": body["expectedRevision"], "actualRevision": s["revision"]},
        )
    allowed = (
        {"IN_PROGRESS", "COMPLETED"} if action in {"last", "revert", "delete"} else {"IN_PROGRESS"}
    )
    if s["status"] not in allowed:
        raise error("series_not_in_progress", {"currentStatus": s["status"]})
    records = await games(c, p)
    count = len(records)
    identifier = str(p["id"])
    before: dict[str, Any]
    after: dict[str, Any]
    if recruitment is not None:
        if recruitment["status"] != "CONVERTED":
            raise error("series_source_not_converted", {"currentStatus": recruitment["status"]})
        for _ in records:
            await revert_last(c, p)
        revision = await repository.bump(c, p, "CANCELLED", None, "CANCELLED")
        restored = "CLOSED" if action == "revert" else "CANCELLED"
        source = str(recruitment["id"])
        await repository.restore_recruitment(
            c,
            {
                **p,
                "source": recruitment["id"],
                "status": restored,
                "cancelled": p["now"] if action == "delete" else None,
            },
        )
        result = {
            "nextHref": f"/g/{p['guild']}/recruitments/{source}"
            + ("/entry" if action == "revert" else "")
        }
        before = {
            "seriesRevision": s["revision"],
            "seriesStatus": s["status"],
            "confirmedGameCount": count,
            "sourceRecruitmentId": source,
            "sourceRecruitmentRevision": recruitment["revision"],
            "sourceRecruitmentStatus": recruitment["status"],
        }
        after = {
            "seriesRevision": revision,
            "seriesStatus": "CANCELLED",
            "revertedGameCount": count,
            "sourceRecruitmentId": source,
            "sourceRecruitmentRevision": recruitment["revision"] + 1,
            "sourceRecruitmentStatus": restored,
            **result,
        }
        event = "SERIES_REVERTED" if action == "revert" else "SERIES_CANCELLED"
        await audit(c, p, event, "SERIES", identifier, before, after)
        await invalidate(c, p, "SERIES", identifier, revision, event)
        await invalidate(
            c,
            p,
            "RECRUITMENT",
            source,
            recruitment["revision"] + 1,
            "RECRUITMENT_RESTORED_FROM_SERIES"
            if action == "revert"
            else "RECRUITMENT_CANCELLED_FROM_SERIES",
        )
        await complete(c, p, result, 200)
        return result
    if action in {"pickban", "submit"}:
        roster = await repository.lock_roster(c, p)
        ids = {r["user_id"] for r in roster}
        if (
            len(roster) != 10
            or len(ids) != 10
            or {(r["team"], r["lane"]) for r in roster}
            != {(t, lane) for t in ("TEAM_1", "TEAM_2") for lane in LANES}
        ):
            raise ApiError.from_status(500)
        number = count + 1
        if number > s["best_of"]:
            raise error("series_no_current_set")
        if action == "submit" and body["setNumber"] != number:
            raise error(
                "series_set_not_current",
                {"expectedSetNumber": number, "actualSetNumber": body["setNumber"]},
            )
        draft = await ensure_input(c, p, number)
        v = {**p, "input": draft["id"]}
        if action == "pickban":
            await apply(c, v, body["operation"], ids)
        else:
            await replace(c, v, draft, body, ids)
        await repository.touch_input(c, v)
        await validate_draft(c, p, s["hard_fearless"])
        if action == "pickban":
            revision = await repository.bump(c, p, "IN_PROGRESS")
            result = await snapshot(c, p)
            await audit(
                c,
                p,
                "SERIES_PICKBAN_UPDATED",
                "SERIES_PICKBAN_DRAFT",
                identifier,
                {
                    "revision": s["revision"],
                    "setNumber": number,
                    "hardFearless": s["hard_fearless"],
                },
                {"revision": revision, "operation": body["operation"]},
            )
            await invalidate(c, p, "SERIES", identifier, revision, "SERIES_PICKBAN_UPDATED")
            return result
        draft = (await repository.game_input(c, v))[0]
        picked = await input_snapshot(c, p, draft, number, ids)
        game_id = uuid.uuid7()
        v.update(game=game_id, season=s["season_id"], number=number, **body)
        await repository.insert_game(c, v)
        for r in roster:
            await repository.insert_game_participant(
                c, {**v, **r, "champion": picked["picks"][r["user_id"]]}
            )
        for side, slots in picked["bans"].items():
            for i, champ in enumerate(slots, 1):
                if champ is not None:
                    await repository.insert_game_ban(
                        c, {**v, "side": side, "slot": i, "champion": champ}
                    )
        await apply_mmr(c, v, roster, winner(body["sidePreset"], body["winner"]))
        await repository.submit_input(c, v)
        sc = score(await games(c, p))
        winning = (
            "TEAM_1"
            if sc["team1"] >= s["best_of"] // 2 + 1
            else "TEAM_2"
            if sc["team2"] >= s["best_of"] // 2 + 1
            else None
        )
        status = "COMPLETED" if winning else "IN_PROGRESS"
        revision = await repository.bump(
            c, p, status, winning, "SCORE_REACHED" if winning else None
        )
        count += 1
        before = {"seriesId": identifier, "seriesRevision": s["revision"], "setNumber": number}
        event, target_type, target = "SERIES_GAME_CONFIRMED", "SERIES_GAME", str(game_id)
    elif action == "last":
        game = await revert_last(c, p)
        status = "IN_PROGRESS"
        revision = await repository.bump(c, p, status)
        count -= 1
        before = {
            "seriesId": identifier,
            "seriesRevision": s["revision"],
            "setNumber": game["game_number"],
        }
        event, target_type, target = "SERIES_GAME_REVERTED", "SERIES_GAME", str(game["id"])
    else:
        if s["best_of"] not in {3, 5} or count == 0:
            raise error(
                "series_early_completion_unavailable",
                {"bestOf": s["best_of"], "confirmedGameCount": count},
            )
        sc = score(records)
        if body["outcome"] == "DRAW" and sc["team1"] != sc["team2"]:
            raise ApiError(
                409,
                "early_draw_requires_tied_score",
                "무승부 조기 종료는 현재 스코어가 동률일 때만 가능합니다.",
                {"team1Score": sc["team1"], "team2Score": sc["team2"]},
            )
        winning = None if body["outcome"] == "DRAW" else body["outcome"]
        reason = "EARLY_WIN" if winning else "EARLY_DRAW"
        status = "COMPLETED"
        revision = await repository.bump(c, p, status, winning, reason)
        before = {
            "seriesRevision": s["revision"],
            "bestOf": s["best_of"],
            "confirmedGameCount": count,
            "score": sc,
        }
        event, target_type, target = "SERIES_EARLY_COMPLETED", "SERIES", identifier
    result = {
        "series": {"id": identifier, "status": status, "revision": revision},
        "confirmedGameCount": count,
        "nextHref": f"/g/{p['guild']}/series/{identifier}/"
        + ("result" if status == "COMPLETED" else "pick-ban"),
    }
    if status == "COMPLETED":
        await result_notification(c, p, s, revision)
    after = result
    if action == "complete":
        after = {
            "seriesRevision": revision,
            "status": status,
            "outcome": body["outcome"],
            "winningTeam": winning,
            "terminalReason": reason,
            "confirmedGameCount": count,
            "nextHref": result["nextHref"],
        }
    await audit(c, p, event, target_type, target, before, after)
    await invalidate(c, p, "SERIES", identifier, revision, event)
    await complete(c, p, result, 201 if action == "submit" else 200)
    return result


async def execute_command(
    database: Database,
    guild_id: str,
    identifier: uuid.UUID,
    actor: str,
    request_id: str,
    method: str,
    action: str,
    key: str,
    body: dict[str, Any],
) -> dict[str, Any]:
    validate(body, action)
    if body.get("picks") is not None:
        body["picks"] = dict(sorted(body["picks"].items()))
    command = {"seriesId": str(identifier), **body}
    if action in {"revert", "delete"}:
        command["action"] = action.upper()
    now = datetime.now(UTC)
    suffix = {
        "pickban": "/pickban",
        "submit": "/games",
        "last": "/games/last",
        "complete": "/complete",
        "revert": "/revert",
        "delete": "",
    }[action]
    p = {
        "guild": guild_id,
        "id": identifier,
        "actor_id": actor,
        "now": now,
        "request_id": request_id,
        "audit_id": uuid.uuid7(),
        "idem_id": uuid.uuid7(),
        "principal_type": "USER",
        "principal": actor,
        "method": method,
        "route": "/api/guilds/{guildId}/series/{id}" + suffix,
        "key": key,
        "expires": now + timedelta(hours=24),
        "hash": hashlib.sha256(
            json.dumps([guild_id, command], ensure_ascii=False, separators=(",", ":")).encode()
        ).digest(),
    }
    return await mutation(database, lambda c: mutate(c, p, action, body))
