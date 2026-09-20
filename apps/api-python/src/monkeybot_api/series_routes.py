import asyncio
import hashlib
import json
import re
import uuid
from datetime import UTC, datetime, timedelta
from typing import Any

from fastapi import APIRouter, Request
from pydantic import BaseModel
from sqlalchemy import text
from sqlalchemy.exc import DBAPIError
from starlette.responses import Response

from monkeybot_api.auth import require_csrf
from monkeybot_api.config import Settings, is_snowflake
from monkeybot_api.context import timestamp
from monkeybot_api.database import Database
from monkeybot_api.errors import ApiError, success
from monkeybot_api.json_body import read_json
from monkeybot_api.recruitment_create import command_key
from monkeybot_api.recruitment_lifecycle import Revision
from monkeybot_api.series_contracts import Early, PickBan, Submit, invalid, validate
from monkeybot_api.series_mutations import mutate
from monkeybot_api.series_queries import LIST
from monkeybot_api.series_snapshot import rows, snapshot
from monkeybot_api.session import resolve_session
from monkeybot_api.workflow import decode_cursor, encode_cursor, require_guild_access


def path_ids(guild: str, identifier: str) -> uuid.UUID:
    try:
        if not is_snowflake(guild):
            raise ValueError("guild")
        return uuid.UUID(identifier)
    except ValueError:
        raise ApiError.from_status(400) from None


def router(settings: Settings, database: Database) -> APIRouter:
    routes = APIRouter()

    async def execute(request: Request, guild_id: str, series_id: str, action: str) -> Response:
        identifier = path_ids(guild_id, series_id)
        principal = await resolve_session(request, database, settings)
        models: dict[str, type[BaseModel]] = {
            "pickban": PickBan,
            "submit": Submit,
            "complete": Early,
        }
        model = models.get(action, Revision)
        parsed: BaseModel = await read_json(request, model)
        body = parsed.model_dump()
        if body["expectedRevision"] < 0:
            raise invalid(action)
        require_csrf(request, settings, principal["csrf_secret_hash"])
        key = command_key(request, False) if action != "pickban" else ""
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
            "actor_id": principal["user_id"],
            "now": now,
            "request_id": request.state.request_id,
            "audit_id": uuid.uuid7(),
            "idem_id": uuid.uuid7(),
            "principal_type": "USER",
            "principal": principal["user_id"],
            "method": request.method,
            "route": "/api/guilds/{guildId}/series/{id}" + suffix,
            "key": key,
            "expires": now + timedelta(hours=24),
            "hash": hashlib.sha256(
                json.dumps([guild_id, command], ensure_ascii=False, separators=(",", ":")).encode()
            ).digest(),
        }
        for attempt in range(3):
            try:
                async with database.connection(transaction=True) as c:
                    result = await mutate(c, p, action, body)
                response = success(request, result)
                response.status_code = 201 if action == "submit" else 200
                return response
            except DBAPIError as exc:
                if getattr(exc.orig, "sqlstate", "") != "40P01":
                    raise
                if attempt == 2:
                    raise ApiError(
                        503,
                        "database_busy",
                        "데이터베이스가 사용 중입니다. 잠시 후 다시 시도해 주세요.",
                    ) from None
                await asyncio.sleep(0.025 * 2**attempt)
        raise AssertionError("unreachable")

    @routes.patch("/api/guilds/{guild_id}/series/{series_id}/pickban")
    async def pickban(request: Request, guild_id: str, series_id: str) -> Response:
        return await execute(request, guild_id, series_id, "pickban")

    @routes.post("/api/guilds/{guild_id}/series/{series_id}/games")
    async def submit(request: Request, guild_id: str, series_id: str) -> Response:
        return await execute(request, guild_id, series_id, "submit")

    @routes.delete("/api/guilds/{guild_id}/series/{series_id}/games/last")
    async def last(request: Request, guild_id: str, series_id: str) -> Response:
        return await execute(request, guild_id, series_id, "last")

    @routes.post("/api/guilds/{guild_id}/series/{series_id}/complete")
    async def early(request: Request, guild_id: str, series_id: str) -> Response:
        return await execute(request, guild_id, series_id, "complete")

    @routes.post("/api/guilds/{guild_id}/series/{series_id}/revert")
    async def revert(request: Request, guild_id: str, series_id: str) -> Response:
        return await execute(request, guild_id, series_id, "revert")

    @routes.delete("/api/guilds/{guild_id}/series/{series_id}")
    async def delete(request: Request, guild_id: str, series_id: str) -> Response:
        return await execute(request, guild_id, series_id, "delete")

    async def read(request: Request, guild_id: str, series_id: str, result: bool) -> Response:
        identifier = path_ids(guild_id, series_id)
        principal = await resolve_session(request, database, settings)
        async with database.connection() as c:
            await require_guild_access(c, principal["user_id"], guild_id)
        p = {"guild": guild_id, "id": identifier, "actor_id": principal["user_id"]}
        try:
            async with database.connection(transaction=True) as c:
                await c.execute(text("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY"))
                value = await snapshot(c, p, result_page=result)
        except ApiError as exc:
            if exc.code == "series_not_found" or (
                not result and exc.code == "series_not_in_progress"
            ):
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
        return success(request, value)

    @routes.api_route("/api/guilds/{guild_id}/series/{series_id}", methods=["GET", "HEAD"])
    async def detail(request: Request, guild_id: str, series_id: str) -> Response:
        return await read(request, guild_id, series_id, False)

    @routes.api_route("/api/guilds/{guild_id}/series/{series_id}/result", methods=["GET", "HEAD"])
    async def result(request: Request, guild_id: str, series_id: str) -> Response:
        return await read(request, guild_id, series_id, True)

    @routes.api_route("/api/guilds/{guild_id}/series", methods=["GET", "HEAD"])
    async def listing(request: Request, guild_id: str) -> Response:
        if not is_snowflake(guild_id):
            raise ApiError.from_status(400)
        pairs = list(request.query_params.multi_items())
        q = dict(pairs)
        if (
            len(pairs) != len(q)
            or set(q) - {"state", "seasonId", "cursor", "limit"}
            or q.get("state") not in {"active", "completed"}
            or (
                "limit" in q
                and (not re.fullmatch(r"\+?[0-9]+", q["limit"]) or int(q["limit"]) > 65535)
            )
        ):
            raise ApiError.from_status(400)
        principal = await resolve_session(request, database, settings)
        limit = int(q.get("limit", "50"))
        if not 1 <= limit <= 100:
            raise ApiError(400, "invalid_limit", "limit은 1 이상 100 이하이어야 합니다.")
        cursor_time, cursor_id = decode_cursor(q["cursor"]) if "cursor" in q else (None, None)
        async with database.connection() as c:
            await require_guild_access(c, principal["user_id"], guild_id)
            found = await rows(
                c,
                LIST,
                {
                    "p1": principal["user_id"],
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
                    "href": f"/g/{guild_id}/series/{r['id']}/"
                    + ("pick-ban" if active else "result"),
                }
            )
        return success(
            request,
            {
                "series": result,
                "nextCursor": encode_cursor(page[-1]["updated_at"], page[-1]["id"])
                if len(found) > limit
                else None,
            },
        )

    return routes
