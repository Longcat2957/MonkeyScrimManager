from monkeybot_api.core.config import is_snowflake
from monkeybot_api.core.errors import ApiError


def guild_path(guild: str) -> None:
    if not is_snowflake(guild):
        raise ApiError.from_status(400)
