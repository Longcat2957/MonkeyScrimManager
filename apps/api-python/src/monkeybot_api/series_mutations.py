import uuid
from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.delivery_effects import audit, complete, invalidate, notification
from monkeybot_api.errors import ApiError
from monkeybot_api.profile import LANES
from monkeybot_api.recruitment_create import acquire
from monkeybot_api.recruitment_lifecycle import require_operator
from monkeybot_api.series_errors import error
from monkeybot_api.series_mmr import apply_mmr, revert_last
from monkeybot_api.series_pickban import apply, ensure_input, replace, validate_draft
from monkeybot_api.series_snapshot import games, input_snapshot, rows, score, snapshot, winner


async def bump(
    c: AsyncConnection,
    p: dict[str, Any],
    status: str,
    winning: str | None = None,
    reason: str | None = None,
) -> int:
    return int(
        await c.scalar(
            text(
                "UPDATE series SET status=:status,winning_team=:winning, "
                "terminal_reason=:reason,ended_at=:ended,revision=revision+1, "
                "updated_by=:actor_id,updated_at=:now "
                "WHERE guild_id=:guild AND id=:id RETURNING revision"
            ),
            {
                **p,
                "status": status,
                "winning": winning,
                "reason": reason,
                "ended": p["now"] if status != "IN_PROGRESS" else None,
            },
        )
        or 0
    )


async def result_notification(
    c: AsyncConnection, p: dict[str, Any], s: dict[str, Any], revision: int
) -> None:
    channel = await c.scalar(
        text("SELECT result_channel_id FROM guild_config WHERE guild_id=:guild"), p
    )
    if channel is None:
        return
    # Result cards use frozen game rows, not editable input rows or current MMR.
    roster = await rows(
        c,
        "SELECT sp.user_id AS id,sp.team,sp.lane,m.display_name FROM series_participants sp "
        "JOIN guild_members m ON m.guild_id=sp.guild_id AND m.user_id=sp.user_id "
        "WHERE sp.guild_id=:guild AND sp.series_id=:id",
        p,
    )
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
        picks = {
            r["user_id"]: r
            for r in await rows(
                c, "SELECT * FROM game_participants WHERE guild_id=:guild AND game_id=:game", v
            )
        }
        bans: dict[str, list[Any]] = {"blue": [None] * 5, "red": [None] * 5}
        for b in await rows(
            c, "SELECT * FROM game_bans WHERE guild_id=:guild AND game_id=:game", v
        ):
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
    current = (await rows(c, "SELECT * FROM series WHERE guild_id=:guild AND id=:id", p))[0]
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
    state = await c.scalar(text("SELECT status FROM guilds WHERE guild_id=:guild FOR UPDATE"), p)
    if state is None:
        raise ApiError(404, "not_found", "요청한 대상을 찾을 수 없습니다.")
    if state != "ACTIVE":
        raise ApiError(
            409,
            "guild_inactive",
            "현재 길드에서는 command를 실행할 수 없습니다.",
            {"currentStatus": state},
        )
    role = await c.scalar(
        text("SELECT operator_role_id FROM guild_config WHERE guild_id=:guild FOR UPDATE"), p
    )
    if role is None:
        raise ApiError(409, "guild_setup_required", "길드 초기 설정을 먼저 완료해야 합니다.")
    member = await c.scalar(
        text(
            "SELECT status FROM guild_members WHERE guild_id=:guild AND user_id=:actor_id "
            "FOR UPDATE"
        ),
        p,
    )
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
        source = await c.scalar(
            text(
                "SELECT source_recruitment_id FROM series WHERE guild_id=:guild AND "
                "id=:id AND deleted_at IS NULL"
            ),
            p,
        )
        if source is None:
            raise ApiError(404, "not_found", "요청한 대상을 찾을 수 없습니다.")
        recruitment = (
            await rows(
                c,
                "SELECT * FROM recruitments WHERE guild_id=:guild AND id=:source FOR UPDATE",
                {**p, "source": source},
            )
        )[0]
    ss = await rows(
        c,
        "SELECT * FROM series WHERE guild_id=:guild AND id=:id AND deleted_at IS NULL FOR UPDATE",
        p,
    )
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
        revision = await bump(c, p, "CANCELLED", None, "CANCELLED")
        restored = "CLOSED" if action == "revert" else "CANCELLED"
        source = str(recruitment["id"])
        await c.execute(
            text(
                "UPDATE recruitments SET status=:status,revision=revision+1,updated_by=:actor_id, "
                "updated_at=:now,converted_at=NULL,cancelled_at=:cancelled WHERE "
                "guild_id=:guild AND id=:source"
            ),
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
        roster = await rows(
            c,
            "SELECT user_id,team,lane FROM series_participants WHERE guild_id=:guild "
            "AND series_id=:id ORDER BY user_id,lane FOR UPDATE",
            p,
        )
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
        await c.execute(
            text(
                "UPDATE game_inputs SET updated_by=:actor_id,updated_at=:now WHERE "
                "guild_id=:guild AND id=:input"
            ),
            v,
        )
        await validate_draft(c, p, s["hard_fearless"])
        if action == "pickban":
            revision = await bump(c, p, "IN_PROGRESS")
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
        draft = (await rows(c, "SELECT * FROM game_inputs WHERE guild_id=:guild AND id=:input", v))[
            0
        ]
        picked = await input_snapshot(c, p, draft, number, ids)
        game_id = uuid.uuid7()
        v.update(game=game_id, season=s["season_id"], number=number, **body)
        await c.execute(
            text("""INSERT INTO games (guild_id,id,season_id,series_id,source_input_id,
            game_number,side_preset,winning_side,patch_version,status,mmr_status,played_at,confirmed_by,
            confirmed_by_type,confirmed_at) VALUES
            (:guild,:game,:season,:id,:input,:number,:sidePreset,
            :winner,:patchVersion,'CONFIRMED','APPLIED',:now,:actor_id,'USER',:now)"""),
            v,
        )
        for r in roster:
            await c.execute(
                text(
                    "INSERT INTO game_participants "
                    "(guild_id,game_id,user_id,team,lane,champion_id) "
                    "VALUES (:guild,:game,:user_id,:team,:lane,:champion)"
                ),
                {**v, **r, "champion": picked["picks"][r["user_id"]]},
            )
        for side, slots in picked["bans"].items():
            for i, champ in enumerate(slots, 1):
                if champ is not None:
                    await c.execute(
                        text(
                            "INSERT INTO game_bans (guild_id,game_id,side,slot,champion_id) "
                            "VALUES (:guild,:game,:side,:slot,:champion)"
                        ),
                        {**v, "side": side, "slot": i, "champion": champ},
                    )
        await apply_mmr(c, v, roster, winner(body["sidePreset"], body["winner"]))
        await c.execute(
            text("UPDATE game_inputs SET status='SUBMITTED' WHERE guild_id=:guild AND id=:input"), v
        )
        sc = score(await games(c, p))
        winning = (
            "TEAM_1"
            if sc["team1"] >= s["best_of"] // 2 + 1
            else "TEAM_2"
            if sc["team2"] >= s["best_of"] // 2 + 1
            else None
        )
        status = "COMPLETED" if winning else "IN_PROGRESS"
        revision = await bump(c, p, status, winning, "SCORE_REACHED" if winning else None)
        count += 1
        before = {"seriesId": identifier, "seriesRevision": s["revision"], "setNumber": number}
        event, target_type, target = "SERIES_GAME_CONFIRMED", "SERIES_GAME", str(game_id)
    elif action == "last":
        game = await revert_last(c, p)
        status = "IN_PROGRESS"
        revision = await bump(c, p, status)
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
        revision = await bump(c, p, status, winning, reason)
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
