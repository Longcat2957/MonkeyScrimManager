import uuid
from datetime import datetime
from typing import Any

from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.pagination import encode_cursor
from monkeybot_api.db.database import Database
from monkeybot_api.repositories.guild_access import require_guild_access
from monkeybot_api.repositories.recruitments.entries import entry_snapshot
from monkeybot_api.repositories.recruitments.queries import actionable_recruitments, repeatable_read
from monkeybot_api.repositories.recruitments.snapshots import recruitment_snapshot
from monkeybot_api.schemas.recruitments.summaries import recruitment_summary


async def list_recruitments(
    database: Database,
    guild_id: str,
    user_id: str,
    limit: int,
    cursor_time: datetime | None,
    cursor_id: uuid.UUID | None,
) -> dict[str, Any]:
    async with database.connection() as connection:
        await require_guild_access(connection, user_id, guild_id)
        rows = await actionable_recruitments(
            connection,
            {
                "user_id": user_id,
                "guild_id": guild_id,
                "cursor_time": cursor_time,
                "cursor_id": cursor_id,
                "fetch_limit": limit + 1,
            },
        )
    page = rows[:limit]
    next_cursor = None
    if len(rows) > limit:
        next_cursor = encode_cursor(page[-1]["updated_at"], page[-1]["id"])
    items = []
    for row in page:
        href = f"/g/{guild_id}/recruitments/{row['id']}"
        items.append(
            {
                **recruitment_summary(dict(row)),
                "joinedCount": row["participant_count"],
                "assignedCount": row["assigned_count"],
                "href": href + ("/entry" if row["status"] == "CLOSED" else ""),
            }
        )
    return {"recruitments": items, "nextCursor": next_cursor}


async def get_recruitment(
    database: Database,
    guild_id: str,
    user_id: str,
    record_id: uuid.UUID,
) -> dict[str, Any]:
    async with database.connection() as connection:
        await require_guild_access(connection, user_id, guild_id)
    async with database.connection(transaction=True) as connection:
        snapshot = await recruitment_snapshot(connection, guild_id, record_id)
        if snapshot["recruitment"]["status"] not in {"OPEN", "CLOSED", "CANCELLED"}:
            raise ApiError(404, "not_found", "요청한 모집을 찾을 수 없습니다.")
    return snapshot


async def get_entry_draft(
    database: Database,
    guild_id: str,
    user_id: str,
    record_id: uuid.UUID,
) -> dict[str, Any]:
    async with database.connection() as connection:
        await require_guild_access(connection, user_id, guild_id)
    async with database.connection(transaction=True) as connection:
        await repeatable_read(connection)
        snapshot = await entry_snapshot(connection, guild_id, record_id, user_id)
        if snapshot["recruitment"]["status"] not in {"OPEN", "CLOSED"}:
            raise ApiError(404, "not_found", "요청한 모집을 찾을 수 없습니다.")
    return snapshot
