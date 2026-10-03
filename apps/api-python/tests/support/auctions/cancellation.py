"""Reusable fixtures and request builders; no test module imports."""

from sqlalchemy import text

from tests.support.auctions.bidding import PATH, prepared
from tests.support.auctions.bidding import request as draft
from tests.support.guilds.context import ROLE_A, USER_ID
from tests.support.recruitments.creation import BOT

ACTOR = {
    "id": USER_ID,
    "globalDisplayName": "취소 운영자",
    "displayName": "운영자",
    "roleIds": [ROLE_A],
}


BOT_HEADERS = {"authorization": f"Bearer {BOT}", "idempotency-key": "997001"}


async def tournament(owner, client):
    headers, source = await prepared(owner, client)
    result = await draft(client, headers, None, source)
    return headers, result["tournament"]


async def cancel(client, headers, record, *, key="cancel", reason="운영 사유", bot=False):
    return await client.post(
        f"{PATH}/{record['id']}/cancel",
        headers={**headers, "idempotency-key": key},
        json={"expectedRevision": record["revision"], "reason": reason}
        | ({"actor": ACTOR} if bot else {}),
    )


async def effects(owner):
    async with owner.connect() as c:
        return tuple(
            [
                await c.scalar(text(f"SELECT count(*) FROM {table}"))
                for table in (
                    "idempotency_records",
                    "admin_audit_log",
                    "outbox_events",
                    "bot_notification_outbox",
                    "mmr_ledger_entries",
                )
            ]
        )
