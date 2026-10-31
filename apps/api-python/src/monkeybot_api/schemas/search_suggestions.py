"""Suggestion query extraction and public result contracts."""

import re
from typing import Any, Literal, Self

from pydantic import BaseModel, ConfigDict, ValidationError, field_validator

from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.riot_identity import normalize_riot_id, rust_trim

_PLATFORMS = {
    "br": "BR1",
    "br1": "BR1",
    "eune": "EUN1",
    "eun1": "EUN1",
    "euw": "EUW1",
    "euw1": "EUW1",
    "jp": "JP1",
    "jp1": "JP1",
    "kr": "KR",
    "lan": "LA1",
    "la1": "LA1",
    "las": "LA2",
    "la2": "LA2",
    "na": "NA1",
    "na1": "NA1",
    "oce": "OC1",
    "oc1": "OC1",
    "ph": "PH2",
    "ph2": "PH2",
    "ru": "RU",
    "sg": "SG2",
    "sg2": "SG2",
    "th": "TH2",
    "th2": "TH2",
    "tr": "TR1",
    "tr1": "TR1",
    "tw": "TW2",
    "tw2": "TW2",
    "vn": "VN2",
    "vn2": "VN2",
}


class GuildSuggestionQuery(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid")

    q: str
    limit: int = 8

    @field_validator("limit", mode="before")
    @classmethod
    def unsigned_limit(cls, value: Any) -> int:
        if not isinstance(value, str) or re.fullmatch(r"\+?[0-9]+", value) is None:
            raise ValueError("invalid unsigned integer")
        digits = value.removeprefix("+").lstrip("0") or "0"
        if len(digits) > 20 or int(digits) > 2**64 - 1:
            raise ValueError("unsigned integer overflow")
        return int(digits)

    @classmethod
    def from_pairs(cls, pairs: list[tuple[str, str]]) -> Self:
        values = dict(pairs)
        if len(values) != len(pairs):
            raise ApiError.from_status(400)
        try:
            return cls.model_validate(values)
        except ValidationError:
            raise ApiError.from_status(400) from None

    def parameters(self, *, public: bool = False) -> tuple[str, int]:
        if not 1 <= self.limit <= 8:
            raise ApiError(
                400,
                "invalid_search_limit",
                "검색 결과 개수 설정이 올바르지 않습니다.",
                {"maxLimit": 8},
            )
        prefix = normalize_riot_id(self.q) if public else rust_trim(self.q)
        if len(prefix) < 2:
            raise ApiError(
                400,
                "invalid_search_query",
                "검색어를 두 글자 이상 입력해 주세요.",
                {"minLength": 2},
            )
        return prefix, self.limit


class PublicSuggestionQuery(GuildSuggestionQuery):
    platform: str = "KR"

    def normalized_platform(self) -> str:
        key = rust_trim(self.platform).translate(
            str.maketrans("ABCDEFGHIJKLMNOPQRSTUVWXYZ", "abcdefghijklmnopqrstuvwxyz")
        )
        if key not in _PLATFORMS:
            raise ApiError(
                400,
                "invalid_region",
                "지원하지 않는 Riot 플랫폼입니다.",
                {"platform": self.platform},
            )
        return _PLATFORMS[key]


class Suggestion(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid")

    kind: Literal["riotAccount", "guildMember"]
    value: str
    label: str
    description: str | None
    profileIconId: int | None


class SuggestionsData(BaseModel):
    suggestions: list[Suggestion]
