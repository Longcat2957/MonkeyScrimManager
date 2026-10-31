"""Public Riot records request and recent-page response contracts."""

from typing import Annotated

from pydantic import BaseModel, ConfigDict, Field

from monkeybot_api.integrations.riot.cache import RiotAccount, RiotRecentMatch

Usize = Annotated[int, Field(ge=0, le=2**64 - 1)]


class RecordsRequest(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid")

    gameName: str
    tagLine: str
    platform: str


class RecentPageMeta(BaseModel):
    model_config = ConfigDict(strict=True, extra="ignore")

    offset: Usize
    requested: Usize
    returned: Usize
    hasMore: bool
    nextOffset: Usize | None


class RecentPage(BaseModel):
    model_config = ConfigDict(strict=True, extra="ignore")

    account: RiotAccount
    platform: str
    routingRegion: str
    targetPuuid: str
    matches: list[RiotRecentMatch]
    page: RecentPageMeta
