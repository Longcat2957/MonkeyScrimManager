"""Expired transfer preview cleanup and its cancellable lifespan worker."""

import asyncio
import json
import uuid
from collections.abc import AsyncIterator
from contextlib import asynccontextmanager
from datetime import UTC, datetime, timedelta
from typing import Any

import pytest
from pydantic import SecretStr
from sqlalchemy import text

from monkeybot_api.core.config import Settings
from monkeybot_api.db.database import Database
from monkeybot_api.repositories.guilds import full_data_import, legacy_data_transfer
from monkeybot_api.services.guilds.transfer_retention import (
    _skip_missed_ticks,
    cleanup_expired_previews,
    run_transfer_retention,
)
from tests.support.database import isolated_database_url
from tests.support.guilds.context import GUILD_A, ROLE_A, USER_ID, guild_fixture


@pytest.mark.postgres
async def test_cleanup_uses_utc_cutoff_and_preserves_future_and_applied_rows(owner) -> None:
    async with owner.begin() as connection:
        await connection.execute(
            text("""
                INSERT INTO users (user_id, identity_kind, display_name, created_at, updated_at)
                VALUES (:user, 'DISCORD', 'Transfer actor', now(), now())
            """),
            {"user": USER_ID},
        )
    season = await guild_fixture(owner, GUILD_A, "Transfer guild", ROLE_A)
    cutoff = datetime.now(UTC).replace(microsecond=0) - timedelta(hours=1)
    created = cutoff - timedelta(days=2)
    ids: dict[tuple[str, str], uuid.UUID] = {}

    async with owner.begin() as connection:
        for state, expiry in (
            ("past", cutoff - timedelta(seconds=1)),
            ("boundary", cutoff),
            ("future", cutoff + timedelta(minutes=30)),
            ("applied", cutoff - timedelta(seconds=1)),
        ):
            legacy_id, full_id = uuid.uuid7(), uuid.uuid7()
            ids[("legacy", state)] = legacy_id
            ids[("full", state)] = full_id
            await legacy_data_transfer.create_preview(
                connection,
                {
                    "guild": GUILD_A,
                    "import_id": legacy_id,
                    "actor_id": USER_ID,
                    "digest": b"a" * 32,
                    "archive": json.dumps({"state": state}),
                    "source_guild": GUILD_A,
                    "source_season": season,
                    "season": season,
                    "revision": 0,
                    "mmr_digest": b"b" * 32,
                    "summary": "{}",
                    "conflict_count": 0,
                    "request_id": f"legacy-{state}",
                    "now": created,
                    "expires": expiry,
                },
            )
            await full_data_import.store_preview(
                connection,
                {
                    "guild": GUILD_A,
                    "import_id": full_id,
                    "actor_id": USER_ID,
                    "archive_digest": b"c" * 32,
                    "archive_payload": json.dumps({"state": state}),
                    "source_guild_id": GUILD_A,
                    "target_digest": b"d" * 32,
                    "summary": "{}",
                    "conflicts": "[]",
                    "conflict_count": 0,
                    "request_id": f"full-{state}",
                    "now": created,
                    "expires": expiry,
                },
            )
        for table, import_id in (
            ("guild_data_imports", ids[("legacy", "applied")]),
            ("guild_full_data_imports", ids[("full", "applied")]),
        ):
            # Both table identifiers come from this closed test fixture.
            await connection.execute(
                text(f"""
                    UPDATE {table} SET status='APPLIED', archive_payload=NULL, applied_at=:applied
                    WHERE guild_id=:guild AND id=:import_id
                """),
                {"applied": cutoff, "guild": GUILD_A, "import_id": import_id},
            )

    settings = Settings(
        env="test",
        public_origin="http://localhost:5173",
        database_url=SecretStr(isolated_database_url("FASTAPI_TEST_RUNTIME_URL")),
    )
    database = Database(settings)
    try:
        assert await cleanup_expired_previews(database, clock=lambda: cutoff) == (2, 2)
        async with owner.connect() as connection:
            legacy = (
                await connection.execute(
                    text("SELECT id, status FROM guild_data_imports WHERE guild_id=:guild"),
                    {"guild": GUILD_A},
                )
            ).all()
            full = (
                await connection.execute(
                    text("SELECT id, status FROM guild_full_data_imports WHERE guild_id=:guild"),
                    {"guild": GUILD_A},
                )
            ).all()
        assert set(legacy) == {
            (ids[("legacy", "future")], "PREVIEWED"),
            (ids[("legacy", "applied")], "APPLIED"),
        }
        assert set(full) == {
            (ids[("full", "future")], "PREVIEWED"),
            (ids[("full", "applied")], "APPLIED"),
        }
        assert await cleanup_expired_previews(
            database, clock=lambda: cutoff + timedelta(hours=1)
        ) == (1, 1)
        assert await cleanup_expired_previews(
            database, clock=lambda: cutoff + timedelta(hours=1)
        ) == (0, 0)
    finally:
        await database.close()


class _Result:
    def __init__(self, count: int) -> None:
        self.rowcount = count


class _Connection:
    def __init__(self, database: _FakeDatabase) -> None:
        self.database = database

    async def execute(self, query: Any, values: dict[str, datetime]) -> _Result:
        assert values["now"].tzinfo is UTC
        self.database.calls += 1
        if self.database.block:
            self.database.started.set()
            await self.database.release.wait()
        if "guild_data_imports" in str(query) and self.database.fail_legacy_once:
            self.database.fail_legacy_once = False
            raise RuntimeError("simulated transient database failure")
        return _Result(2)


class _FakeDatabase:
    def __init__(self, *, fail_legacy_once: bool = False, block: bool = False) -> None:
        self.engine = object()
        self.calls = 0
        self.fail_legacy_once = fail_legacy_once
        self.block = block
        self.started = asyncio.Event()
        self.release = asyncio.Event()
        self.connection_closed = asyncio.Event()

    @asynccontextmanager
    async def connection(self, *, transaction: bool = False) -> AsyncIterator[_Connection]:
        assert transaction
        try:
            yield _Connection(self)
        finally:
            self.connection_closed.set()


async def test_cleanup_continues_other_table_and_retries_next_sweep() -> None:
    database = _FakeDatabase(fail_legacy_once=True)

    def clock() -> datetime:
        return datetime.now(UTC)

    assert await cleanup_expired_previews(database, clock=clock) == (0, 2)  # type: ignore[arg-type]
    assert await cleanup_expired_previews(database, clock=clock) == (2, 2)  # type: ignore[arg-type]
    assert database.calls == 4


async def test_worker_sweeps_immediately_and_cancels_blocked_query() -> None:
    database = _FakeDatabase(block=True)
    task = asyncio.create_task(run_transfer_retention(database, interval_seconds=0.01))  # type: ignore[arg-type]
    await asyncio.wait_for(database.started.wait(), timeout=1)
    assert database.calls == 1
    task.cancel()
    with pytest.raises(asyncio.CancelledError):
        await task
    await asyncio.wait_for(database.connection_closed.wait(), timeout=1)


def test_retention_skips_missed_ticks_after_slow_sweep() -> None:
    assert _skip_missed_ticks(60, 59.9, 60) == 60
    assert _skip_missed_ticks(60, 60, 60) == 120
    assert _skip_missed_ticks(60, 70, 60) == 120
    assert _skip_missed_ticks(60, 190, 60) == 240
