"""Remove expired, unapplied guild transfer previews while the API is running."""

import asyncio
import logging
from collections.abc import Callable
from datetime import UTC, datetime

from sqlalchemy import text
from sqlalchemy.sql.elements import TextClause

from monkeybot_api.db.database import Database

RETENTION_INTERVAL_SECONDS = 60.0

_LEGACY_DELETE = text("""
    DELETE FROM guild_data_imports
    WHERE status = 'PREVIEWED' AND expires_at <= :now
""")
_FULL_DELETE = text("""
    DELETE FROM guild_full_data_imports
    WHERE status = 'PREVIEWED' AND expires_at <= :now
""")

logger = logging.getLogger(__name__)


def _utc_now() -> datetime:
    return datetime.now(UTC)


def _skip_missed_ticks(next_tick: float, now: float, interval_seconds: float) -> float:
    if next_tick <= now:
        next_tick += (int((now - next_tick) // interval_seconds) + 1) * interval_seconds
    return next_tick


async def _remove(database: Database, query: TextClause, now: datetime) -> int:
    async with database.connection(transaction=True) as connection:
        result = await connection.execute(query, {"now": now})
        return int(result.rowcount)


async def cleanup_expired_previews(
    database: Database, *, clock: Callable[[], datetime] = _utc_now
) -> tuple[int, int]:
    """Run each preview cleanup independently so one failure does not stop the other."""
    if database.engine is None:
        return 0, 0

    removed: list[int] = []
    for name, query in (("legacy", _LEGACY_DELETE), ("full", _FULL_DELETE)):
        try:
            count = await _remove(database, query, clock())
        except Exception:
            logger.warning(
                "guild_transfer_preview_cleanup_failed", extra={"kind": name}, exc_info=True
            )
            count = 0
        else:
            if count:
                logger.info(
                    "guild_transfer_previews_removed", extra={"kind": name, "removed": count}
                )
        removed.append(count)
    return removed[0], removed[1]


async def run_transfer_retention(
    database: Database,
    *,
    interval_seconds: float = RETENTION_INTERVAL_SECONDS,
    clock: Callable[[], datetime] = _utc_now,
) -> None:
    """Sweep immediately, then every minute, skipping missed intervals after slow sweeps."""
    if interval_seconds <= 0:
        raise ValueError("retention interval must be positive")
    if database.engine is None:
        return

    loop = asyncio.get_running_loop()
    next_tick = loop.time()
    while True:
        await asyncio.sleep(max(0, next_tick - loop.time()))
        await cleanup_expired_previews(database, clock=clock)
        next_tick = _skip_missed_ticks(next_tick + interval_seconds, loop.time(), interval_seconds)
