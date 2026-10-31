"""Reusable fixtures and request builders; no test module imports."""

import asyncio
import uuid

import httpx
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncEngine

from tests.support.auctions.recruitments import call, filled, teams
from tests.support.auth.crypto import rust_auth
from tests.support.guilds.context import GUILD_A, USER_ID

PATH = f"/api/guilds/{GUILD_A}/auction-tournaments"


async def prepared(owner: AsyncEngine, client: httpx.AsyncClient, count: int = 10):
    headers, source = await filled(owner, client, count=count)
    await call(client, headers, "close", source)
    saved = await call(
        client, headers, "setup", source, {"expectedRevision": count, "teams": teams(count // 5)}
    )
    async with owner.begin() as c:
        await c.execute(
            text("INSERT INTO guild_counters VALUES (:g,'AUCTION_TOURNAMENT',1)"), {"g": GUILD_A}
        )
        await c.execute(
            text("UPDATE auction_recruitments SET channel_id='123456789',message_id='987654321'")
        )
    return headers, {"sourceRecruitmentId": source, "expectedRevision": saved["draft"]["revision"]}


async def rust(action, identifier, body, key):
    return await asyncio.to_thread(
        rust_auth,
        {
            "operation": "auction_tournament",
            "guild_id": GUILD_A,
            "user_id": USER_ID,
            "tournament_id": identifier,
            "action": action,
            "body": body,
            "key": key,
        },
    )


async def request(client, headers, identifier, body, key=None):
    result = await client.post(
        PATH + (f"/{identifier}/commands" if identifier else ""),
        json=body,
        headers={**headers, "idempotency-key": key or str(uuid.uuid7())},
    )
    assert result.status_code == (200 if identifier else 201), result.text
    return result.json()["data"]
