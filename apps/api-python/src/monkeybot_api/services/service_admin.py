"""Service-admin status and guarded test-record reset workflow."""

import logging
import re
import time
import uuid
from datetime import UTC, datetime, timedelta
from typing import Any

from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.config import Settings, is_snowflake
from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.time import timestamp
from monkeybot_api.db.database import Database
from monkeybot_api.db.transactions import mutation, repeatable_read
from monkeybot_api.repositories import service_admin as repository

logger = logging.getLogger(__name__)


def authorize(principal: dict[str, Any], settings: Settings, *, reset: bool = False) -> None:
    if principal["discord_id"] not in settings.admin_ids:
        raise ApiError(403, "service_admin_required", "서비스 관리자 권한이 필요합니다.")
    if reset and not settings.dummy_users_enabled:
        raise ApiError(
            403, "test_record_reset_disabled", "테스트 기록 초기화 기능이 비활성화되어 있습니다."
        )


def validate_guild(guild: str) -> None:
    if not is_snowflake(guild):
        raise ApiError(400, "invalid_guild_id", "Discord 서버 ID 형식이 올바르지 않습니다.")


def validate_key(values: list[str]) -> str:
    key = values[0] if len(values) == 1 else ""
    # Rust's Uuid::parse_str accepts both canonical and simple 32-hex forms.
    canonical = r"[0-9a-fA-F]{8}(?:-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}"
    pattern = rf"(?:[0-9a-fA-F]{{32}}|{canonical}|\{{{canonical}\}}|urn:uuid:{canonical})"
    if not re.fullmatch(pattern, key):
        raise ApiError(400, "invalid_idempotency_key", "Idempotency-Key가 올바르지 않습니다.")
    try:
        uuid.UUID(key.removeprefix("urn:uuid:"))
    except ValueError:
        raise ApiError(
            400, "invalid_idempotency_key", "Idempotency-Key가 올바르지 않습니다."
        ) from None
    return key


def _item(
    key: str,
    status: str,
    value: str,
    *,
    latency: int | None = None,
    healthy_at: datetime | None = None,
    issue: str | None = None,
) -> dict[str, Any]:
    return {
        "key": key,
        "status": status,
        "latencyMs": latency,
        "value": value,
        "lastHealthyAt": timestamp(healthy_at) if healthy_at is not None else None,
        "issue": issue,
    }


def _bot_age(last: datetime | None, checked: datetime) -> tuple[str, str, str | None]:
    if last is None:
        return (
            "down",
            "heartbeat 없음",
            "마지막 heartbeat가 5분을 초과했거나 아직 수신되지 않았습니다.",
        )
    age = max(int((checked - last).total_seconds()), 0)
    if age < 60:
        value = f"{age}초 전"
    elif age % 60 == 0:
        value = f"{age // 60}분 전"
    else:
        value = f"{age // 60}분 {age % 60}초 전"
    if checked - last <= timedelta(seconds=90):
        return "normal", value, None
    if checked - last <= timedelta(minutes=5):
        return "delayed", value, "마지막 heartbeat가 정상 기준 90초를 초과했습니다."
    return "down", value, "마지막 heartbeat가 5분을 초과했거나 아직 수신되지 않았습니다."


async def service_status(database: Database) -> dict[str, Any]:
    started = time.monotonic()
    report = await database.readiness()
    latency = min(int((time.monotonic() - started) * 1000), 2**64 - 1)
    checked = datetime.now(UTC)
    async with database.connection() as c:
        last_heartbeat = await repository.latest_heartbeat(c)
    dependencies = report["dependencies"]
    services = []
    if report["acceptingRequests"]:
        services.append(_item("API", "normal", "요청 수신 가능", healthy_at=checked))
    else:
        services.append(
            _item(
                "API", "down", "요청 수신 중지", issue="API가 종료 중이거나 요청을 받지 않습니다."
            )
        )
    postgres = dependencies["postgres"]
    migration = dependencies["migration"]
    if postgres == "available" and migration == "available":
        services.append(
            _item("POSTGRESQL", "normal", "연결 정상", latency=latency, healthy_at=checked)
        )
    elif postgres == "available":
        services.append(
            _item(
                "POSTGRESQL",
                "delayed",
                "migration 확인 필요",
                latency=latency,
                issue="PostgreSQL migration 상태를 확인할 수 없습니다.",
            )
        )
    elif postgres == "not_configured":
        services.append(
            _item("POSTGRESQL", "down", "미구성", issue="PostgreSQL이 구성되지 않았습니다.")
        )
    else:
        services.append(
            _item("POSTGRESQL", "down", "연결 불가", issue="PostgreSQL 연결을 확인할 수 없습니다.")
        )
    bot_status, bot_value, bot_issue = _bot_age(last_heartbeat, checked)
    services.append(
        _item(
            "BOT_HEARTBEAT",
            bot_status,
            bot_value,
            healthy_at=last_heartbeat,
            issue=bot_issue,
        )
    )
    valkey = dependencies["valkey"]
    if valkey == "available":
        services.append(_item("VALKEY", "normal", "연결 정상", latency=latency, healthy_at=checked))
    elif valkey == "not_configured":
        services.append(_item("VALKEY", "delayed", "미구성", issue="Valkey가 구성되지 않았습니다."))
    else:
        services.append(
            _item("VALKEY", "down", "연결 불가", issue="Valkey 연결을 확인할 수 없습니다.")
        )
    return {"checkedAt": timestamp(checked), "services": services}


async def reset_preview(database: Database, guild: str) -> dict[str, Any]:
    async with database.connection(transaction=True) as c:
        await repeatable_read(c)
        preview = await repository.preview(c, guild, lock=False)
    preview.pop("creator")
    return preview


async def reset_records(
    database: Database, guild: str, actor: str, key: str, request_id: str
) -> dict[str, Any]:
    async def operation(c: AsyncConnection) -> dict[str, Any]:
        now = datetime.now(UTC)
        record, replay = await repository.acquire_reset(c, guild, actor, key, now)
        if replay is not None:
            return replay
        assert record is not None
        preview = await repository.preview(c, guild, lock=True)
        deleted, season_id = await repository.purge_and_reseed(
            c, guild, preview["activeSeasonName"], preview["creator"], now
        )
        counts = preview["counts"]
        counts["totalRows"] = deleted
        result = {
            "guildId": guild,
            "displayName": preview["displayName"],
            "resetAt": timestamp(now),
            "deleted": counts,
            "freshSeason": {"id": str(season_id), "name": preview["activeSeasonName"]},
        }
        await repository.complete_reset(c, record, result, now)
        return result

    result = await mutation(database, operation)
    logger.warning(
        "service_admin_reset_all_test_guild_records",
        extra={
            "guild_id": guild,
            "actor_id": actor,
            "deleted_rows": result["deleted"]["totalRows"],
            "request_id": request_id,
        },
    )
    return result
