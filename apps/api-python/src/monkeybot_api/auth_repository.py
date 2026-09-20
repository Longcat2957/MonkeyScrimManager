import uuid
from datetime import UTC, datetime, timedelta
from typing import Any

from sqlalchemy import text

from monkeybot_api.auth import hash_secret
from monkeybot_api.database import Database
from monkeybot_api.discord import DiscordUser
from monkeybot_api.errors import ApiError


async def create_attempt(
    database: Database, attempt_id: uuid.UUID, state: str, ciphertext: bytes, redirect: str
) -> None:
    async with database.connection(transaction=True) as connection:
        await connection.execute(
            text("""
            WITH expired_attempts AS (
                DELETE FROM oauth_login_attempts WHERE id IN (
                    SELECT id FROM oauth_login_attempts WHERE expires_at <= CURRENT_TIMESTAMP
                    ORDER BY expires_at, id LIMIT 1000
                )
            )
            INSERT INTO oauth_login_attempts
                (id, state_hash, pkce_verifier_ciphertext, redirect_path, created_at, expires_at)
            VALUES (:id, :hash, :ciphertext, :redirect, CURRENT_TIMESTAMP, :expires_at)
        """),
            {
                "id": attempt_id,
                "hash": hash_secret(state),
                "ciphertext": ciphertext,
                "redirect": redirect,
                "expires_at": datetime.now(UTC) + timedelta(minutes=5),
            },
        )


async def consume_attempt(database: Database, state: str) -> dict[str, Any] | None:
    # Consume before calling Discord; cancellation/provider failure cannot replay the attempt.
    async with database.connection(transaction=True) as connection:
        row = (
            (
                await connection.execute(
                    text("""
            UPDATE oauth_login_attempts SET consumed_at = CURRENT_TIMESTAMP
            WHERE state_hash = :hash AND consumed_at IS NULL AND expires_at > CURRENT_TIMESTAMP
            RETURNING id, pkce_verifier_ciphertext, redirect_path
        """),
                    {"hash": hash_secret(state)},
                )
            )
            .mappings()
            .one_or_none()
        )
        return dict(row) if row is not None else None


async def complete_login(
    database: Database,
    user: DiscordUser,
    previous_token: str | None,
    token: str,
    csrf: str,
    ttl: int,
) -> None:
    async with database.connection(transaction=True) as connection:
        identity = {"user_id": user.id, "name": user.display_name, "avatar": user.avatar_hash}
        user_id = await connection.scalar(
            text("""
            INSERT INTO users
                (user_id, identity_kind, display_name, avatar_hash,
                 created_at, updated_at, deleted_at)
            VALUES (:user_id, 'DISCORD', :name, :avatar, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, NULL)
            ON CONFLICT (user_id) DO UPDATE
            SET display_name = EXCLUDED.display_name, avatar_hash = EXCLUDED.avatar_hash,
                updated_at = CURRENT_TIMESTAMP, deleted_at = NULL
            WHERE users.identity_kind = 'DISCORD'
            RETURNING user_id
        """),
            identity,
        )
        if user_id is None:
            raise ApiError.from_status(500)
        mapped = await connection.scalar(
            text("""
            INSERT INTO user_discord_identities (user_id, discord_id, created_at, updated_at)
            VALUES (:user_id, :user_id, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
            ON CONFLICT (discord_id) DO UPDATE SET updated_at = CURRENT_TIMESTAMP
            WHERE user_discord_identities.user_id = EXCLUDED.user_id
            RETURNING user_id
        """),
            identity,
        )
        if mapped != user_id:
            raise ApiError.from_status(500)
        await connection.execute(
            text("""
            INSERT INTO user_privacy_settings
                (user_id, public_profile_enabled, show_riot_ids,
                 allow_external_record_search, updated_at)
            VALUES (:user_id, true, true, false, CURRENT_TIMESTAMP)
            ON CONFLICT (user_id) DO NOTHING
        """),
            identity,
        )
        await connection.execute(
            text("""
            INSERT INTO user_guild_preferences (user_id, default_guild_id, updated_at)
            VALUES (:user_id, NULL, CURRENT_TIMESTAMP) ON CONFLICT (user_id) DO NOTHING
        """),
            identity,
        )
        rotated_from = None
        if previous_token is not None:
            previous = (
                (
                    await connection.execute(
                        text("""
                SELECT id, user_id FROM user_sessions
                WHERE token_hash = :hash AND revoked_at IS NULL
                  AND expires_at > CURRENT_TIMESTAMP FOR UPDATE
            """),
                        {"hash": hash_secret(previous_token)},
                    )
                )
                .mappings()
                .one_or_none()
            )
            if previous is not None:
                await connection.execute(
                    text("""
                    UPDATE user_sessions SET revoked_at = CURRENT_TIMESTAMP
                    WHERE id = :id AND revoked_at IS NULL
                """),
                    {"id": previous["id"]},
                )
                if previous["user_id"] == user_id:
                    rotated_from = previous["id"]
        await connection.execute(
            text("""
            INSERT INTO user_sessions
                (id, user_id, token_hash, csrf_secret_hash, rotated_from_id,
                 created_at, last_seen_at, expires_at, revoked_at)
            VALUES (:id, :user_id, :token_hash, :csrf_hash, :rotated_from,
                    CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, :expires_at, NULL)
        """),
            {
                "id": uuid.uuid7(),
                "user_id": user_id,
                "token_hash": hash_secret(token),
                "csrf_hash": hash_secret(csrf),
                "rotated_from": rotated_from,
                "expires_at": datetime.now(UTC) + timedelta(seconds=ttl),
            },
        )


async def revoke_session(database: Database, token_hash: bytes) -> None:
    async with database.connection(transaction=True) as connection:
        await connection.execute(
            text("""
            UPDATE user_sessions SET revoked_at = CURRENT_TIMESTAMP
            WHERE token_hash = :hash AND revoked_at IS NULL
        """),
            {"hash": token_hash},
        )
