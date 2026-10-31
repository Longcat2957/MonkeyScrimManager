import uuid
from typing import Any

from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.pagination import decode_cursor, encode_cursor
from monkeybot_api.core.time import timestamp
from monkeybot_api.db.database import Database
from monkeybot_api.repositories.guild_access import require_guild_access
from monkeybot_api.repositories.series import queries as repository
from monkeybot_api.repositories.series.snapshots import snapshot


async def read_series(
    database: Database,
    guild_id: str,
    identifier: uuid.UUID,
    actor: str,
    result: bool,
) -> dict[str, Any]:
    async with database.connection() as c:
        await require_guild_access(c, actor, guild_id)
    p = {"guild": guild_id, "id": identifier, "actor_id": actor}
    try:
        async with database.connection(transaction=True) as c:
            await repository.read_only_snapshot(c)
            value = await snapshot(c, p, result_page=result)
    except ApiError as exc:
        if exc.code == "series_not_found" or (not result and exc.code == "series_not_in_progress"):
            raise ApiError(
                404,
                "not_found",
                "요청한 완료 시리즈 결과를 찾을 수 없습니다."
                if result
                else "요청한 진행 중 시리즈를 찾을 수 없습니다.",
            ) from None
        if result and exc.code == "series_result_unavailable":
            raise ApiError(
                409,
                exc.code,
                "시리즈가 아직 완료되지 않아 결과를 확인할 수 없습니다.",
                exc.details,
            ) from None
        raise
    return value


async def list_series(
    database: Database, guild_id: str, actor: str, q: dict[str, str]
) -> dict[str, Any]:
    limit = int(q.get("limit", "50"))
    if not 1 <= limit <= 100:
        raise ApiError(400, "invalid_limit", "limit은 1 이상 100 이하이어야 합니다.")
    cursor_time, cursor_id = decode_cursor(q["cursor"]) if "cursor" in q else (None, None)
    async with database.connection() as c:
        await require_guild_access(c, actor, guild_id)
        found = await repository.list_series(
            c,
            {
                "p1": actor,
                "p2": guild_id,
                "p3": q["state"],
                "p4": cursor_time,
                "p5": cursor_id,
                "p6": limit + 1,
            },
        )
    page = found[:limit]
    result: list[dict[str, Any]] = []
    for r in page:
        active = r["status"] == "IN_PROGRESS"
        outcome = (
            None
            if active
            else "DRAW"
            if r["terminal_reason"] == "EARLY_DRAW"
            else r["winning_team"]
        )
        if (r["status"], r["terminal_reason"], r["winning_team"]) not in {
            ("IN_PROGRESS", None, None),
            ("COMPLETED", "EARLY_DRAW", None),
            ("COMPLETED", "EARLY_WIN", "TEAM_1"),
            ("COMPLETED", "EARLY_WIN", "TEAM_2"),
            ("COMPLETED", "SCORE_REACHED", "TEAM_1"),
            ("COMPLETED", "SCORE_REACHED", "TEAM_2"),
        }:
            raise ApiError.from_status(500)
        number = r["team_1_score"] + r["team_2_score"] + 1
        result.append(
            {
                "id": str(r["id"]),
                "displayNumber": r["display_number"],
                "title": r["title"],
                "bestOf": r["best_of"],
                "hardFearless": r["hard_fearless"],
                "status": r["status"],
                "team1Score": r["team_1_score"],
                "team2Score": r["team_2_score"],
                "winningTeam": r["winning_team"],
                "terminalReason": r["terminal_reason"],
                "currentSetNumber": number if active and number <= r["best_of"] else None,
                "outcome": outcome,
                "updatedAt": timestamp(r["updated_at"]),
                "endedAt": timestamp(r["ended_at"]) if r["ended_at"] else None,
                "href": f"/g/{guild_id}/series/{r['id']}/" + ("pick-ban" if active else "result"),
            }
        )
    return {
        "series": result,
        "nextCursor": encode_cursor(page[-1]["updated_at"], page[-1]["id"])
        if len(found) > limit
        else None,
    }
