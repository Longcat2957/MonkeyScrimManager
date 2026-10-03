import uuid

from monkeybot_api.core.config import is_snowflake
from monkeybot_api.core.errors import ApiError


def path(guild: str, identifier: str | None = None) -> uuid.UUID | None:
    try:
        if not is_snowflake(guild):
            raise ValueError("guild")
        return uuid.UUID(identifier) if identifier is not None else None
    except ValueError:
        raise ApiError.from_status(400) from None


def path_ids(guild: str, identifier: str) -> uuid.UUID:
    try:
        if not is_snowflake(guild):
            raise ValueError("guild")
        return uuid.UUID(identifier)
    except ValueError:
        raise ApiError.from_status(400) from None
