"""Member identity shared by the profile and complete records projections."""

from typing import Annotated, Literal

from pydantic import AfterValidator, BaseModel, ConfigDict

from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.riot_identity import rust_trim
from monkeybot_api.schemas.common import user_id

Lane = Literal["TOP", "JUNGLE", "MID", "BOTTOM", "SUPPORT"]


def validated_query(value: str) -> str:
    query = rust_trim(value)
    if not query or len(query) > 100:
        raise ApiError(
            400, "invalid_search_query", "검색어가 올바르지 않습니다.", {"maxLength": 100}
        )
    return query


class ProfileQuery(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid")
    q: str


class StoredMember(BaseModel):
    model_config = ConfigDict(strict=True, extra="ignore")
    user_id: Annotated[str, AfterValidator(user_id)]
    display_name: str
    avatar_hash: str | None
    main_lane: Lane | None
    public_profile_enabled: bool
    matched_by_riot_id: bool


class Member(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid")
    displayName: str
    avatarUrl: str | None
    mainLane: Lane | None
    profileHref: str | None


class VisibleRiotAccount(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid")
    platform: str
    gameName: str
    tagLine: str
    profileIconId: int | None
    isMain: bool


class ProfileData(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid")
    member: Member
    championPreferences: dict[Lane, list[str]]
    riotAccounts: list[VisibleRiotAccount]
