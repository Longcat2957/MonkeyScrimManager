import base64
import binascii
import json
import re
import uuid
from datetime import UTC, datetime, timedelta
from typing import Any

from fastapi import APIRouter, Request
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection
from starlette.responses import Response

from monkeybot_api.config import Settings, is_snowflake
from monkeybot_api.context import hidden_guild, timestamp
from monkeybot_api.database import Database
from monkeybot_api.errors import ApiError, success
from monkeybot_api.json_body import unique_json_fields
from monkeybot_api.recruitment_queries import ACTIONABLE_SQL
from monkeybot_api.recruitment_snapshot import recruitment_snapshot
from monkeybot_api.session import resolve_session

EPOCH = datetime(1970, 1, 1, tzinfo=UTC)
PG_EPOCH = datetime(2000, 1, 1, tzinfo=UTC)


def encode_cursor(updated_at: datetime, record_id: uuid.UUID) -> str:
    delta = updated_at - EPOCH
    nanos = ((delta.days * 86400 + delta.seconds) * 1_000_000 + delta.microseconds) * 1000
    if not -(2**63) <= nanos < 2**63:
        raise ApiError.from_status(500)
    payload = json.dumps({"updatedAtNanos": nanos, "id": str(record_id)}, separators=(",", ":"))
    return base64.urlsafe_b64encode(payload.encode()).decode().rstrip("=")


def decode_cursor(value: str) -> tuple[datetime, uuid.UUID]:
    try:
        if not re.fullmatch(r"[A-Za-z0-9_-]+", value):
            raise ValueError("invalid base64url")
        raw = base64.b64decode(value + "=" * (-len(value) % 4), altchars=b"-_", validate=True)
        if base64.urlsafe_b64encode(raw).decode().rstrip("=") != value:
            raise ValueError("noncanonical base64url")
        token = json.loads(raw, object_pairs_hook=unique_json_fields)
        if not isinstance(token, dict) or set(token) != {"updatedAtNanos", "id"}:
            raise ValueError("invalid cursor fields")
        nanos = token["updatedAtNanos"]
        if type(nanos) is not int or not -(2**63) <= nanos < 2**63:
            raise ValueError("invalid cursor timestamp")
        record_id = uuid.UUID(token["id"])
        # SQLx encodes time as whole microseconds since PostgreSQL's 2000 epoch.
        pg_nanos = nanos - 946684800 * 1_000_000_000
        micros = (abs(pg_nanos) // 1000) * (-1 if pg_nanos < 0 else 1)
        return PG_EPOCH + timedelta(microseconds=micros), record_id
    except ValueError, TypeError, AttributeError, binascii.Error, ApiError:
        raise ApiError(400, "invalid_cursor", "cursor 형식이 올바르지 않습니다.") from None


def actionable_query(request: Request) -> dict[str, str]:
    pairs = list(request.query_params.multi_items())
    query = dict(pairs)
    if len(query) != len(pairs) or set(query) - {"state", "cursor", "limit"}:
        raise ApiError.from_status(400)
    if query.get("state") != "actionable":
        raise ApiError.from_status(400)
    if "limit" in query and (
        not re.fullmatch(r"\+?[0-9]+", query["limit"]) or int(query["limit"]) > 65535
    ):
        raise ApiError.from_status(400)
    return query


async def require_guild_access(connection: AsyncConnection, user_id: str, guild_id: str) -> None:
    accessible = await connection.scalar(
        text("""
        SELECT EXISTS (
            SELECT 1 FROM guild_members membership
            JOIN guilds guild ON guild.guild_id = membership.guild_id AND guild.status = 'ACTIVE'
            WHERE membership.guild_id = :guild_id AND membership.user_id = :user_id
              AND membership.status = 'ACTIVE'
        )
    """),
        {"guild_id": guild_id, "user_id": user_id},
    )
    if not accessible:
        raise hidden_guild()


def recruitment_summary(row: dict[str, Any]) -> dict[str, Any]:
    return {
        "id": str(row["id"]),
        "displayNumber": row["display_number"],
        "title": row["title"],
        "status": row["status"],
        "targetCount": row["target_count"],
        "bestOf": row["best_of"],
        "hardFearless": row["hard_fearless"],
        "revision": row["revision"],
        "updatedAt": timestamp(row["updated_at"]),
    }


def router(settings: Settings, database: Database) -> APIRouter:
    routes = APIRouter()

    @routes.api_route("/api/guilds/{guild_id}/recruitments", methods=["GET", "HEAD"])
    async def recruitments(request: Request, guild_id: str) -> Response:
        if not is_snowflake(guild_id):
            raise ApiError.from_status(400)
        query = actionable_query(request)
        principal = await resolve_session(request, database, settings)
        limit = int(query.get("limit", "50"))
        if not 1 <= limit <= 100:
            raise ApiError(400, "invalid_limit", "limit은 1 이상 100 이하이어야 합니다.")
        cursor_time, cursor_id = (
            decode_cursor(query["cursor"]) if "cursor" in query else (None, None)
        )
        async with database.connection() as connection:
            await require_guild_access(connection, principal["user_id"], guild_id)
            rows = (
                (
                    await connection.execute(
                        text(ACTIONABLE_SQL),
                        {
                            "user_id": principal["user_id"],
                            "guild_id": guild_id,
                            "cursor_time": cursor_time,
                            "cursor_id": cursor_id,
                            "fetch_limit": limit + 1,
                        },
                    )
                )
                .mappings()
                .all()
            )
        page = rows[:limit]
        next_cursor = None
        if len(rows) > limit:
            next_cursor = encode_cursor(page[-1]["updated_at"], page[-1]["id"])
        items = []
        for row in page:
            href = f"/g/{guild_id}/recruitments/{row['id']}"
            items.append(
                {
                    **recruitment_summary(dict(row)),
                    "joinedCount": row["participant_count"],
                    "assignedCount": row["assigned_count"],
                    "href": href + ("/entry" if row["status"] == "CLOSED" else ""),
                }
            )
        return success(request, {"recruitments": items, "nextCursor": next_cursor})

    @routes.api_route(
        "/api/guilds/{guild_id}/recruitments/{recruitment_id}", methods=["GET", "HEAD"]
    )
    async def recruitment(request: Request, guild_id: str, recruitment_id: str) -> Response:
        if not is_snowflake(guild_id):
            raise ApiError.from_status(400)
        try:
            record_id = uuid.UUID(recruitment_id)
        except ValueError:
            raise ApiError.from_status(400) from None
        principal = await resolve_session(request, database, settings)
        async with database.connection() as connection:
            await require_guild_access(connection, principal["user_id"], guild_id)
        async with database.connection(transaction=True) as connection:
            snapshot = await recruitment_snapshot(connection, guild_id, record_id)
            if snapshot["recruitment"]["status"] not in {"OPEN", "CLOSED", "CANCELLED"}:
                raise ApiError(404, "not_found", "요청한 모집을 찾을 수 없습니다.")
        return success(request, snapshot)

    return routes
