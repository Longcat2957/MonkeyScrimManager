import hashlib
import json
import uuid
from datetime import UTC, datetime, timedelta
from typing import Any

from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.text import trim
from monkeybot_api.core.time import timestamp
from monkeybot_api.db.database import Database
from monkeybot_api.db.transactions import mutation, repeatable_read
from monkeybot_api.repositories.effects import audit, complete
from monkeybot_api.repositories.guilds import seasons as repository
from monkeybot_api.repositories.idempotency import acquire
from monkeybot_api.repositories.permissions import authorize
from monkeybot_api.schemas.guilds.seasons import RotateSeason, SeasonsSnapshot


def no_active() -> ApiError:
    return ApiError(409, "no_active_season", "길드에 진행 중인 시즌이 없습니다.")


def camel(value: str) -> str:
    head, *tail = value.split("_")
    return head + "".join(word.title() for word in tail)


async def snapshot(c: AsyncConnection, guild: str) -> dict[str, Any]:
    rows = await repository.list_seasons(c, guild)
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
        status = await repository.replay_status(c, p)
        if status != 200:
            raise ApiError.from_status(500)
        try:
            return SeasonsSnapshot.model_validate(replay).model_dump()
        except ValueError:
            raise ApiError.from_status(500) from None
    previous = await repository.lock_active(c, p["guild"])
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
    changed = await repository.close_season(c, p, previous["id"])
    if changed != 1:
        raise ApiError.from_status(500)
    name = trim(body.name)
    await repository.create_season(c, p, name)
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


async def list_seasons(database: Database, guild: str, actor: str) -> dict[str, Any]:
    async with database.connection(transaction=True) as c:
        await repeatable_read(c)
        await authorize(c, guild, actor)
        return await snapshot(c, guild)


async def rotate_season(
    database: Database, guild: str, actor: str, request_id: str, key: str, body: RotateSeason
) -> dict[str, Any]:
    if not 1 <= len(trim(body.name)) <= 100:
        raise ApiError(
            422,
            "invalid_season_name",
            "시즌 이름을 확인해 주세요.",
            {"field": "name", "maxLength": 100},
        )
    p = {
        "guild": guild,
        "actor_id": actor,
        "principal": actor,
        "principal_type": "USER",
        "method": "POST",
        "route": "/api/guilds/{guildId}/seasons",
        "key": key,
        "request_id": request_id,
        "audit_id": uuid.uuid7(),
        "idem_id": uuid.uuid7(),
        "season": uuid.uuid7(),
        "hash": hashlib.sha256(
            json.dumps(
                [guild, body.model_dump()], ensure_ascii=False, separators=(",", ":")
            ).encode()
        ).digest(),
    }
    return await mutation(database, lambda c: rotate(c, p, body))
