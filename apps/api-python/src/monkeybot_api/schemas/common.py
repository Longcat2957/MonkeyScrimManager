import re

from pydantic import BaseModel, ConfigDict, Field

from monkeybot_api.core.config import is_snowflake


def user_id(value: str) -> str:
    if is_snowflake(value) or re.fullmatch(
        r"dmy_[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}", value
    ):
        return value
    raise ValueError("invalid user ID")


class Revision(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)
    expectedRevision: int = Field(ge=-(2**63), le=2**63 - 1)
