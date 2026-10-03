"""Input normalization, provider routing, and stable service errors."""

from dataclasses import dataclass
from enum import StrEnum
from unicodedata import category


class ErrorKind(StrEnum):
    INVALID_REQUEST = "invalid_request"
    UNAUTHORIZED = "unauthorized"
    NOT_FOUND = "riot_account_not_found"
    RATE_LIMITED = "riot_rate_limited"
    TIMEOUT = "riot_timeout"
    UNAVAILABLE = "riot_unavailable"
    CREDENTIALS_REJECTED = "riot_credentials_rejected"
    BAD_GATEWAY = "riot_bad_gateway"


@dataclass(slots=True)
class RiotError(Exception):
    kind: ErrorKind
    retry_after_seconds: int | None = None
    stale_eligible: bool = False

    def __str__(self) -> str:
        return self.kind.value

    @property
    def can_use_stale(self) -> bool:
        return self.stale_eligible or self.kind in {
            ErrorKind.RATE_LIMITED,
            ErrorKind.TIMEOUT,
            ErrorKind.UNAVAILABLE,
        }


_PLATFORMS: dict[str, tuple[str, str]] = {
    "BR": ("BR1", "AMERICAS"),
    "BR1": ("BR1", "AMERICAS"),
    "EUNE": ("EUN1", "EUROPE"),
    "EUN1": ("EUN1", "EUROPE"),
    "EUW": ("EUW1", "EUROPE"),
    "EUW1": ("EUW1", "EUROPE"),
    "JP": ("JP1", "ASIA"),
    "JP1": ("JP1", "ASIA"),
    "KR": ("KR", "ASIA"),
    "LAN": ("LA1", "AMERICAS"),
    "LA1": ("LA1", "AMERICAS"),
    "LAS": ("LA2", "AMERICAS"),
    "LA2": ("LA2", "AMERICAS"),
    "NA": ("NA1", "AMERICAS"),
    "NA1": ("NA1", "AMERICAS"),
    "OCE": ("OC1", "AMERICAS"),
    "OC1": ("OC1", "AMERICAS"),
    "PH": ("PH2", "SEA"),
    "PH2": ("PH2", "SEA"),
    "RU": ("RU", "EUROPE"),
    "SG": ("SG2", "SEA"),
    "SG2": ("SG2", "SEA"),
    "TH": ("TH2", "SEA"),
    "TH2": ("TH2", "SEA"),
    "TR": ("TR1", "EUROPE"),
    "TR1": ("TR1", "EUROPE"),
    "TW": ("TW2", "SEA"),
    "TW2": ("TW2", "SEA"),
    "VN": ("VN2", "SEA"),
    "VN2": ("VN2", "SEA"),
}


@dataclass(frozen=True, slots=True)
class Platform:
    code: str
    routing: str

    @classmethod
    def parse(cls, value: str) -> Platform:
        result = _PLATFORMS.get(value.strip().upper())
        if result is None:
            raise RiotError(ErrorKind.INVALID_REQUEST)
        return cls(*result)

    @property
    def host(self) -> str:
        return f"{self.code.lower()}.api.riotgames.com"

    @property
    def routing_host(self) -> str:
        return f"{self.routing.lower()}.api.riotgames.com"


@dataclass(frozen=True, slots=True)
class RiotId:
    game_name: str
    tag_line: str
    platform: Platform

    @classmethod
    def parse(cls, game_name: str, tag_line: str, platform: str) -> RiotId:
        return cls(_part(game_name, 64), _part(tag_line, 16), Platform.parse(platform))


def _part(value: str, max_chars: int) -> str:
    trimmed = value.strip()
    if not trimmed or len(trimmed) > max_chars or any(category(c) == "Cc" for c in trimmed):
        raise RiotError(ErrorKind.INVALID_REQUEST)
    return trimmed


def validate_puuid(value: str) -> str:
    trimmed = value.strip()
    if (
        not trimmed
        or len(trimmed.encode()) > 128
        or any(c.isspace() or category(c) == "Cc" for c in trimmed)
    ):
        raise RiotError(ErrorKind.INVALID_REQUEST)
    return trimmed
