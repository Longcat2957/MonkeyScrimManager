"""Service-admin request and stored reset-response contracts."""

from pydantic import BaseModel, ConfigDict


class ResetCommand(BaseModel):
    model_config = ConfigDict(strict=True, extra="ignore")

    confirmation: str


class ResetCounts(BaseModel):
    model_config = ConfigDict(strict=True, extra="ignore")

    totalRows: int
    seasons: int
    recruitments: int
    series: int
    games: int
    auctionRecruitments: int
    auctionTournaments: int
    mmrRows: int
    importJobs: int


class FreshSeason(BaseModel):
    model_config = ConfigDict(strict=True, extra="ignore")

    id: str
    name: str


class ResetResult(BaseModel):
    model_config = ConfigDict(strict=True, extra="ignore")

    guildId: str
    displayName: str
    resetAt: str
    deleted: ResetCounts
    freshSeason: FreshSeason
