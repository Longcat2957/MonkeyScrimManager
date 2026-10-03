import base64
import binascii
import json
import re
import uuid
from dataclasses import dataclass
from datetime import datetime, timedelta
from typing import Any, Literal

from pydantic import BaseModel, ConfigDict

from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.json import unique_json_fields
from monkeybot_api.core.pagination import EPOCH, PG_EPOCH


@dataclass(frozen=True)
class AuditLogCursor:
    created_at_nanos: int
    id: uuid.UUID

    def database_timestamp(self) -> datetime:
        # SQLx truncates the duration from PostgreSQL's epoch toward zero, including
        # submicrosecond cursors before 2000. Preserve that boundary exactly.
        nanos = self.created_at_nanos - 946684800 * 1_000_000_000
        micros = (abs(nanos) // 1000) * (-1 if nanos < 0 else 1)
        return PG_EPOCH + timedelta(microseconds=micros)


class AuditLogQuery(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid")

    cursor: str | None = None
    limit: str | None = None

    def page_parameters(self) -> tuple[AuditLogCursor | None, int]:
        value = self.limit if self.limit is not None else "50"
        try:
            if not re.fullmatch(r"\+?[0-9]+", value):
                raise ValueError("invalid unsigned integer")
            digits = value.removeprefix("+").lstrip("0")
            if len(digits) > 3:
                raise ValueError("out of range")
            limit = int(digits or "0")
            if not 1 <= limit <= 100:
                raise ValueError("out of range")
        except ValueError:
            raise ApiError(
                400,
                "invalid_limit",
                "limit은 1 이상 100 이하이어야 합니다.",
                {"min": 1, "max": 100},
            ) from None
        return decode_cursor(self.cursor) if self.cursor is not None else None, limit


def cursor_token(cursor: AuditLogCursor) -> str:
    payload = json.dumps(
        {"createdAtNanos": cursor.created_at_nanos, "id": str(cursor.id)}, separators=(",", ":")
    )
    return base64.urlsafe_b64encode(payload.encode()).decode().rstrip("=")


def encode_cursor(created_at: datetime, record_id: uuid.UUID) -> str:
    delta = created_at - EPOCH
    nanos = ((delta.days * 86400 + delta.seconds) * 1_000_000 + delta.microseconds) * 1000
    if not -(2**63) <= nanos < 2**63:
        raise ValueError("audit cursor timestamp exceeds the signed 64-bit contract")
    return cursor_token(AuditLogCursor(nanos, record_id))


def decode_cursor(value: str) -> AuditLogCursor:
    try:
        if not 1 <= len(value) <= 512 or not re.fullmatch(r"[A-Za-z0-9_-]+", value):
            raise ValueError("invalid base64url")
        raw = base64.b64decode(value + "=" * (-len(value) % 4), altchars=b"-_", validate=True)
        token = json.loads(raw, object_pairs_hook=unique_json_fields)
        if not isinstance(token, dict) or set(token) != {"createdAtNanos", "id"}:
            raise ValueError("invalid cursor fields")
        nanos = token["createdAtNanos"]
        if type(nanos) is not int or not -(2**63) <= nanos < 2**63:
            raise ValueError("invalid cursor timestamp")
        cursor = AuditLogCursor(nanos, uuid.UUID(token["id"]))
        if cursor_token(cursor) != value:
            raise ValueError("noncanonical cursor")
        return cursor
    except ValueError, TypeError, AttributeError, binascii.Error, ApiError:
        raise ApiError(400, "invalid_cursor", "cursor 형식이 올바르지 않습니다.") from None


class AuditLogEntry(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid")

    id: str
    principalType: Literal["USER", "BOT", "SYSTEM", "MIGRATION"]
    principalId: str
    principalDisplayName: str | None
    action: str
    targetType: str | None
    targetId: str | None
    targetDisplayName: str | None
    checkpointId: str | None
    requestId: str | None
    beforeSummary: Any
    afterSummary: Any
    result: Literal["APPLIED", "REVERTED"]
    note: str | None
    createdAt: str


class AuditLogPage(BaseModel):
    entries: list[AuditLogEntry]
    nextCursor: str | None
