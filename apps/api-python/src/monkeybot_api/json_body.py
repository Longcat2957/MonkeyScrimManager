import json
from typing import Any

from fastapi import Request
from pydantic import BaseModel, ValidationError

from monkeybot_api.errors import ApiError


def unique_json_fields(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise ApiError.from_status(422)
        result[key] = value
    return result


def reject_json_constant(value: str) -> None:
    raise ValueError("non-finite JSON constant")


async def read_json[T: BaseModel](request: Request, model: type[T]) -> T:
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
        )
    except ValueError, UnicodeDecodeError:
        raise ApiError.from_status(400) from None
    try:
        return model.model_validate(value)
    except ValidationError:
        raise ApiError.from_status(422) from None
