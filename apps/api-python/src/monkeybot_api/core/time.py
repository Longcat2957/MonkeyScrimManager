from datetime import UTC, datetime


def timestamp(value: datetime) -> str:
    result = value.astimezone(UTC).isoformat().removesuffix("+00:00")
    if "." in result:
        result = result.rstrip("0").rstrip(".")
    return result + "Z"
