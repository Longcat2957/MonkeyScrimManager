import json
import re
from typing import Any

from fastapi import Request

from monkeybot_api.core.config import is_snowflake
from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.json import reject_json_constant


async def command_json(request: Request) -> Any:
    media = request.headers.get("content-type", "").split(";", 1)[0].strip().lower()
    if media != "application/json" and not (
        media.startswith("application/") and media.endswith("+json")
    ):
        raise ApiError.from_status(415)
    try:
        # Axum first decodes Json<Value>: duplicate object fields use the final value.
        raw = json.loads(
            (await request.body()).decode("utf-8"), parse_constant=reject_json_constant
        )
    except ValueError, UnicodeDecodeError:
        raise ApiError.from_status(400) from None
    return raw


def command_key(request: Request, bot: bool) -> str:
    keys = request.headers.getlist("idempotency-key")
    key = keys[0] if len(keys) == 1 else ""
    if (bot and not is_snowflake(key)) or (
        not bot and not re.fullmatch(r"[\x21-\x7e]{1,128}", key)
    ):
        raise ApiError(
            400,
            "invalid_idempotency_key",
            "Idempotency-Key는 Discord interaction ID여야 합니다."
            if bot
            else "Idempotency-Key 헤더가 필요합니다.",
            None if bot else {"maxLength": 128},
        )
    return key
