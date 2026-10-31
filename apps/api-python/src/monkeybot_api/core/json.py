from typing import Any

from monkeybot_api.core.errors import ApiError


def unique_json_fields(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise ApiError.from_status(422)
        result[key] = value
    return result


def reject_json_constant(value: str) -> None:
    raise ValueError("non-finite JSON constant")
