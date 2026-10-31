"""Statistics query values and public snapshot models."""

import re
from typing import Annotated, Literal
from uuid import UUID

from pydantic import AfterValidator, BaseModel, ConfigDict, Field, model_validator
from pydantic.alias_generators import to_camel

from monkeybot_api.core.errors import ApiError
from monkeybot_api.schemas.common import user_id

Scope = Literal["ALL", "REGULAR", "AUCTION"]
Count = Annotated[int, Field(ge=0)]
UserId = Annotated[str, AfterValidator(user_id)]


def nonblank(value: str) -> str:
    if not value.strip():
        raise ValueError("stored statistics text is empty")
    return value


Name = Annotated[str, AfterValidator(nonblank)]


class StatisticsQuery(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid")
    seasonId: str | None = None
    scope: str | None = None

    def parameters(self) -> tuple[UUID | None, Scope]:
        try:
            season = None
            if self.seasonId is not None:
                canonical = (
                    r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-"
                    r"[0-9a-fA-F]{4}-[0-9a-fA-F]{12}"
                )
                if (
                    re.fullmatch(
                        rf"(?:[0-9a-fA-F]{{32}}|{canonical}|\{{{canonical}\}}|urn:uuid:{canonical})",
                        self.seasonId,
                    )
                    is None
                ):
                    raise ValueError("invalid UUID format")
                season = UUID(self.seasonId)
        except ValueError:
            raise ApiError(400, "invalid_season_id", "seasonId 형식이 올바르지 않습니다.") from None
        match self.scope:
            case None | "ALL":
                return season, "ALL"
            case "REGULAR":
                return season, "REGULAR"
            case "AUCTION":
                return season, "AUCTION"
            case _:
                raise ApiError(
                    400,
                    "invalid_scope",
                    "scope은 ALL, REGULAR, AUCTION 중 하나여야 합니다.",
                    {"allowed": ["ALL", "REGULAR", "AUCTION"]},
                )


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
    name: Name
    status: Literal["ACTIVE", "CLOSED"]


class Coverage(SnapshotModel):
    duration_games: Count = 0
    participant_stat_games: Count = 0
    champion_pick_games: Count = 0
    champion_ban_games: Count = 0


class Overview(SnapshotModel):
    games: Count = 0
    regular_games: Count = 0
    auction_games: Count = 0
    unique_players: Count = 0
    blue_wins: Count = 0
    red_wins: Count = 0
    average_duration_seconds: Annotated[float, Field(gt=0)] | None = None
    coverage: Coverage = Field(default_factory=Coverage)

    @model_validator(mode="after")
    def consistent_counts(self) -> Overview:
        if (
            self.games != self.regular_games + self.auction_games
            or self.games != self.blue_wins + self.red_wins
            or any(value > self.games for value in self.coverage.model_dump().values())
        ):
            raise ValueError("stored statistics overview is inconsistent")
        return self


class Record(SnapshotModel):
    games: Count
    wins: Count
    losses: Count


class Player(Record):
    user_id: UserId
    name: Name


class Players(SnapshotModel):
    minimum_win_rate_games: int = 3
    most_active: list[Player] = Field(default_factory=list)
    best_win_rates: list[Player] = Field(default_factory=list)


class Duo(Record):
    user_a_id: UserId
    user_a_name: Name
    user_b_id: UserId
    user_b_name: Name

    @model_validator(mode="after")
    def canonical_pair(self) -> Duo:
        if self.user_a_id >= self.user_b_id:
            raise ValueError("stored statistics pair is not canonical")
        return self


class Relationships(SnapshotModel):
    best_duos: list[Duo] = Field(default_factory=list)


class ChampionPick(Record):
    champion_id: Name
    lane: Literal["TOP", "JUNGLE", "MID", "BOTTOM", "SUPPORT"]


class ChampionBan(SnapshotModel):
    champion_id: Name
    bans: Count


class Champions(SnapshotModel):
    picks: list[ChampionPick] = Field(default_factory=list)
    bans: list[ChampionBan] = Field(default_factory=list)


class Captain(SnapshotModel):
    user_id: UserId
    name: Name
    teams_captained: Annotated[int, Field(ge=1)]
    titles: Count
    final_appearances: Count
    match_wins: Count
    match_losses: Count
    game_wins: Count
    game_losses: Count
    total_spent_points: Count

    @model_validator(mode="after")
    def consistent_totals(self) -> Captain:
        if (
            self.titles > self.teams_captained
            or self.final_appearances > self.match_wins + self.match_losses
        ):
            raise ValueError("stored statistics captain totals are inconsistent")
        return self


class Bid(Record):
    tournament_id: str
    tournament_title: Name
    team_name: Name
    user_id: UserId
    name: Name
    points: Count
    won_tournament: bool


class Auction(SnapshotModel):
    completed_tournaments: Count = 0
    captains: list[Captain] = Field(default_factory=list)
    top_bids: list[Bid] = Field(default_factory=list)


class StatisticsSnapshot(SnapshotModel):
    seasons: list[Season]
    selected_season_id: str | None
    scope: Scope
    overview: Overview = Field(default_factory=Overview)
    players: Players = Field(default_factory=Players)
    relationships: Relationships = Field(default_factory=Relationships)
    champions: Champions = Field(default_factory=Champions)
    auction: Auction = Field(default_factory=Auction)
