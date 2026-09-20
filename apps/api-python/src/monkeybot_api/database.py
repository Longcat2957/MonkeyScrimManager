import asyncio
import json
import logging
from collections.abc import AsyncIterator, Awaitable
from contextlib import asynccontextmanager
from typing import Any, cast

from redis.asyncio import Redis
from sqlalchemy import text
from sqlalchemy.engine import make_url
from sqlalchemy.ext.asyncio import AsyncConnection, AsyncEngine, create_async_engine

from monkeybot_api.config import Settings
from monkeybot_api.errors import ApiError

logger = logging.getLogger(__name__)


def create_engine(settings: Settings) -> AsyncEngine:
    assert settings.database_url is not None
    url = make_url(settings.database_url.get_secret_value()).set(drivername="postgresql+asyncpg")
    options: dict[str, Any] = {
        "timeout": settings.database_acquire_timeout_ms / 1000,
        "server_settings": {
            "application_name": f"monkeybot-api-python-{settings.env}",
            "search_path": "public, pg_catalog",
            "TimeZone": "UTC",
            "statement_timeout": f"{settings.database_statement_timeout_ms}ms",
            "lock_timeout": f"{settings.database_lock_timeout_ms}ms",
            "idle_in_transaction_session_timeout": (
                f"{settings.database_idle_transaction_timeout_ms}ms"
            ),
        },
    }
    if "sslmode" in url.query:
        options["ssl"] = url.query["sslmode"]
        url = url.difference_update_query(["sslmode"])
    return create_async_engine(
        url,
        connect_args=options,
        pool_size=settings.database_max_connections,
        max_overflow=0,
        pool_timeout=settings.database_acquire_timeout_ms / 1000,
        pool_recycle=settings.database_max_lifetime_ms / 1000,
        pool_pre_ping=True,
        hide_parameters=True,
    )


class Database:
    def __init__(self, settings: Settings) -> None:
        self.settings = settings
        self.engine = create_engine(settings) if settings.database_url is not None else None
        self.valkey: Redis | None = (
            Redis.from_url(settings.valkey_url.get_secret_value())
            if settings.valkey_url is not None
            else None
        )
        manifest = json.loads(settings.migration_manifest.read_text())
        self.expected_migrations = {
            entry["version"]: bytes.fromhex(entry["sha384"]) for entry in manifest["migrations"]
        }
        if not self.expected_migrations or len(self.expected_migrations) != len(
            manifest["migrations"]
        ):
            raise ValueError("SQLx migration manifest must contain unique versions")
        self.accepting_requests = False

    @asynccontextmanager
    async def connection(self, *, transaction: bool = False) -> AsyncIterator[AsyncConnection]:
        if self.engine is None:
            raise ApiError(
                503, "database_unavailable", "데이터베이스를 일시적으로 사용할 수 없습니다."
            )
        try:
            async with self.engine.begin() if transaction else self.engine.connect() as connection:
                yield connection
        except (OSError, TimeoutError) as error:
            logger.error("database_transport_failed", extra={"failure_type": type(error).__name__})
            raise ApiError(
                503, "database_unavailable", "데이터베이스를 일시적으로 사용할 수 없습니다."
            ) from error

    async def start(self) -> None:
        if self.engine is not None:
            async with self.engine.connect() as connection:
                await connection.execute(text("SELECT 1"))
        self.accepting_requests = True

    async def close(self) -> None:
        self.accepting_requests = False
        try:
            if self.valkey is not None:
                await self.valkey.aclose()
        finally:
            if self.engine is not None:
                await self.engine.dispose()

    async def postgres_status(self) -> tuple[str, str]:
        if self.engine is None:
            return "not_configured", "not_configured"
        postgres = "unavailable"
        try:
            async with asyncio.timeout(self.settings.database_probe_timeout_ms / 1000):
                async with self.engine.connect() as connection:
                    await connection.execute(text("SELECT 1"))
                    postgres = "available"
                    rows = (
                        (
                            await connection.execute(
                                text(
                                    "SELECT version, success, checksum FROM public._sqlx_migrations"
                                )
                            )
                        )
                        .mappings()
                        .all()
                    )
                    applied = {row["version"]: bytes(row["checksum"]) for row in rows}
                    valid = (
                        all(row["success"] for row in rows) and applied == self.expected_migrations
                    )
                    return postgres, "available" if valid else "unavailable"
        except Exception as error:
            logger.warning(
                "readiness_postgres_failed", extra={"failure_type": type(error).__name__}
            )
            return postgres, "unavailable"

    async def valkey_status(self) -> str:
        if self.valkey is None:
            return "not_configured"
        try:
            async with asyncio.timeout(self.settings.database_probe_timeout_ms / 1000):
                return (
                    "available"
                    if await cast(Awaitable[bool], self.valkey.ping())
                    else "unavailable"
                )
        except Exception as error:
            logger.warning("readiness_valkey_failed", extra={"failure_type": type(error).__name__})
            return "unavailable"

    async def readiness(self) -> dict[str, Any]:
        if not self.accepting_requests:
            postgres, migration, valkey = "unavailable", "unavailable", "not_configured"
        else:
            (postgres, migration), valkey = await asyncio.gather(
                self.postgres_status(), self.valkey_status()
            )
        ready = self.accepting_requests and postgres == migration == "available"
        return {
            "status": ("ready" if valkey == "available" else "degraded") if ready else "not_ready",
            "acceptingRequests": self.accepting_requests,
            "dependencies": {"postgres": postgres, "migration": migration, "valkey": valkey},
        }
