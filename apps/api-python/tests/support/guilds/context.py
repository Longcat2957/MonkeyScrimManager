"""Reusable fixtures and request builders; no test module imports."""

import uuid

import httpx
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncEngine

from tests.support.auth.oauth import cookies, finish_login, start_login

USER_ID = "328516947210438656"


GUILD_A = "954301284660912138"


GUILD_B = "954301284660912139"


ROLE_A = "300000000000000001"


ROLE_B = "300000000000000002"


async def guild_fixture(owner: AsyncEngine, guild_id: str, name: str, role: str) -> uuid.UUID:
    season_id = uuid.uuid7()
    async with owner.begin() as connection:
        await connection.execute(
            text("""
            INSERT INTO guilds (guild_id, discord_name, status, installed_at, updated_at)
            VALUES (:guild, :name, 'ACTIVE', now(), now())
        """),
            {"guild": guild_id, "name": name},
        )
        await connection.execute(
            text("""
            INSERT INTO guild_members (guild_id, user_id, display_name, status, first_seen_at)
            VALUES (:guild, :user, '테스트 멤버', 'ACTIVE', now())
        """),
            {"guild": guild_id, "user": USER_ID},
        )
        await connection.execute(
            text("""
            INSERT INTO guild_config
                (guild_id, display_name, operator_role_id,
                 is_externally_public, revision, updated_at)
            VALUES (:guild, '', :role, true, 0, now())
        """),
            {"guild": guild_id, "role": role},
        )
        await connection.execute(
            text("""
            INSERT INTO seasons (guild_id, id, name, status, started_at, created_at)
            VALUES (:guild, :id, '현재 시즌', 'ACTIVE', '2026-09-13T01:02:03.12Z', now())
        """),
            {"guild": guild_id, "id": season_id},
        )
    return season_id


async def authenticated_setup(owner: AsyncEngine, client: httpx.AsyncClient) -> dict[str, str]:
    state, _ = await start_login(client)
    response = await finish_login(client, state)
    assert response.status_code == 303
    await guild_fixture(owner, GUILD_A, "Alpha", ROLE_A)
    return cookies(response)
