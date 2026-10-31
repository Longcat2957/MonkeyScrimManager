import uuid
from typing import Any


def mapped(row: dict[str, Any], fields: str) -> dict[str, Any]:
    result = {}
    for field in fields.split():
        parts = field.split("_")
        value = row[field]
        result[parts[0] + "".join(p.title() for p in parts[1:])] = (
            str(value) if isinstance(value, uuid.UUID) else value
        )
    return result
