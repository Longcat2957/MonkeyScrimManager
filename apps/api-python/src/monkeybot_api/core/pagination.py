import base64
import binascii
import json
import re
import uuid
from datetime import UTC, datetime, timedelta

from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.json import unique_json_fields

EPOCH = datetime(1970, 1, 1, tzinfo=UTC)


PG_EPOCH = datetime(2000, 1, 1, tzinfo=UTC)


def encode_cursor(updated_at: datetime, record_id: uuid.UUID) -> str:
    delta = updated_at - EPOCH
    nanos = ((delta.days * 86400 + delta.seconds) * 1_000_000 + delta.microseconds) * 1000
    if not -(2**63) <= nanos < 2**63:
        raise ApiError.from_status(500)
    payload = json.dumps({"updatedAtNanos": nanos, "id": str(record_id)}, separators=(",", ":"))
    return base64.urlsafe_b64encode(payload.encode()).decode().rstrip("=")


def decode_cursor(value: str) -> tuple[datetime, uuid.UUID]:
    try:
        if not re.fullmatch(r"[A-Za-z0-9_-]+", value):
            raise ValueError("invalid base64url")
        raw = base64.b64decode(value + "=" * (-len(value) % 4), altchars=b"-_", validate=True)
        if base64.urlsafe_b64encode(raw).decode().rstrip("=") != value:
            raise ValueError("noncanonical base64url")
        token = json.loads(raw, object_pairs_hook=unique_json_fields)
        if not isinstance(token, dict) or set(token) != {"updatedAtNanos", "id"}:
            raise ValueError("invalid cursor fields")
        nanos = token["updatedAtNanos"]
        if type(nanos) is not int or not -(2**63) <= nanos < 2**63:
            raise ValueError("invalid cursor timestamp")
        record_id = uuid.UUID(token["id"])
        # SQLx encodes time as whole microseconds since PostgreSQL's 2000 epoch.
        pg_nanos = nanos - 946684800 * 1_000_000_000
        micros = (abs(pg_nanos) // 1000) * (-1 if pg_nanos < 0 else 1)
        return PG_EPOCH + timedelta(microseconds=micros), record_id
    except ValueError, TypeError, AttributeError, binascii.Error, ApiError:
        raise ApiError(400, "invalid_cursor", "cursor 형식이 올바르지 않습니다.") from None
