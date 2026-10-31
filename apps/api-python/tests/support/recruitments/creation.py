"""Reusable fixtures and request builders; no test module imports."""

import httpx
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncEngine

from tests.support.guilds.context import GUILD_A, authenticated_setup
from tests.support.users.profile import csrf

PATH = f"/api/guilds/{GUILD_A}/recruitments"


START = "2026-09-13T20:00:00.123456789+09:00"


BODY = {"bestOf": 3, "hardFearless": True, "expectedStartAt": START}


BOT = "isolated-bot-service-token-32-characters"


async def setup(owner: AsyncEngine, client: httpx.AsyncClient) -> dict[str, str]:
    material = await authenticated_setup(owner, client)
    async with owner.begin() as connection:
        await connection.execute(
            text("UPDATE guild_config SET recruitment_channel_id = '123456789'")
        )
        await connection.execute(
            text(
                "INSERT INTO guild_counters (guild_id, counter_key, next_value) "
                "VALUES (:guild, 'RECRUITMENT', 1)"
            ),
            {"guild": GUILD_A},
        )
    return {**csrf(material), "idempotency-key": "browser-create-1"}


async def counts(owner: AsyncEngine) -> tuple[int, ...]:
    async with owner.connect() as connection:
        return tuple(
            [
                await connection.scalar(text(f"SELECT count(*) FROM {table}"))
                for table in (
                    "recruitments",
                    "recruitment_entry_drafts",
                    "admin_audit_log",
                    "outbox_events",
                    "bot_notification_outbox",
                    "idempotency_records",
                )
            ]
        )
