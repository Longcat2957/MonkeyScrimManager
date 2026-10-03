import asyncio
from collections.abc import Awaitable, Callable
from typing import Any

from sqlalchemy import text
from sqlalchemy.exc import DBAPIError
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError
from monkeybot_api.db.database import Database


async def repeatable_read(connection: AsyncConnection) -> None:
    await connection.execute(text("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ"))


async def mutation(
    database: Database, operation: Callable[[AsyncConnection], Awaitable[dict[str, Any]]]
) -> dict[str, Any]:
    for attempt in range(3):
        try:
            async with database.connection(transaction=True) as c:
                result = await operation(c)
            return result
        except DBAPIError as exc:
            if getattr(exc.orig, "sqlstate", "") != "40P01":
                raise
            if attempt == 2:
                raise ApiError(
                    503,
                    "database_busy",
                    "데이터베이스가 사용 중입니다. 잠시 후 다시 시도해 주세요.",
                ) from None
            await asyncio.sleep(0.025 * 2**attempt)
    raise AssertionError("unreachable")
