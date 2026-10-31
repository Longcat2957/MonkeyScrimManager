"""Small positional-query helpers shared by repositories."""

from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError


def params(*values: Any) -> dict[str, Any]:
    return {f"p{i}": value for i, value in enumerate(values, 1)}


async def one(c: AsyncConnection, query: str, *values: Any) -> dict[str, Any] | None:
    row = (await c.execute(text(query), params(*values))).mappings().one_or_none()
    return dict(row) if row is not None else None


async def execute(c: AsyncConnection, query: str, *values: Any) -> None:
    result = await c.execute(text(query), params(*values))
    if result.rowcount != 1:
        raise ApiError.from_status(500)
