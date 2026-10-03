import base64
import json
import uuid
from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError


async def notification(
    c: AsyncConnection,
    p: dict[str, Any],
    kind: str,
    channel: str,
    dedup: str,
    payload: dict[str, Any],
) -> bool:
    identifier = uuid.uuid7()
    result = await c.scalar(
        text("""
        INSERT INTO bot_notification_outbox (id, guild_id, deduplication_key, discord_nonce,
            channel_id, notification_type, payload, created_at, available_at)
        VALUES (:notification, :guild, :dedup, :nonce, :channel, :kind,
            CAST(:payload AS jsonb), :now, :now)
        ON CONFLICT (guild_id, deduplication_key) DO NOTHING RETURNING id
    """),
        {
            **p,
            "notification": identifier,
            "dedup": dedup,
            "channel": channel,
            "kind": kind,
            "nonce": base64.urlsafe_b64encode(identifier.bytes).decode().rstrip("="),
            "payload": json.dumps(payload),
        },
    )
    if result is not None:
        return True
    existing = (
        (
            await c.execute(
                text(
                    "SELECT channel_id, notification_type, payload FROM bot_notification_outbox "
                    "WHERE guild_id = :guild AND deduplication_key = :dedup"
                ),
                {**p, "dedup": dedup},
            )
        )
        .mappings()
        .one_or_none()
    )
    if (
        existing is None
        or existing["channel_id"] != channel
        or existing["notification_type"] != kind
        or existing["payload"] != payload
    ):
        raise ApiError.from_status(500)
    return False


async def message_update(c: AsyncConnection, p: dict[str, Any], snapshot: dict[str, Any]) -> None:
    data = snapshot["recruitment"]
    if data["channelId"] is not None and data["messageId"] is not None:
        await notification(
            c,
            p,
            "RECRUITMENT_MESSAGE_UPDATE",
            data["channelId"],
            f"recruitment:{data['id']}:message:update:{data['revision']}",
            {"schemaVersion": 1, "recruitmentId": data["id"], "snapshot": snapshot},
        )


async def audit(
    c: AsyncConnection,
    p: dict[str, Any],
    action: str,
    target_type: str,
    target: str,
    before: dict[str, Any] | None,
    after: dict[str, Any],
) -> None:
    await c.execute(
        text("""
        INSERT INTO admin_audit_log (guild_id, id, principal_type, principal_id, action,
            target_type, target_id, request_id, before_summary, after_summary, result, created_at)
        VALUES (:guild, :audit_id, 'USER', :actor_id, :action, :target_type, :target, :request_id,
            CAST(:before AS jsonb), CAST(:after AS jsonb), 'APPLIED', :now)
    """),
        {
            **p,
            "action": action,
            "target_type": target_type,
            "target": target,
            "before": json.dumps(before) if before is not None else None,
            "after": json.dumps(after),
        },
    )


async def invalidate(
    c: AsyncConnection, p: dict[str, Any], kind: str, identifier: str, revision: int, event: str
) -> None:
    await c.execute(
        text("""
        INSERT INTO outbox_events (id, guild_id, topic, aggregate_type, aggregate_id,
            aggregate_revision, event_type, payload, occurred_at, available_at)
        VALUES (:event_id, :guild, :topic, :kind, :target, :revision, :event,
            CAST(:payload AS jsonb), :now, :now)
    """),
        {
            **p,
            "event_id": uuid.uuid7(),
            "topic": f"guild:{p['guild']}:{kind.lower().replace('_', '-')}:{identifier}",
            "kind": kind,
            "target": identifier,
            "revision": revision,
            "event": event,
            "payload": json.dumps(
                {"type": "invalidate", "resourceId": identifier, "revision": revision}
            ),
        },
    )


async def complete(
    c: AsyncConnection, p: dict[str, Any], result: dict[str, Any], status: int
) -> None:
    identifier = await c.scalar(
        text("""
        UPDATE idempotency_records SET state = 'COMPLETED', status_code = :status,
            response_body = CAST(:response AS jsonb), completed_at = :now
        WHERE id = :idem_id AND guild_id = :guild AND scope_key = :guild AND state = 'IN_PROGRESS'
        RETURNING id
    """),
        {**p, "status": status, "response": json.dumps(result)},
    )
    if identifier is None:
        raise ApiError.from_status(500)
