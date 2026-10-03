"""Reusable fixtures and request builders; no test module imports."""

import hashlib
import uuid

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncEngine

USER_ID = "328516947210438656"


GUILD_ID = "954301284660912138"


TOKEN = "integration-session-token"


async def seed_session(owner: AsyncEngine) -> None:
    async with owner.begin() as connection:
        await connection.execute(
            text("""
            INSERT INTO users
                (user_id, identity_kind, display_name, avatar_hash, created_at, updated_at)
            VALUES (:user_id, 'DISCORD', '세션 사용자', 'a_0123abcdef', now(), now())
        """),
            {"user_id": USER_ID},
        )
        await connection.execute(
            text("""
            INSERT INTO user_discord_identities (user_id, discord_id, created_at, updated_at)
            VALUES (:user_id, :user_id, now(), now())
        """),
            {"user_id": USER_ID},
        )
        await connection.execute(
            text("""
            INSERT INTO user_sessions
                (id, user_id, token_hash, csrf_secret_hash, created_at, last_seen_at, expires_at)
            VALUES (:id, :user_id, :token_hash, :csrf_hash, now() - interval '1 hour',
                    now() - interval '10 minutes', now() + interval '1 hour')
        """),
            {
                "id": uuid.uuid7(),
                "user_id": USER_ID,
                "token_hash": hashlib.sha256(TOKEN.encode()).digest(),
                "csrf_hash": hashlib.sha256(b"integration-csrf").digest(),
            },
        )
