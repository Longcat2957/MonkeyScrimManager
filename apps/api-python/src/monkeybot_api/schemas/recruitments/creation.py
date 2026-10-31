import re
from datetime import UTC, datetime, timedelta
from typing import Any

from pydantic import BaseModel, ConfigDict, Field

from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.time import timestamp
from monkeybot_api.schemas.bot import Actor


class BrowserCreate(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid")
    bestOf: int | None = Field(default=None, ge=-32768, le=32767)
    hardFearless: bool | None = None
    expectedStartAt: Any = None


class BotCreate(BrowserCreate):
    actor: Actor


def canonical_start(value: Any) -> tuple[datetime, str]:
    if value is None or (isinstance(value, str) and not value.strip()):
        date = datetime.now(UTC) + timedelta(minutes=30)
        return date, timestamp(date)
    try:
        if not isinstance(value, str):
            raise ValueError("timestamp must be a string")
        match = re.fullmatch(
            r"(\d{4}-\d{2}-\d{2})[Tt](\d{2}:\d{2}:)(\d{2})(?:\.(\d+))?([Zz]|[+-]\d{2}:\d{2})",
            value.strip(),
        )
        if match is None:
            raise ValueError("invalid RFC3339")
        day, clock, second, fraction, offset = match.groups()
        fraction = (fraction or "")[:9]
        if second == "60":
            second, fraction = "59", "999999999"
        offset = "+00:00" if offset.lower() == "z" else offset
        if int(offset[1:3]) > 23 or int(offset[4:]) > 59:
            raise ValueError("invalid offset")
        date = datetime.fromisoformat(f"{day}T{clock}{second}.{fraction or '0'}{offset}")
        fraction = fraction.rstrip("0")
        canonical = f"{day}T{clock}{second}" + (f".{fraction}" if fraction else "")
        canonical += "Z" if date.utcoffset() == timedelta(0) else offset
        return date, canonical
    except ValueError, OverflowError:
        raise ApiError(
            422,
            "invalid_expected_start_at",
            "예상 시작 시간은 시간대가 포함된 ISO 8601 형식이어야 합니다.",
            {"field": "expectedStartAt"},
        ) from None
