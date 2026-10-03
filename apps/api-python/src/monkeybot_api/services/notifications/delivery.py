"""Notification delivery workflows; callers own the transaction boundary."""

import uuid
from datetime import UTC, datetime, timedelta
from typing import Any

from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.time import timestamp
from monkeybot_api.repositories import notifications as repository
from monkeybot_api.repositories.effects import notification
from monkeybot_api.schemas.notifications import (
    Ack,
    Claim,
    Failure,
    Heartbeat,
    Nack,
    error,
    validate_failure,
    validate_heartbeat,
    validate_owner,
)


async def claim(c: AsyncConnection, body: Claim) -> dict[str, Any]:
    validate_owner(body.instanceId)
    limit = 20 if body.limit is None else body.limit
    if not 1 <= limit <= 50:
        raise error("invalid_notification_request")
    await repository.reap(c, 8)
    ids = await repository.claim_candidates(c, 8, limit)
    result = []
    for identifier in ids:
        token = uuid.uuid7()
        row = await repository.claim(
            c, body.instanceId, token, timedelta(seconds=30), identifier, 8
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
    row = await repository.lease(c, identifier, body.instanceId, token)
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
    row = await repository.ack(c, body.discordMessageId, identifier, body.instanceId, token)
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
    binding = await repository.bind(
        c,
        auction,
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
        current = await repository.payload(c, auction, row["guild_id"], recruitment)
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
    row = await repository.nack_lease(c, identifier, body.instanceId, token)
    if row is None:
        raise error("notification_lease_conflict")
    code, delay = disposition(identifier, row["attempts"], body.failure)
    if code:
        changed = await repository.fail(
            c, code, body.failure.message, identifier, body.instanceId, token
        )
        if not changed:
            raise error("notification_lease_conflict")
        return {"state": "FAILED", "failureCode": code, "attempts": row["attempts"]}
    available = await repository.retry(
        c, delay, body.failure.message, identifier, body.instanceId, token
    )
    if available is None:
        raise error("notification_lease_conflict")
    return {
        "state": "RETRY_SCHEDULED",
        "availableAt": timestamp(available),
        "attempts": row["attempts"],
    }


async def heartbeat(c: AsyncConnection, body: Heartbeat) -> dict[str, Any]:
    validate_heartbeat(body)
    value = await repository.heartbeat(c, body, datetime.now(UTC))
    return {"accepted": True, "lastHeartbeatAt": timestamp(value)}
