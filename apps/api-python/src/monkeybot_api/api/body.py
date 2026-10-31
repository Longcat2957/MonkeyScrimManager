import json
from collections.abc import Callable

from fastapi import Request
from pydantic import BaseModel, ValidationError

from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.json import reject_json_constant, unique_json_fields


async def read_json[T: BaseModel](
    request: Request,
    model: type[T],
    *,
    parse_float: Callable[[str], float] = float,
    parse_int: Callable[[str], int | float] = int,
) -> T:
    media_type = request.headers.get("content-type", "").split(";", 1)[0].strip().lower()
    if media_type != "application/json" and not (
        media_type.startswith("application/") and media_type.endswith("+json")
    ):
        raise ApiError.from_status(415)
    try:
        value = json.loads(
            (await request.body()).decode("utf-8"),
            object_pairs_hook=unique_json_fields,
            parse_constant=reject_json_constant,
            parse_float=parse_float,
            parse_int=parse_int,
        )
    except ValueError, UnicodeDecodeError:
        raise ApiError.from_status(400) from None
    try:
        return model.model_validate(value)
    except ValidationError:
        raise ApiError.from_status(422) from None
