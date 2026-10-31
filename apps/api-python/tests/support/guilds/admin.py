"""Reusable fixtures and request builders; no test module imports."""

import asyncio

from sqlalchemy import text

from tests.support.auth.crypto import rust_auth
from tests.support.guilds.context import GUILD_A, ROLE_A, USER_ID
from tests.support.recruitments.creation import BOT
from tests.support.recruitments.lifecycle import fixture

SETTINGS = f"/api/guilds/{GUILD_A}/settings"


SEASONS = f"/api/guilds/{GUILD_A}/seasons"


CATALOG = f"/internal/guilds/{GUILD_A}/discord-catalog"


CATALOG_BODY = {
    "roles": [
        {"id": ROLE_A, "name": " Operators ", "position": 1, "managed": False},
        {"id": "9001", "name": " Zeta ", "position": 3, "managed": False},
        {"id": "9002", "name": "Managed", "position": 10, "managed": True},
    ],
    "channels": [
        {"id": "8001", "name": " recruitment ", "position": 2, "kind": "TEXT"},
        {
            "id": "8002",
            "name": " results ",
            "parentName": " Group ",
            "position": 1,
            "kind": "ANNOUNCEMENT",
        },
    ],
}


async def oracle(action, body=None, key=None):
    return await asyncio.to_thread(
        rust_auth,
        {
            "operation": "guild_admin",
            "guild_id": GUILD_A,
            "user_id": USER_ID,
            "action": action,
            "body": body,
            "key": key,
        },
    )


async def setup(owner, client):
    _, headers = await fixture(owner, client, full=False)
    async with owner.begin() as c:
        await c.execute(
            text("UPDATE guild_config SET display_name='Admin guild' WHERE guild_id=:guild"),
            {"guild": GUILD_A},
        )
    response = await client.get(SETTINGS, headers=headers)
    assert response.status_code == 200, response.text
    return headers, response.json()["data"]


async def catalog(client, body=None):
    return await client.put(
        CATALOG,
        json=CATALOG_BODY if body is None else body,
        headers={"authorization": f"Bearer {BOT}"},
    )


def settings_body(snapshot, **changes):
    return {**snapshot["settings"], "expectedRevision": snapshot["revision"], **changes}


async def rotate(client, headers, current, *, name="새 시즌", key="season-1"):
    return await client.post(
        SEASONS,
        json={"name": name, "expectedCurrentSeasonId": current},
        headers={**headers, "idempotency-key": key},
    )
