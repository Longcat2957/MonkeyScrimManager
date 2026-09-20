import logging
import re
import uuid
from datetime import UTC, datetime, timedelta
from typing import Annotated, Any, Literal

from fastapi import APIRouter, Request
from pydantic import AfterValidator, Field
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection
from starlette.responses import JSONResponse

from monkeybot_api import notification_queries as sql
from monkeybot_api.config import Settings, is_snowflake
from monkeybot_api.context import timestamp
from monkeybot_api.database import Database
from monkeybot_api.delivery_effects import notification
from monkeybot_api.errors import ApiError, success
from monkeybot_api.json_body import read_json
from monkeybot_api.recruitment_create import require_bot
from monkeybot_api.series_contracts import Strict

logger = logging.getLogger(__name__)


def valid_uuid(value: str) -> str:
    canonical = r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}"
    if (
        re.fullmatch(
            rf"(?:[0-9a-fA-F]{{32}}|{canonical}|\{{{canonical}\}}|urn:uuid:{canonical})", value
        )
        is None
    ):
        raise ValueError("invalid UUID format")
    return str(uuid.UUID(value))


def valid_message(value: str) -> str:
    if not is_snowflake(value):
        raise ValueError("invalid message ID")
    return value


Identifier = Annotated[str, AfterValidator(valid_uuid)]


class Claim(Strict):
    instanceId: str
    limit: int | None = Field(default=None, ge=0, le=2**32 - 1)


class Lease(Strict):
    instanceId: str
    notificationId: Identifier
    leaseToken: Identifier


class Ack(Lease):
    discordMessageId: Annotated[str, AfterValidator(valid_message)]


class Failure(Strict):
    kind: Literal["HTTP", "TRANSPORT", "DELIVERY_UNKNOWN"]
    status: int | None = Field(default=None, ge=0, le=65535)
    retryAfterMs: int | None = Field(default=None, ge=0, le=2**64 - 1)
    message: str


class Nack(Lease):
    failure: Failure


class Heartbeat(Strict):
    instanceId: str
    version: str
    commitSha: str | None = None


ERRORS = {
    "invalid_notification_request": (400, "Bot 알림 전송 요청 값이 올바르지 않습니다."),
    "notification_lease_conflict": (
        409,
        "알림 전송 lease가 만료되었거나 다른 Bot 인스턴스로 이전되었습니다.",
    ),
    "notification_type_mismatch": (422, "이 알림은 모집 메시지 확인으로 처리할 수 없습니다."),
    "recruitment_message_create_requires_typed_ack": (
        422,
        "모집 메시지 생성 알림은 전용 확인 경로로 처리해야 합니다.",
    ),
    "recruitment_message_bind_conflict": (
        409,
        "모집 메시지를 현재 모집 상태에 연결할 수 없습니다.",
    ),
    "notification_outbox_invariant_failed": (500, "저장된 알림 전송 상태를 처리할 수 없습니다."),
    "invalid_heartbeat": (422, "Bot heartbeat 요청 값이 올바르지 않습니다."),
}


def error(code: str) -> ApiError:
    status, message = ERRORS[code]
    if status == 500:
        logger.error("notification_outbox_invariant_failed", stack_info=True)
    return ApiError(status, code, message)


def owner_valid(value: str) -> bool:
    return re.fullmatch(r"[A-Za-z0-9._:-]{1,128}", value) is not None


def validate_owner(value: str) -> None:
    if not owner_valid(value):
        raise error("invalid_notification_request")


def params(*values: Any) -> dict[str, Any]:
    return {f"p{i}": value for i, value in enumerate(values, 1)}


async def one(c: AsyncConnection, query: str, *values: Any) -> dict[str, Any] | None:
    row = (await c.execute(text(query), params(*values))).mappings().one_or_none()
    return dict(row) if row is not None else None


async def claim(c: AsyncConnection, body: Claim) -> dict[str, Any]:
    validate_owner(body.instanceId)
    limit = 20 if body.limit is None else body.limit
    if not 1 <= limit <= 50:
        raise error("invalid_notification_request")
    await c.execute(text(sql.REAP), params(8))
    ids = (
        (
            await c.execute(
                text(sql.CLAIM_CANDIDATES),
                params(
                    8,
                    limit,
                    "RECRUITMENT_MESSAGE_CREATE",
                    "RECRUITMENT_MESSAGE_UPDATE",
                    "RECRUITMENT_MESSAGE_DELETE",
                    "AUCTION_RECRUITMENT_MESSAGE_CREATE",
                    "AUCTION_RECRUITMENT_MESSAGE_UPDATE",
                    "AUCTION_RECRUITMENT_MESSAGE_DELETE",
                ),
            )
        )
        .scalars()
        .all()
    )
    result = []
    for identifier in ids:
        token = uuid.uuid7()
        row = await one(
            c, sql.CLAIM_UPDATE, body.instanceId, token, timedelta(seconds=30), identifier, 8
        )
        if row is None:
            raise error("notification_outbox_invariant_failed")
        result.append(
            {
                "id": str(row["id"]),
                "guildId": row["guild_id"],
                "leaseToken": str(token),
                "leaseUntil": timestamp(row["lease_until"]),
                "channelId": row["channel_id"],
                "type": row["notification_type"],
                "payload": row["payload"],
                "discordNonce": row["discord_nonce"],
                "attempts": row["attempts"],
            }
        )
    return {"notifications": result}


async def ack(c: AsyncConnection, body: Ack, mode: str | None) -> dict[str, Any]:
    validate_owner(body.instanceId)
    identifier, token = uuid.UUID(body.notificationId), uuid.UUID(body.leaseToken)
    row = await one(c, sql.LEASE, identifier, body.instanceId, token)
    if row is None:
        raise error("notification_lease_conflict")
    if mode is None:
        if row["notification_type"] in (
            "RECRUITMENT_MESSAGE_CREATE",
            "AUCTION_RECRUITMENT_MESSAGE_CREATE",
        ):
            raise error("recruitment_message_create_requires_typed_ack")
    elif row["notification_type"] != mode + "_MESSAGE_CREATE":
        raise error("notification_type_mismatch")
    row = await one(c, sql.ACK, body.discordMessageId, identifier, body.instanceId, token)
    if row is None:
        raise error("notification_lease_conflict")
    result: dict[str, Any] = {"acknowledged": True, "deliveredAt": timestamp(row["delivered_at"])}
    if mode is None:
        return result
    auction = mode == "AUCTION_RECRUITMENT"
    key = "auctionRecruitmentId" if auction else "recruitmentId"
    payload = row["payload"]
    try:
        if not isinstance(payload, dict) or set(payload) != {"schemaVersion", key, "snapshot"}:
            raise ValueError("invalid payload fields")
        version = payload["schemaVersion"]
        if type(version) is not int or version not in ((1, 2) if auction else (1,)):
            raise ValueError("invalid schema version")
        if not isinstance(payload[key], str) or not isinstance(payload["snapshot"], dict):
            raise ValueError("invalid snapshot")
        recruitment = uuid.UUID(payload[key])
    except ValueError, TypeError, KeyError:
        raise error("notification_outbox_invariant_failed") from None
    binding = await one(
        c,
        sql.BIND_AUCTION if auction else sql.BIND_REGULAR,
        row["channel_id"],
        body.discordMessageId,
        row["delivered_at"],
        row["guild_id"],
        recruitment,
    )
    if binding is None:
        raise error("recruitment_message_bind_conflict")
    snapshot_recruitment = payload["snapshot"].get("recruitment")
    sent_revision = (
        snapshot_recruitment.get("revision") if isinstance(snapshot_recruitment, dict) else None
    )
    if type(sent_revision) is not int or not -(2**63) <= sent_revision < 2**63:
        raise error("notification_outbox_invariant_failed")
    kind = (
        "DELETE"
        if binding["status"] == "CANCELLED"
        else ("UPDATE" if binding["revision"] > min(sent_revision + 1, 2**63 - 1) else None)
    )
    if kind:
        current = await c.scalar(
            text(sql.PAYLOAD_AUCTION if auction else sql.PAYLOAD_REGULAR),
            params(row["guild_id"], recruitment, 2 if auction else 1),
        )
        if current is None:
            raise error("recruitment_message_bind_conflict")
        prefix = "auction-recruitment" if auction else "recruitment"
        try:
            await notification(
                c,
                {"guild": row["guild_id"], "now": row["delivered_at"]},
                mode + "_MESSAGE_" + kind,
                row["channel_id"],
                f"{prefix}:{recruitment}:message:{kind.lower()}:{binding['revision']}",
                current,
            )
        except ApiError:
            raise error("notification_outbox_invariant_failed") from None
    result["auctionRecruitment" if auction else "recruitment"] = {
        "id": str(recruitment),
        "revision": binding["revision"],
    }
    return result


def validate_failure(failure: Failure) -> None:
    invalid = not failure.message.strip() or len(failure.message.encode()) > 1024
    if failure.kind == "HTTP":
        invalid |= failure.status is None or not 400 <= failure.status <= 599
        if failure.retryAfterMs is not None:
            invalid |= failure.status != 429 or not 1 <= failure.retryAfterMs <= 3_600_000
    else:
        invalid |= failure.status is not None or failure.retryAfterMs is not None
    if invalid:
        raise error("invalid_notification_request")


def disposition(
    identifier: uuid.UUID, attempts: int, failure: Failure
) -> tuple[str | None, timedelta]:
    if failure.kind == "DELIVERY_UNKNOWN":
        return "DELIVERY_UNKNOWN", timedelta(0)
    if (
        failure.kind == "HTTP"
        and failure.status != 429
        and failure.status is not None
        and failure.status < 500
    ):
        return "DISCORD_HTTP_4XX", timedelta(0)
    if attempts >= 8:
        return "MAX_ATTEMPTS", timedelta(0)
    delay = failure.retryAfterMs
    if delay is None:
        seed = int.from_bytes(identifier.bytes[:8], "big") ^ max(attempts, 0)
        delay = min(300_000, (1 << min(max(attempts - 1, 0), 8)) * 1000 + seed % 1000)
    return None, timedelta(milliseconds=delay)


async def nack(c: AsyncConnection, body: Nack) -> dict[str, Any]:
    validate_owner(body.instanceId)
    validate_failure(body.failure)
    identifier, token = uuid.UUID(body.notificationId), uuid.UUID(body.leaseToken)
    row = await one(c, sql.NACK_LEASE, identifier, body.instanceId, token)
    if row is None:
        raise error("notification_lease_conflict")
    code, delay = disposition(identifier, row["attempts"], body.failure)
    if code:
        result = await c.execute(
            text(sql.NACK_FAIL),
            params(code, body.failure.message, identifier, body.instanceId, token),
        )
        if result.rowcount != 1:
            raise error("notification_lease_conflict")
        return {"state": "FAILED", "failureCode": code, "attempts": row["attempts"]}
    available = await c.scalar(
        text(sql.NACK_RETRY),
        params(delay, body.failure.message, identifier, body.instanceId, token),
    )
    if available is None:
        raise error("notification_lease_conflict")
    return {
        "state": "RETRY_SCHEDULED",
        "availableAt": timestamp(available),
        "attempts": row["attempts"],
    }


async def heartbeat(c: AsyncConnection, body: Heartbeat) -> dict[str, Any]:
    if (
        not owner_valid(body.instanceId)
        or not body.instanceId[0].isascii()
        or not body.instanceId[0].isalnum()
        or re.fullmatch(r"[\x21-\x7e]{1,100}", body.version) is None
        or (
            body.commitSha is not None
            and re.fullmatch(r"[A-Za-z0-9._-]{1,128}", body.commitSha) is None
        )
    ):
        raise error("invalid_heartbeat")
    now = datetime.now(UTC)
    value = await c.scalar(
        text("""
        INSERT INTO bot_instances
            (instance_id, version, commit_sha, started_at, last_heartbeat_at, updated_at)
        VALUES (:id, :version, :commit, :now, :now, :now)
        ON CONFLICT (instance_id) DO UPDATE SET version = EXCLUDED.version,
            commit_sha = EXCLUDED.commit_sha, last_heartbeat_at = EXCLUDED.last_heartbeat_at,
            updated_at = EXCLUDED.updated_at RETURNING last_heartbeat_at
    """),
        {"id": body.instanceId, "version": body.version, "commit": body.commitSha, "now": now},
    )
    return {"accepted": True, "lastHeartbeatAt": timestamp(value)}


def router(settings: Settings, database: Database) -> APIRouter:
    routes = APIRouter()

    @routes.post("/internal/heartbeat")
    async def record_heartbeat(request: Request) -> JSONResponse:
        require_bot(request, settings)
        body = await read_json(request, Heartbeat)
        async with database.connection(transaction=True) as c:
            result = await heartbeat(c, body)
        return success(request, result)

    @routes.post("/internal/notifications/claim")
    async def claim_notifications(request: Request) -> JSONResponse:
        require_bot(request, settings)
        body = await read_json(request, Claim)
        async with database.connection(transaction=True) as c:
            result = await claim(c, body)
        return success(request, result)

    @routes.post("/internal/notifications/nack")
    async def nack_notification(request: Request) -> JSONResponse:
        require_bot(request, settings)
        body = await read_json(request, Nack)
        async with database.connection(transaction=True) as c:
            result = await nack(c, body)
        return success(request, result)

    def ack_route(path: str, mode: str | None) -> None:
        async def acknowledge(request: Request) -> JSONResponse:
            require_bot(request, settings)
            body = await read_json(request, Ack)
            async with database.connection(transaction=True) as c:
                result = await ack(c, body, mode)
            return success(request, result)

        routes.add_api_route(path, acknowledge, methods=["POST"])

    ack_route("/internal/notifications/ack", None)
    ack_route("/internal/notifications/recruitment-message/ack", "RECRUITMENT")
    ack_route("/internal/notifications/auction-recruitment-message/ack", "AUCTION_RECRUITMENT")
    return routes
