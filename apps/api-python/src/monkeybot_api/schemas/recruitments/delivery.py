import uuid
from typing import Literal

from pydantic import field_validator

from monkeybot_api.core.config import is_snowflake
from monkeybot_api.schemas.common import Revision
from monkeybot_api.schemas.recruitments.members import BotRemove

Action = Literal["bind", "dm", "series", "convert"]


class BindMessage(BotRemove):
    channelId: str
    messageId: str

    @field_validator("channelId", "messageId")
    @classmethod
    def validate_snowflake(cls, value: str) -> str:
        if not is_snowflake(value):
            raise ValueError("invalid snowflake")
        return value


class SeriesCreate(Revision):
    sourceRecruitmentId: str

    @field_validator("sourceRecruitmentId")
    @classmethod
    def validate_uuid(cls, value: str) -> str:
        return str(uuid.UUID(value))
