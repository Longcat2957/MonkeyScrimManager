"""Reusable fixtures and request builders; no test module imports."""

import asyncio
import uuid

import httpx
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncEngine

from tests.support.auth.crypto import rust_auth
from tests.support.guilds.context import GUILD_A, USER_ID
from tests.support.recruitments.members import setup_members


async def fixture(
    owner: AsyncEngine, client: httpx.AsyncClient, *, full: bool = True
) -> tuple[str, dict[str, str]]:
    member_path, headers = await setup_members(owner, client)
    path = member_path.removesuffix("/members")
    if full:
        record_id = uuid.UUID(path.split("/")[-1])
        async with owner.begin() as connection:
            await connection.execute(
                text("""
                INSERT INTO users (user_id, identity_kind, display_name, created_at, updated_at)
                SELECT (1000+n)::text, 'DISCORD', '참가자', now(), now()
                FROM generate_series(1,10) n
            """)
            )
            await connection.execute(
                text("""
                INSERT INTO guild_members (guild_id, user_id, display_name, status, first_seen_at)
                SELECT :guild, (1000+n)::text, '참가자', 'ACTIVE', now()
                FROM generate_series(1,10) n
            """),
                {"guild": GUILD_A},
            )
            await connection.execute(
                text("""
                INSERT INTO recruitment_participants
                    (guild_id, recruitment_id, user_id, status, joined_at)
                SELECT :guild, :id, (1000+n)::text, 'JOINED', now() FROM generate_series(1,10) n
            """),
                {"guild": GUILD_A, "id": record_id},
            )
            await connection.execute(
                text("""
                INSERT INTO recruitment_entry_assignments
                    (guild_id, recruitment_id, user_id, team, lane)
                SELECT :guild, :id, (1000+n)::text,
                    CASE WHEN n <= 5 THEN 'TEAM_1' ELSE 'TEAM_2' END,
                    (ARRAY['TOP','JUNGLE','MID','BOTTOM','SUPPORT'])[(n-1)%5+1]
                FROM generate_series(1,10) n
            """),
                {"guild": GUILD_A, "id": record_id},
            )
    return path, headers


async def call(
    client: httpx.AsyncClient,
    path: str,
    headers: dict[str, str],
    action: str,
    revision: int = 0,
    key: str | None = None,
) -> httpx.Response:
    return await client.request(
        "DELETE" if action == "cancel" else "POST",
        path + ("" if action == "cancel" else "/" + action),
        json={} if action == "close" else {"expectedRevision": revision},
        headers={**headers, "idempotency-key": key or action},
    )


async def oracle(path: str, action: str, revision: int = 0, key: str | None = None) -> dict:
    return await asyncio.to_thread(
        rust_auth,
        {
            "operation": "recruitment_lifecycle",
            "guild_id": GUILD_A,
            "user_id": USER_ID,
            "recruitment_id": path.split("/")[-1],
            "action": action,
            "key": key or action,
            "expected_revision": revision,
        },
    )
