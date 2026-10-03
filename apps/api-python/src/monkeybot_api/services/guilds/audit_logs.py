import logging
from typing import Any

from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.time import timestamp
from monkeybot_api.db.database import Database
from monkeybot_api.db.transactions import repeatable_read
from monkeybot_api.repositories.guilds import audit_logs as repository
from monkeybot_api.repositories.permissions import authorize
from monkeybot_api.schemas.guilds.audit_logs import (
    AuditLogCursor,
    AuditLogEntry,
    AuditLogPage,
    encode_cursor,
)

logger = logging.getLogger(__name__)


async def list_audit_logs(
    database: Database,
    guild: str,
    actor: str,
    cursor: AuditLogCursor | None,
    limit: int,
    request_id: str,
) -> dict[str, Any]:
    async with database.connection(transaction=True) as connection:
        await repeatable_read(connection)
        await authorize(connection, guild, actor)
        rows = await repository.fetch_page(connection, guild, cursor, limit)
        retained = rows[:limit]
        try:
            next_cursor = (
                encode_cursor(retained[-1]["created_at"], retained[-1]["id"])
                if len(rows) > limit
                else None
            )
            entries = [
                AuditLogEntry(
                    id=str(row["id"]),
                    principalType=row["principal_type"],
                    principalId=row["principal_id"],
                    principalDisplayName=row["principal_display_name"],
                    action=row["action"],
                    targetType=row["target_type"],
                    targetId=row["target_id"],
                    targetDisplayName=row["target_display_name"],
                    checkpointId=str(row["checkpoint_id"]) if row["checkpoint_id"] else None,
                    requestId=row["request_id"],
                    beforeSummary=row["before_summary"],
                    afterSummary=row["after_summary"],
                    result=row["result"],
                    note=row["note"],
                    createdAt=timestamp(row["created_at"]),
                )
                for row in retained
            ]
        except ValueError:
            logger.exception("audit_log_projection_failed", extra={"request_id": request_id})
            raise ApiError.from_status(500) from None
        return AuditLogPage(entries=entries, nextCursor=next_cursor).model_dump()
