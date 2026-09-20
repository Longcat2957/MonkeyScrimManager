import hashlib
import json
import uuid
from datetime import UTC, datetime, timedelta
from typing import Annotated, Any, Literal

from fastapi import APIRouter, Request
from pydantic import AfterValidator, Field
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection
from starlette.responses import JSONResponse

from monkeybot_api import guild_admin_queries as sql
from monkeybot_api.auth import require_csrf
from monkeybot_api.config import Settings
from monkeybot_api.context import timestamp
from monkeybot_api.database import Database
from monkeybot_api.delivery_effects import audit, complete
from monkeybot_api.errors import ApiError, success
from monkeybot_api.guild_admin import authorize, guild_path, mutation, trim
from monkeybot_api.json_body import read_json
from monkeybot_api.notification_delivery import Identifier, one, params
from monkeybot_api.recruitment_create import acquire, canonical_start, command_key
from monkeybot_api.series_contracts import Strict
from monkeybot_api.session import resolve_session


class RotateSeason(Strict):
    name: str
    expectedCurrentSeasonId: Identifier


def stored_timestamp(value: str) -> str:
    if not value or value != value.strip():
        raise ValueError("invalid stored timestamp")
    try:
        return canonical_start(value)[1]
    except ApiError:
        raise ValueError("invalid stored timestamp") from None


Timestamp = Annotated[str, AfterValidator(stored_timestamp)]
I64 = Annotated[int, Field(ge=-(2**63), le=2**63 - 1)]


class SeasonSummary(Strict):
    # Replay deserializes the stored Rust DTO; unknown fields are ignored by serde.
    model_config = {"strict": True, "extra": "ignore"}
    id: Identifier
    name: str
    status: Literal["ACTIVE", "CLOSED"]
    startedAt: Timestamp
    endedAt: Timestamp | None = None
    createdBy: str | None = None
    participantCount: I64
    generalSeriesCount: I64
    auctionTournamentCount: I64
    auctionTenCount: I64
    auctionTwentyCount: I64
    confirmedGameCount: I64
    lastGameAt: Timestamp | None = None


class SeasonsSnapshot(Strict):
    model_config = {"strict": True, "extra": "ignore"}
    current: SeasonSummary
    completed: list[SeasonSummary]


def no_active() -> ApiError:
    return ApiError(409, "no_active_season", "길드에 진행 중인 시즌이 없습니다.")


def camel(value: str) -> str:
    head, *tail = value.split("_")
    return head + "".join(word.title() for word in tail)


async def snapshot(c: AsyncConnection, guild: str) -> dict[str, Any]:
    rows = (await c.execute(text(sql.SEASONS), params(guild))).mappings().all()
    current = None
    completed = []
    for row in rows:
        if (
            row["status"] not in {"ACTIVE", "CLOSED"}
            or (row["status"] == "ACTIVE") != (row["ended_at"] is None)
            or any(row[key] < 0 for key in row if key.endswith("_count"))
            or row["auction_ten_count"] + row["auction_twenty_count"]
            != row["auction_tournament_count"]
        ):
            raise ApiError.from_status(500)
        item = {
            camel(key): str(value)
            if key == "id"
            else (timestamp(value) if key.endswith("_at") and value is not None else value)
            for key, value in row.items()
        }
        if row["status"] == "ACTIVE":
            if current is not None:
                raise ApiError.from_status(500)
            current = item
        else:
            completed.append(item)
    if current is None:
        raise no_active()
    return {"current": current, "completed": completed}


async def rotate(c: AsyncConnection, p: dict[str, Any], body: RotateSeason) -> dict[str, Any]:
    p["now"] = datetime.now(UTC)
    p["expires"] = p["now"] + timedelta(hours=24)
    await authorize(c, p["guild"], p["actor_id"])
    try:
        replay = await acquire(c, p, False)
    except ApiError as exc:
        if exc.code == "idempotency_in_progress":
            raise ApiError(409, exc.code, "같은 요청을 처리하고 있습니다.") from None
        raise
    if replay is not None:
        status = await c.scalar(
            text("""
            SELECT status_code FROM idempotency_records WHERE guild_id=:guild AND scope_key=:guild
                AND principal_type='USER' AND principal_id=:principal AND method='POST'
                AND route_template=:route AND idempotency_key=:key
        """),
            p,
        )
        if status != 200:
            raise ApiError.from_status(500)
        try:
            return SeasonsSnapshot.model_validate(replay).model_dump()
        except ValueError:
            raise ApiError.from_status(500) from None
    await c.execute(text(sql.ROTATION_LOCK), params(p["guild"]))
    previous = await one(c, sql.ACTIVE_SEASON, p["guild"])
    if previous is None:
        raise no_active()
    if str(previous["id"]) != body.expectedCurrentSeasonId:
        raise ApiError(
            409,
            "current_season_conflict",
            "현재 시즌이 변경되었습니다. 최신 상태를 다시 확인해 주세요.",
            {
                "expectedCurrentSeasonId": body.expectedCurrentSeasonId,
                "currentSeasonId": str(previous["id"]),
            },
        )
    closed = await c.execute(
        text("""
        UPDATE seasons SET status='CLOSED', ended_at=:now
        WHERE guild_id=:guild AND id=:previous AND status='ACTIVE' AND ended_at IS NULL
    """),
        {**p, "previous": previous["id"]},
    )
    if closed.rowcount != 1:
        raise ApiError.from_status(500)
    name = trim(body.name)
    await c.execute(
        text("""
        INSERT INTO seasons
            (guild_id, id, name, status, started_at, ended_at, created_by, created_at)
        VALUES (:guild, :season, :name, 'ACTIVE', :now, NULL, :actor_id, :now)
    """),
        {**p, "name": name},
    )
    before = {
        "id": str(previous["id"]),
        "name": previous["name"],
        "status": "ACTIVE",
        "startedAt": timestamp(previous["started_at"]),
        "endedAt": None,
    }
    after = {
        "id": str(p["season"]),
        "name": name,
        "status": "ACTIVE",
        "startedAt": timestamp(p["now"]),
        "endedAt": None,
    }
    await audit(
        c,
        p,
        "SEASON_ROTATED",
        "SEASON",
        str(p["season"]),
        {"current": before},
        {
            "closed": {**before, "status": "CLOSED", "endedAt": timestamp(p["now"])},
            "current": after,
        },
    )
    result = await snapshot(c, p["guild"])
    await complete(c, p, result, 200)
    return result


def router(settings: Settings, database: Database) -> APIRouter:
    routes = APIRouter()

    @routes.api_route("/api/guilds/{guild_id}/seasons", methods=["GET", "HEAD"])
    async def seasons(request: Request, guild_id: str) -> JSONResponse:
        guild_path(guild_id)
        principal = await resolve_session(request, database, settings)
        async with database.connection(transaction=True) as c:
            await c.execute(text("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ"))
            await authorize(c, guild_id, principal["user_id"])
            result = await snapshot(c, guild_id)
        return success(request, result)

    @routes.post("/api/guilds/{guild_id}/seasons")
    async def rotate_season(request: Request, guild_id: str) -> JSONResponse:
        guild_path(guild_id)
        principal = await resolve_session(request, database, settings)
        body = await read_json(request, RotateSeason)
        require_csrf(request, settings, principal["csrf_secret_hash"])
        try:
            key = command_key(request, False)
        except ApiError as exc:
            raise ApiError(
                exc.status, exc.code, "Idempotency-Key가 올바르지 않습니다.", exc.details
            ) from None
        if not 1 <= len(trim(body.name)) <= 100:
            raise ApiError(
                422,
                "invalid_season_name",
                "시즌 이름을 확인해 주세요.",
                {"field": "name", "maxLength": 100},
            )
        p = {
            "guild": guild_id,
            "actor_id": principal["user_id"],
            "principal": principal["user_id"],
            "principal_type": "USER",
            "method": "POST",
            "route": "/api/guilds/{guildId}/seasons",
            "key": key,
            "request_id": request.state.request_id,
            "audit_id": uuid.uuid7(),
            "idem_id": uuid.uuid7(),
            "season": uuid.uuid7(),
            "hash": hashlib.sha256(
                json.dumps(
                    [guild_id, body.model_dump()], ensure_ascii=False, separators=(",", ":")
                ).encode()
            ).digest(),
        }
        return success(request, await mutation(database, lambda c: rotate(c, p, body)))

    return routes
