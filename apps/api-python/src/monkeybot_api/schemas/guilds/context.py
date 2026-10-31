from pydantic import BaseModel, ConfigDict, field_validator

from monkeybot_api.core.config import is_snowflake


class DefaultGuildInput(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)
    guildId: str

    @field_validator("guildId")
    @classmethod
    def validate_guild_id(cls, value: str) -> str:
        if not is_snowflake(value):
            raise ValueError("invalid guild snowflake")
        return value
