from typing import Annotated, Literal

from pydantic import AfterValidator, Field

from monkeybot_api.schemas.notifications import valid_message
from monkeybot_api.schemas.series import Strict

Snowflake = Annotated[str, AfterValidator(valid_message)]


class CatalogRole(Strict):
    id: Snowflake
    name: str
    position: int = Field(ge=-(2**31), le=2**31 - 1)
    managed: bool


class CatalogChannel(Strict):
    id: Snowflake
    name: str
    parentName: str | None = None
    position: int = Field(ge=-(2**31), le=2**31 - 1)
    kind: Literal["TEXT", "ANNOUNCEMENT"]


class Catalog(Strict):
    roles: list[CatalogRole]
    channels: list[CatalogChannel]


class UpdateSettings(Strict):
    displayName: str
    operatorRoleId: Snowflake
    recruitmentChannelId: Snowflake | None = None
    entryChannelId: Snowflake | None = None
    resultChannelId: Snowflake | None = None
    logChannelId: Snowflake | None = None
    isExternallyPublic: bool
    expectedRevision: int = Field(ge=-(2**63), le=2**63 - 1)
