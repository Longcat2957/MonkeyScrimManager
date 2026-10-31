"""Real PostgreSQL equivalence and failure fencing for the Alembic baseline."""

import asyncio
import os
import subprocess
import uuid
from collections.abc import AsyncIterator
from pathlib import Path

import httpx
import pytest
from pydantic import SecretStr
from sqlalchemy import text
from sqlalchemy.engine import make_url
from sqlalchemy.ext.asyncio import AsyncEngine, create_async_engine

from monkeybot_api.app import create_app
from monkeybot_api.core.config import Settings
from monkeybot_api.db import migration_handover as handover
from tests.support.assertions import assert_failure
from tests.support.database import isolated_database_url

pytestmark = pytest.mark.postgres

ROOT = Path(__file__).resolve().parents[4]
RUNTIME_ROLE = "fastapi_test_runtime"


def _engine(url: str, *, autocommit: bool = False) -> AsyncEngine:
    parsed = make_url(url).set(drivername="postgresql+asyncpg")
    parsed = parsed.difference_update_query(["sslmode"])
    options = {"isolation_level": "AUTOCOMMIT"} if autocommit else {}
    return create_async_engine(parsed, connect_args={"ssl": "disable"}, **options)


@pytest.fixture
async def migration_databases() -> AsyncIterator[dict[str, str]]:
    owner_url = make_url(isolated_database_url("FASTAPI_TEST_OWNER_URL"))
    admin = _engine(
        owner_url.set(database="postgres").render_as_string(hide_password=False), autocommit=True
    )
    token = uuid.uuid4().hex[:12]
    names = {
        "sqlx": f"monkeybot_alembic_{token}_s",
        "fresh": f"monkeybot_alembic_{token}_f",
        "race": f"monkeybot_alembic_{token}_r",
    }
    try:
        async with admin.connect() as connection:
            for name in names.values():
                await connection.exec_driver_sql(f'CREATE DATABASE "{name}"')
        yield {
            kind: owner_url.set(database=name).render_as_string(hide_password=False)
            for kind, name in names.items()
        }
    finally:
        async with admin.connect() as connection:
            for name in names.values():
                await connection.exec_driver_sql(f'DROP DATABASE IF EXISTS "{name}" WITH (FORCE)')
        await admin.dispose()


def _run_sqlx(url: str) -> None:
    binary = ROOT / "target/debug/monkeybot-migrate"
    if not binary.is_file():
        pytest.skip("SQLx migration binary is built by scripts/test-fastapi.sh")
    subprocess.run(
        [str(binary)],
        cwd=ROOT,
        env={**os.environ, "MONKEYBOT_MIGRATION_DATABASE_URL": url},
        check=True,
        capture_output=True,
        text=True,
    )


def _apply_runtime_grants(url: str, *, reset_enabled: bool = False) -> None:
    parsed = make_url(url)
    subprocess.run(
        [
            "psql",
            "-X",
            "-h",
            str(parsed.host),
            "-p",
            str(parsed.port),
            "-U",
            str(parsed.username),
            "-d",
            str(parsed.database),
            "--set",
            f"runtime_role={RUNTIME_ROLE}",
            "--set",
            f"service_admin_reset_enabled={'true' if reset_enabled else 'false'}",
            "-f",
            str(ROOT / "deploy/postgres/runtime-grants.sql"),
        ],
        cwd=ROOT,
        env={**os.environ, "PGPASSWORD": parsed.password or ""},
        check=True,
        capture_output=True,
        text=True,
    )


async def _ready(url: str) -> httpx.Response:
    runtime_url = (
        make_url(url)
        .set(username=RUNTIME_ROLE, password="isolated-runtime-only")
        .render_as_string(hide_password=False)
    )
    app = create_app(
        Settings(
            env="test",
            public_origin="http://localhost:5173",
            database_url=SecretStr(runtime_url),
        )
    )
    async with app.router.lifespan_context(app):
        async with httpx.AsyncClient(
            transport=httpx.ASGITransport(app=app), base_url="http://localhost"
        ) as client:
            return await client.get("/readyz")


async def _state(url: str) -> str:
    engine = _engine(url)
    try:
        async with engine.connect() as connection:
            return await connection.run_sync(handover.validate_migration_state)
    finally:
        await engine.dispose()


async def test_handover_bootstrap_catalog_grants_and_readiness(
    migration_databases: dict[str, str],
) -> None:
    sqlx_url = migration_databases["sqlx"]
    fresh_url = migration_databases["fresh"]
    _run_sqlx(sqlx_url)
    _apply_runtime_grants(sqlx_url, reset_enabled=True)
    assert await _state(sqlx_url) == "sqlx_legacy"
    assert (await _ready(sqlx_url)).status_code == 200

    with pytest.raises(handover.MigrationValidationError, match="reset function grant"):
        await handover.run_command("handover", sqlx_url, RUNTIME_ROLE)
    await handover.run_command("handover", sqlx_url, RUNTIME_ROLE, service_admin_reset_enabled=True)
    # The stamp adds two owner-only tables. The migration role reapplies runtime
    # grants before the application is allowed to start against this database.
    _apply_runtime_grants(sqlx_url, reset_enabled=True)
    await handover.run_command("bootstrap", fresh_url)
    _apply_runtime_grants(fresh_url, reset_enabled=True)
    await handover.run_command("validate", sqlx_url, RUNTIME_ROLE, service_admin_reset_enabled=True)
    await handover.run_command(
        "validate", fresh_url, RUNTIME_ROLE, service_admin_reset_enabled=True
    )
    assert await _state(sqlx_url) == "sqlx_handover"
    assert await _state(fresh_url) == "alembic_bootstrap"

    legacy = _engine(sqlx_url)
    fresh = _engine(fresh_url)
    try:
        async with legacy.connect() as connection:
            legacy_catalog = await connection.run_sync(handover.catalog_snapshot)
            legacy_grants = await connection.run_sync(handover.grant_snapshot, RUNTIME_ROLE)
            legacy_ledger = await connection.scalar(
                text("SELECT count(*) FROM public._sqlx_migrations")
            )
        async with fresh.connect() as connection:
            fresh_catalog = await connection.run_sync(handover.catalog_snapshot)
            fresh_grants = await connection.run_sync(handover.grant_snapshot, RUNTIME_ROLE)
            has_fake_ledger = await connection.scalar(
                text("SELECT to_regclass('public._sqlx_migrations') IS NOT NULL")
            )
        assert legacy_catalog == fresh_catalog
        legacy_business_grants = {
            name: privileges
            for name, privileges in legacy_grants["tables"].items()
            if name not in {"_sqlx_migrations", "alembic_version", "migration_provenance"}
        }
        fresh_business_grants = {
            name: privileges
            for name, privileges in fresh_grants["tables"].items()
            if name not in {"_sqlx_migrations", "alembic_version", "migration_provenance"}
        }
        assert legacy_business_grants == fresh_business_grants
        assert legacy_grants["schema"] == fresh_grants["schema"]
        assert legacy_grants["functions"] == fresh_grants["functions"]
        assert legacy_grants["default_table_privileges"] == fresh_grants["default_table_privileges"]
        assert legacy_ledger == 26
        assert has_fake_ledger is False
        assert (await _ready(sqlx_url)).status_code == 200
        assert (await _ready(fresh_url)).status_code == 200

        async with legacy.begin() as connection:
            await connection.execute(
                text("UPDATE public.alembic_version SET version_num='unknown_head'")
            )
        response = await _ready(sqlx_url)
        assert_failure(
            response, 503, "service_not_ready", "서비스가 요청을 받을 준비가 되지 않았습니다."
        )
        assert response.json()["error"]["details"]["dependencies"]["migration"] == "unavailable"
        async with legacy.begin() as connection:
            await connection.execute(
                text("UPDATE public.alembic_version SET version_num=:head"),
                {"head": handover.BASELINE_REVISION},
            )

        async with fresh.begin() as connection:
            await connection.execute(
                text("UPDATE public.migration_provenance SET baseline_schema_sha384=:digest"),
                {"digest": bytes(48)},
            )
        response = await _ready(fresh_url)
        assert_failure(
            response, 503, "service_not_ready", "서비스가 요청을 받을 준비가 되지 않았습니다."
        )
        async with fresh.begin() as connection:
            await connection.execute(
                text("UPDATE public.migration_provenance SET baseline_schema_sha384=:digest"),
                {"digest": bytes.fromhex(handover.BASELINE_CATALOG_SHA384)},
            )

        async with legacy.begin() as connection:
            await connection.execute(
                text("UPDATE public._sqlx_migrations SET success=false WHERE version=26")
            )
        response = await _ready(sqlx_url)
        assert_failure(
            response, 503, "service_not_ready", "서비스가 요청을 받을 준비가 되지 않았습니다."
        )
        async with legacy.begin() as connection:
            await connection.execute(
                text("UPDATE public._sqlx_migrations SET success=true WHERE version=26")
            )

        # A catalog change cannot be legitimized by intact metadata/provenance.
        async with fresh.begin() as connection:
            await connection.execute(text("ALTER TABLE public.guilds ADD COLUMN rogue text"))
        with pytest.raises(handover.MigrationValidationError, match="catalog differs"):
            await handover.run_command(
                "validate", fresh_url, RUNTIME_ROLE, service_admin_reset_enabled=True
            )
    finally:
        await legacy.dispose()
        await fresh.dispose()


async def test_rejected_handover_atomic_bootstrap_and_concurrent_fence(
    migration_databases: dict[str, str], monkeypatch: pytest.MonkeyPatch
) -> None:
    sqlx_url = migration_databases["sqlx"]
    fresh_url = migration_databases["fresh"]
    race_url = migration_databases["race"]
    _run_sqlx(sqlx_url)
    _apply_runtime_grants(sqlx_url)
    legacy = _engine(sqlx_url)
    try:
        async with legacy.begin() as connection:
            await connection.execute(
                text("""
                    GRANT EXECUTE ON FUNCTION public.service_admin_reset_guild_records(text)
                    TO fastapi_test_runtime
                """)
            )
        with pytest.raises(handover.MigrationValidationError, match="reset function grant"):
            await handover.run_command("handover", sqlx_url, RUNTIME_ROLE)
        _apply_runtime_grants(sqlx_url)

        async with legacy.begin() as connection:
            await connection.execute(
                text("""
                    GRANT EXECUTE ON FUNCTION public.service_admin_reset_guild_records(text)
                    TO pg_read_all_data
                """)
            )
        with pytest.raises(handover.MigrationValidationError, match="unapproved function"):
            await handover.run_command("handover", sqlx_url, RUNTIME_ROLE)
        async with legacy.begin() as connection:
            await connection.execute(
                text("""
                    REVOKE EXECUTE ON FUNCTION public.service_admin_reset_guild_records(text)
                    FROM pg_read_all_data
                """)
            )

        async with legacy.begin() as connection:
            await connection.execute(text("GRANT SELECT ON public.users TO pg_monitor"))
        with pytest.raises(handover.MigrationValidationError, match="unapproved table grant"):
            await handover.run_command("handover", sqlx_url, RUNTIME_ROLE)
        async with legacy.begin() as connection:
            await connection.execute(text("REVOKE SELECT ON public.users FROM pg_monitor"))

        async with legacy.begin() as connection:
            await connection.execute(text("ALTER TABLE public.guilds ADD COLUMN rogue text"))
        with pytest.raises(handover.MigrationValidationError, match="catalog differs"):
            await handover.run_command("handover", sqlx_url, RUNTIME_ROLE)
        assert await _state(sqlx_url) == "sqlx_legacy"
        async with legacy.begin() as connection:
            await connection.execute(text("ALTER TABLE public.guilds DROP COLUMN rogue"))

        async with legacy.begin() as connection:
            await connection.execute(text(f"REVOKE SELECT ON public.users FROM {RUNTIME_ROLE}"))
        with pytest.raises(handover.MigrationValidationError, match="grants differ"):
            await handover.run_command("handover", sqlx_url, RUNTIME_ROLE)
        _apply_runtime_grants(sqlx_url)

        original_stamp = handover.command.stamp
        with monkeypatch.context() as patch:
            patch.setattr(
                handover.command,
                "stamp",
                lambda *_args: (_ for _ in ()).throw(RuntimeError("stamp failed")),
            )
            with pytest.raises(RuntimeError, match="stamp failed"):
                await handover.run_command("handover", sqlx_url, RUNTIME_ROLE)
        assert handover.command.stamp is original_stamp
        assert await _state(sqlx_url) == "sqlx_legacy"

        # A modified bundled SQL file is rejected before any DDL.
        original_read = Path.read_bytes
        with monkeypatch.context() as patch:
            patch.setattr(
                Path,
                "read_bytes",
                lambda path: (
                    b"tampered" if path.name == "0001_bootstrap.sql" else original_read(path)
                ),
            )
            with pytest.raises(handover.MigrationValidationError, match="bundle checksum mismatch"):
                await handover.run_command("bootstrap", fresh_url)
        assert await _state(sqlx_url) == "sqlx_legacy"

        # Failure after executing all 26 DDL files must roll the entire schema back.
        frozen = handover.BASELINE_CATALOG_SHA384
        with monkeypatch.context() as patch:
            patch.setattr(handover, "BASELINE_CATALOG_SHA384", "0" * 96)
            with pytest.raises(handover.MigrationValidationError, match="catalog differs"):
                await handover.run_command("bootstrap", fresh_url)
        assert handover.BASELINE_CATALOG_SHA384 == frozen
        engine = _engine(fresh_url)
        try:
            async with engine.connect() as connection:
                assert not await connection.scalar(
                    text("SELECT to_regclass('public.guilds') IS NOT NULL")
                )
                assert not await connection.scalar(
                    text("SELECT to_regclass('public.alembic_version') IS NOT NULL")
                )
        finally:
            await engine.dispose()
        await handover.run_command("bootstrap", fresh_url)
        assert await _state(fresh_url) == "alembic_bootstrap"

        outcomes = await asyncio.gather(
            handover.run_command("bootstrap", race_url),
            handover.run_command("bootstrap", race_url),
            return_exceptions=True,
        )
        assert sum(result is None for result in outcomes) == 1
        assert (
            sum(isinstance(result, handover.MigrationValidationError) for result in outcomes) == 1
        )
        assert await _state(race_url) == "alembic_bootstrap"
    finally:
        await legacy.dispose()
