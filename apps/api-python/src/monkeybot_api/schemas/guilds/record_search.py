"""Record search parameters, canonical cursors, and public response projection."""

import base64
import json
import re
from dataclasses import dataclass
from datetime import UTC, datetime, timedelta
from typing import Literal
from uuid import UUID

from pydantic import BaseModel, ConfigDict
from pydantic.alias_generators import to_camel

from monkeybot_api.core.errors import ApiError
from monkeybot_api.schemas.guilds.record_profile import ProfileData, validated_query

Lane = Literal["TOP", "JUNGLE", "MID", "BOTTOM", "SUPPORT"]
Kind = Literal["SERIES", "TOURNAMENT"]
EPOCH = datetime(1970, 1, 1, tzinfo=UTC)
POSTGRES_EPOCH = datetime(2000, 1, 1, tzinfo=UTC)
POSTGRES_EPOCH_NANOS = 946684800_000000000


@dataclass(frozen=True)
class Cursor:
    played_at_nanos: int
    id: UUID
    kind: Kind

    @property
    def database_timestamp(self) -> datetime:
        # SQLx truncates the signed duration from PostgreSQL's epoch toward zero.
        delta = self.played_at_nanos - POSTGRES_EPOCH_NANOS
        micros = (abs(delta) // 1000) * (-1 if delta < 0 else 1)
        return POSTGRES_EPOCH + timedelta(microseconds=micros)

    def encode(self) -> str:
        if not -(2**63) <= self.played_at_nanos < 2**63:
            raise ValueError("record cursor timestamp exceeds i64")
        payload = json.dumps(
            {"playedAtNanos": self.played_at_nanos, "id": str(self.id), "kind": self.kind},
            separators=(",", ":"),
        ).encode()
        return base64.urlsafe_b64encode(payload).rstrip(b"=").decode()

    @classmethod
    def from_head(cls, head: dict[str, object]) -> Cursor:
        played_at = head["played_at"]
        identifier, kind = head["id"], head["kind"]
        if not isinstance(played_at, datetime) or not isinstance(identifier, UUID):
            raise ValueError("invalid record cursor identity")
        if kind not in ("SERIES", "TOURNAMENT"):
            raise ValueError("invalid record cursor kind")
        delta = played_at - EPOCH
        nanos = ((delta.days * 86400 + delta.seconds) * 1_000_000 + delta.microseconds) * 1000
        return cls(nanos, identifier, "SERIES" if kind == "SERIES" else "TOURNAMENT")


def decode_cursor(value: str) -> Cursor:
    try:
        if not value or len(value.encode()) > 512 or not re.fullmatch(r"[A-Za-z0-9_-]+", value):
            raise ValueError("invalid base64")
        payload = json.loads(
            base64.b64decode(value + "=" * (-len(value) % 4), altchars=b"-_", validate=True)
        )
        if not isinstance(payload, dict) or set(payload) != {"playedAtNanos", "id", "kind"}:
            raise ValueError("invalid cursor fields")
        nanos, kind = payload["playedAtNanos"], payload["kind"]
        if type(nanos) is not int or kind not in ("SERIES", "TOURNAMENT"):
            raise ValueError("invalid cursor values")
        cursor = Cursor(nanos, UUID(payload["id"]), kind)
        if cursor.encode() != value:
            raise ValueError("noncanonical cursor")
        return cursor
    except ValueError, TypeError, AttributeError:
        raise ApiError(400, "invalid_cursor", "cursor 형식이 올바르지 않습니다.") from None


class SearchQuery(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid")
    q: str
    seasonId: str | None = None
    cursor: str | None = None
    limit: str | None = None

    def parameters(self) -> tuple[str, UUID | None, Cursor | None, int]:
        query = validated_query(self.q)
        season = None
        if self.seasonId is not None:
            canonical = (
                r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}"
            )
            if (
                re.fullmatch(
                    rf"(?:[0-9a-fA-F]{{32}}|{canonical}|\{{{canonical}\}}|urn:uuid:{canonical})",
                    self.seasonId,
                )
                is None
            ):
                raise ApiError(400, "invalid_season_id", "seasonId 형식이 올바르지 않습니다.")
            season = UUID(self.seasonId)
        limit = 20
        if self.limit is not None:
            digits = self.limit.removeprefix("+").lstrip("0") or "0"
            if (
                re.fullmatch(r"\+?[0-9]+", self.limit) is None
                or len(digits) > 2
                or not 1 <= int(digits) <= 50
            ):
                raise ApiError(400, "invalid_limit", "limit은 1 이상 50 이하이어야 합니다.")
            limit = int(digits)
        return query, season, decode_cursor(self.cursor) if self.cursor is not None else None, limit


class SnapshotModel(BaseModel):
    model_config = ConfigDict(
        strict=True,
        extra="ignore",
        allow_inf_nan=False,
        alias_generator=to_camel,
        populate_by_name=True,
    )


class Season(SnapshotModel):
    id: str
    name: str
    status: Literal["ACTIVE", "CLOSED"]
    started_at: str
    ended_at: str | None


class Summary(SnapshotModel):
    games: int
    wins: int
    losses: int


class LaneMmr(SnapshotModel):
    lane: Lane
    mmr: float
    games: int
    wins: int


class ChampionRecord(Summary):
    champion_id: str
    lane: Lane


class Lineup(SnapshotModel):
    display_name: str
    lane: Lane
    champion_id: str | None
    is_target: bool


class Target(SnapshotModel):
    side: Literal["BLUE", "RED"]
    lane: Lane
    champion_id: str | None
    mmr_before: float | None
    mmr_after: float | None


class Game(SnapshotModel):
    id: str
    game_number: int
    played_at: str
    winning_side: Literal["BLUE", "RED"]
    target: Target
    blue_team: list[Lineup]
    red_team: list[Lineup]
    blue_bans: list[str]
    red_bans: list[str]


class SeriesDetails(SnapshotModel):
    id: str
    title: str
    played_at: str
    result: Literal["WIN", "LOSS", "DRAW"]
    opponent_name: str | None
    stage: Literal["SEMIFINAL_A", "SEMIFINAL_B", "FINAL"] | None
    wins: int
    losses: int
    mmr_delta: float | None
    games: list[Game]


class Series(SeriesDetails):
    kind: Literal["SERIES"]
    source: Literal["REGULAR", "AUCTION"]
    participant_count: Literal[10]
    team_name: str | None


class Tournament(SnapshotModel):
    kind: Literal["TOURNAMENT"]
    id: str
    title: str
    participant_count: Literal[20]
    played_at: str
    team_name: str | None
    spent_points: int | None
    placement: int | None
    wins: int
    losses: int
    mmr_delta: float | None
    roster: list[Lineup]
    series: list[SeriesDetails]


class SearchData(ProfileData):
    model_config = ConfigDict(
        strict=True, extra="forbid", alias_generator=to_camel, populate_by_name=True
    )
    seasons: list[Season]
    selected_season_id: str | None
    summary: Summary
    lane_mmr: list[LaneMmr]
    champion_records: list[ChampionRecord]
    records: list[Series | Tournament]
    next_cursor: str | None
