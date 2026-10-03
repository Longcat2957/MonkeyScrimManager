from typing import Any, Literal

from pydantic import BaseModel, ConfigDict, Field


class FullSnapshotSource(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)

    guildId: str
    guildName: str


class FullSnapshotArchive(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)

    format: Literal["MONKEYBOT_DATA_TRANSFER"]
    kind: Literal["GUILD_FULL_SNAPSHOT"]
    schemaVersion: Literal[1]
    exportedAt: str
    source: FullSnapshotSource
    tables: dict[str, list[dict[str, Any]]]


class FullDataSummary(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)

    totalRows: int = Field(ge=0)
    users: int = Field(ge=0)
    members: int = Field(ge=0)
    seasons: int = Field(ge=0)
    recruitments: int = Field(ge=0)
    series: int = Field(ge=0)
    games: int = Field(ge=0)
    auctionTournaments: int = Field(ge=0)
    auctionMatches: int = Field(ge=0)
    mmrLedgerEntries: int = Field(ge=0)
    auditEntries: int = Field(ge=0)


class FullDataExportData(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)

    archive: FullSnapshotArchive
    fileName: str
    summary: FullDataSummary


class FullDataPreviewRequest(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)

    archive: dict[str, Any]


class FullDataIssue(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)

    reason: str
    message: str
    table: str | None
    rowCount: int = Field(ge=0)


class FullDataPreview(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)

    importId: str
    digest: str
    targetDigest: str
    expiresAt: str
    source: FullSnapshotSource
    summary: FullDataSummary
    conflicts: list[FullDataIssue]
    remapsSourceGuild: bool
    usesTargetActiveSeason: bool
    rebasesDisplayNumbers: bool


class FullDataApplyCommand(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)

    expectedDigest: str
    expectedTargetDigest: str


class FullDataApplyResult(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)

    importId: str
    status: Literal["APPLIED"]
    appliedAt: str
    summary: FullDataSummary
    remappedSourceGuild: bool
    usedTargetActiveSeason: bool
    rebasedDisplayNumbers: bool
