import json
from datetime import UTC, datetime
from pathlib import Path

import httpx
import pytest
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncEngine

from tests.support.assertions import assert_failure
from tests.support.auth.session import GUILD_ID, TOKEN, USER_ID, seed_session

pytestmark = pytest.mark.postgres


async def test_existing_session_hash_and_touch(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    await seed_session(owner)
    before = datetime.now(UTC)
    response = await runtime_client.get("/api/me", headers={"cookie": f"monkeybot_session={TOKEN}"})
    assert response.status_code == 200
    assert response.json()["data"] == {
        "discordId": USER_ID,
        "displayName": "세션 사용자",
        "avatarUrl": f"https://cdn.discordapp.com/avatars/{USER_ID}/a_0123abcdef.webp?size=128",
        "defaultGuildId": None,
        "serviceAdmin": True,
    }
    async with owner.connect() as connection:
        last_seen = await connection.scalar(text("SELECT last_seen_at FROM user_sessions"))
    assert last_seen >= before


@pytest.mark.parametrize(
    "cookie",
    [
        None,
        "monkeybot_session=wrong",
        "monkeybot_session=",
        f"monkeybot_session={TOKEN}; monkeybot_session={TOKEN}",
    ],
)
async def test_missing_invalid_and_duplicate_session_cookies(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, cookie: str | None
) -> None:
    await seed_session(owner)
    response = await runtime_client.get("/api/me", headers={"cookie": cookie} if cookie else {})
    assert_failure(response, 401, "unauthorized", "인증이 필요합니다.")


async def test_duplicate_cookie_headers_are_rejected(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    await seed_session(owner)
    response = await runtime_client.get(
        "/api/me",
        headers=[
            ("cookie", f"monkeybot_session={TOKEN}"),
            ("cookie", f"monkeybot_session={TOKEN}"),
        ],
    )
    assert_failure(response, 401, "unauthorized", "인증이 필요합니다.")


async def test_database_transport_failure_has_safe_reason(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, monkeypatch: pytest.MonkeyPatch
) -> None:
    from unittest.mock import Mock

    await seed_session(owner)
    engine = runtime_client._transport.app.state.database.engine
    with monkeypatch.context() as patch:
        patch.setattr(
            type(engine),
            "connect",
            Mock(side_effect=ConnectionRefusedError("private upstream address")),
        )
        response = await runtime_client.get(
            "/api/me", headers={"cookie": f"monkeybot_session={TOKEN}"}
        )
    assert_failure(
        response, 503, "database_unavailable", "데이터베이스를 일시적으로 사용할 수 없습니다."
    )
    assert "private upstream address" not in response.text


async def test_valkey_failure_does_not_block_core_readiness(
    runtime_client: httpx.AsyncClient,
) -> None:
    from redis.asyncio import Redis

    runtime_client._transport.app.state.database.valkey = Redis.from_url("redis://127.0.0.1:1")
    response = await runtime_client.get("/readyz")
    assert response.status_code == 200
    report = response.json()["data"]
    assert report["status"] == "degraded"
    assert report["dependencies"] == {
        "postgres": "available",
        "migration": "available",
        "valkey": "unavailable",
    }


@pytest.mark.parametrize(
    "change",
    [
        "UPDATE user_sessions SET revoked_at = now()",
        "UPDATE user_sessions SET expires_at = now() - interval '1 minute'",
        "UPDATE users SET deleted_at = now()",
        "DELETE FROM user_discord_identities",
    ],
)
async def test_expired_revoked_deleted_or_unlinked_identity(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, change: str
) -> None:
    await seed_session(owner)
    async with owner.begin() as connection:
        await connection.execute(text(change))
    assert_failure(
        await runtime_client.get("/api/me", headers={"cookie": f"monkeybot_session={TOKEN}"}),
        401,
        "unauthorized",
        "인증이 필요합니다.",
    )


async def test_default_guild_requires_active_membership_and_guild(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    await seed_session(owner)
    async with owner.begin() as connection:
        await connection.execute(
            text("""
            INSERT INTO guilds (guild_id, discord_name, status, installed_at, updated_at)
            VALUES (:guild, '테스트 길드', 'ACTIVE', now(), now())
        """),
            {"guild": GUILD_ID},
        )
        await connection.execute(
            text("""
            INSERT INTO guild_members (guild_id, user_id, display_name, status, first_seen_at)
            VALUES (:guild, :user, '회원', 'ACTIVE', now())
        """),
            {"guild": GUILD_ID, "user": USER_ID},
        )
        await connection.execute(
            text("""
            INSERT INTO user_guild_preferences (user_id, default_guild_id, updated_at)
            VALUES (:user, :guild, now())
        """),
            {"guild": GUILD_ID, "user": USER_ID},
        )

    async def default_guild():
        response = await runtime_client.get(
            "/api/me", headers={"cookie": f"monkeybot_session={TOKEN}"}
        )
        assert response.status_code == 200
        return response.json()["data"]["defaultGuildId"]

    assert await default_guild() == GUILD_ID
    async with owner.begin() as connection:
        await connection.execute(text("UPDATE guild_members SET status = 'INACTIVE'"))
    assert await default_guild() is None
    async with owner.begin() as connection:
        await connection.execute(text("UPDATE guild_members SET status = 'ACTIVE'"))
        await connection.execute(text("UPDATE guilds SET status = 'DISABLED'"))
    assert await default_guild() is None


async def test_exact_migration_history_and_readiness(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    response = await runtime_client.get("/readyz")
    assert response.status_code == 200
    assert response.json()["data"]["status"] == "degraded"
    manifest = json.loads(
        (Path(__file__).parents[3] / "src/monkeybot_api/db/sqlx-manifest.json").read_text()
    )
    version = manifest["migrations"][0]["version"]
    checksum = bytes.fromhex(manifest["migrations"][0]["sha384"])
    try:
        async with owner.begin() as connection:
            await connection.execute(
                text("UPDATE _sqlx_migrations SET checksum = :checksum WHERE version = :version"),
                {"checksum": b"tampered", "version": version},
            )
        response = await runtime_client.get("/readyz")
        assert_failure(
            response, 503, "service_not_ready", "서비스가 요청을 받을 준비가 되지 않았습니다."
        )
        assert response.json()["error"]["details"]["dependencies"]["migration"] == "unavailable"
        assert (await runtime_client.get("/healthz")).status_code == 200
    finally:
        async with owner.begin() as connection:
            await connection.execute(
                text("UPDATE _sqlx_migrations SET checksum = :checksum WHERE version = :version"),
                {"checksum": checksum, "version": version},
            )


async def test_runtime_grants_preserve_migration_and_ledger_boundaries(
    runtime_client: httpx.AsyncClient,
) -> None:
    from pydantic import SecretStr
    from sqlalchemy.exc import DBAPIError

    from monkeybot_api.core.config import Settings
    from monkeybot_api.db.database import create_engine
    from tests.support.database import isolated_database_url

    engine = create_engine(
        Settings(
            env="test",
            public_origin="http://localhost",
            database_url=SecretStr(isolated_database_url("FASTAPI_TEST_RUNTIME_URL")),
        )
    )
    try:
        for forbidden in [
            "CREATE TABLE public.must_not_create (id int)",
            "UPDATE _sqlx_migrations SET success = false",
            "TRUNCATE user_sessions",
            "DELETE FROM mmr_ledger_entries",
            "UPDATE admin_audit_log SET created_at = now()",
        ]:
            with pytest.raises(DBAPIError) as caught:
                async with engine.begin() as connection:
                    await connection.execute(text(forbidden))
            assert caught.value.orig.sqlstate == "42501"
        async with engine.connect() as connection:
            row = (
                (
                    await connection.execute(
                        text("""
                SELECT current_setting('TimeZone') AS timezone,
                       current_setting('search_path') AS search_path,
                       current_setting('statement_timeout') AS statement_timeout,
                       current_setting('lock_timeout') AS lock_timeout
            """)
                    )
                )
                .mappings()
                .one()
            )
        assert dict(row) == {
            "timezone": "UTC",
            "search_path": "public, pg_catalog",
            "statement_timeout": "3s",
            "lock_timeout": "1s",
        }
    finally:
        await engine.dispose()
