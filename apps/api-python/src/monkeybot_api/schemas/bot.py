from pydantic import BaseModel, ConfigDict, field_validator

from monkeybot_api.core.config import is_snowflake
from monkeybot_api.core.errors import ApiError

BOT_ID = "monkeybot-discord-bot"


class Actor(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid")
    id: str
    globalDisplayName: str
    displayName: str
    avatarHash: str | None = None
    roleIds: list[str]

    @field_validator("id")
    @classmethod
    def valid_id(cls, value: str) -> str:
        if not is_snowflake(value):
            raise ValueError("invalid snowflake")
        return value

    @field_validator("roleIds")
    @classmethod
    def valid_roles(cls, value: list[str]) -> list[str]:
        if any(not is_snowflake(role) for role in value):
            raise ValueError("invalid role snowflake")
        return value


def invalid_bot(field: str) -> ApiError:
    return ApiError(
        400, "invalid_bot_command", "Bot command 요청 형식이 올바르지 않습니다.", {"field": field}
    )


def validate_actor(actor: Actor) -> None:
    for name in ("globalDisplayName", "displayName"):
        value = getattr(actor, name)
        if not value.strip() or len(value) > 100:
            raise invalid_bot(f"actor.{name}")
        setattr(actor, name, value.strip())
    if actor.avatarHash is not None and len(actor.avatarHash.encode()) > 128:
        raise invalid_bot("actor.avatarHash")
    if len(actor.roleIds) > 250:
        raise invalid_bot("actor.roleIds")
