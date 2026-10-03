"""Reusable fixtures and request builders; no test module imports."""

import asyncio
import uuid

from sqlalchemy import text

from tests.support.auth.crypto import rust_auth
from tests.support.recruitments.creation import BOT
from tests.support.recruitments.lifecycle import fixture

BOT_HEADERS = {"authorization": f"Bearer {BOT}"}


async def post(client, action, body):
    return await client.post("/internal/notifications/" + action, json=body, headers=BOT_HEADERS)


async def oracle(action, body):
    return await asyncio.to_thread(
        rust_auth, {"operation": "notification", "action": action, "body": body}
    )


async def claim(client, instance="bot:1", limit=20):
    response = await post(client, "claim", {"instanceId": instance, "limit": limit})
    assert response.status_code == 200, response.text
    return response.json()["data"]["notifications"]


def lease(row, instance="bot:1", message=True):
    result = {"instanceId": instance, "notificationId": row["id"], "leaseToken": row["leaseToken"]}
    if message:
        result["discordMessageId"] = "987"
    return result


async def seed(owner, client, *, generic=False, auction=False, version=2):
    path, headers = await fixture(owner, client, full=False)
    identifier = uuid.UUID(path.rsplit("/", 1)[1])
    async with owner.begin() as c:
        await c.execute(text("UPDATE recruitments SET message_id = NULL, channel_id = NULL"))
        if generic:
            await c.execute(
                text(
                    "UPDATE bot_notification_outbox SET notification_type = 'SERIES_RESULT_CREATE'"
                )
            )
        if auction:
            await c.execute(
                text("""
                INSERT INTO auction_recruitments (guild_id, id, season_id, display_number, title,
                    format, target_count, status, revision, created_by, created_at, updated_at)
                SELECT guild_id, id, season_id, display_number, title, 'TEN_PERSON', 10, status,
                    revision, created_by, created_at, updated_at FROM recruitments
            """)
            )
            await c.execute(
                text("""
                UPDATE bot_notification_outbox
                SET notification_type = 'AUCTION_RECRUITMENT_MESSAGE_CREATE',
                    payload = jsonb_build_object('schemaVersion', CAST(:version AS integer),
                        'auctionRecruitmentId', CAST(:id AS text),
                        'snapshot', jsonb_build_object(
                            'recruitment', jsonb_build_object('revision', 0)))
            """),
                {"id": str(identifier), "version": version},
            )
    return path, headers
