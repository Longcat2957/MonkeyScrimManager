"""Reusable fixtures and request builders; no test module imports."""

import httpx
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncEngine

from tests.support.guilds.context import GUILD_A, ROLE_A, USER_ID
from tests.support.recruitments.creation import BODY, PATH, setup


async def setup_members(
    owner: AsyncEngine, client: httpx.AsyncClient
) -> tuple[str, dict[str, str]]:
    headers = await setup(owner, client)
    created = await client.post(PATH, json=BODY, headers=headers)
    assert created.status_code == 201
    record_id = created.json()["data"]["recruitment"]["id"]
    async with owner.begin() as connection:
        await connection.execute(
            text(
                "INSERT INTO guild_member_discord_roles "
                "(guild_id, user_id, role_id, last_synced_at) VALUES (:guild, :user, :role, now())"
            ),
            {"guild": GUILD_A, "user": USER_ID, "role": ROLE_A},
        )
        await connection.execute(
            text("UPDATE recruitments SET channel_id = '123', message_id = '456'")
        )
    return f"{PATH}/{record_id}/members", {**headers, "idempotency-key": "add-member-1"}
