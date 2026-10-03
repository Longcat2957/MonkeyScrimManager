"""The v1 member and season MMR seed archive contract."""

import math
import re
import uuid
from datetime import datetime
from typing import Any, Literal

from pydantic import BaseModel, ConfigDict, Field, field_validator

from monkeybot_api.core.config import is_snowflake

LANES = ("TOP", "JUNGLE", "MID", "BOTTOM", "SUPPORT")


class TransferModel(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)


class ArchiveSeason(TransferModel):
    id: str
    name: str

    @field_validator("id")
    @classmethod
    def uuid_id(cls, value: str) -> str:
        return str(uuid.UUID(value))


class ArchiveSource(TransferModel):
    guildId: str
    guildName: str
    season: ArchiveSeason

    @field_validator("guildId")
    @classmethod
    def guild_id(cls, value: str) -> str:
        if not is_snowflake(value):
            raise ValueError("invalid guild id")
        return value


class ArchiveLaneMmr(TransferModel):
    lane: Literal["TOP", "JUNGLE", "MID", "BOTTOM", "SUPPORT"]
    mmr: float


class ArchiveMember(TransferModel):
    discordId: str
    displayName: str
    lanes: list[ArchiveLaneMmr]

    @field_validator("discordId")
    @classmethod
    def discord_id(cls, value: str) -> str:
        if not is_snowflake(value):
            raise ValueError("invalid Discord ID")
        return value


class SeedArchive(TransferModel):
    format: str
    kind: str
    schemaVersion: int
    exportedAt: str
    source: ArchiveSource
    members: list[ArchiveMember]

    @field_validator("exportedAt")
    @classmethod
    def exported_at(cls, value: str) -> str:
        match = re.fullmatch(
            r"(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})(?:\.(\d{1,9}))?(Z|[+-]\d{2}:\d{2})",
            value,
        )
        if match is None:
            raise ValueError("invalid RFC3339 timestamp")
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
        if parsed.tzinfo is None:
            raise ValueError("timestamp requires an offset")
        fraction = (match.group(2) or "").rstrip("0")
        offset = match.group(3)
        if offset in {"+00:00", "-00:00"}:
            offset = "Z"
        return f"{match.group(1)}{'.' + fraction if fraction else ''}{offset}"


class PreviewRequest(TransferModel):
    archive: Any


class ApplyCommand(TransferModel):
    expectedDigest: str
    expectedTargetRevision: int = Field(ge=-(2**63), le=2**63 - 1)


class ImportSummary(TransferModel):
    archiveMembers: int = Field(ge=0)
    matchedMembers: int = Field(ge=0)
    laneEntries: int = Field(ge=0)
    changes: int = Field(ge=0)


class ImportApplyResult(TransferModel):
    importId: str
    status: Literal["APPLIED"]
    appliedAt: str
    summary: ImportSummary
    targetRevision: int = Field(ge=0)


def archive_valid(archive: SeedArchive) -> bool:
    if (
        archive.format != "MONKEYBOT_DATA_TRANSFER"
        or archive.kind != "MEMBER_SEASON_MMR_SEED"
        or archive.schemaVersion != 1
        or len(archive.members) > 1_000
        or not valid_text(archive.source.guildName)
        or not valid_text(archive.source.season.name)
    ):
        return False
    ids: set[str] = set()
    for member in archive.members:
        if member.discordId in ids or not valid_text(member.displayName):
            return False
        ids.add(member.discordId)
        if len(member.lanes) != len(LANES) or {entry.lane for entry in member.lanes} != set(LANES):
            return False
        if any(not math.isfinite(entry.mmr) for entry in member.lanes):
            return False
    return True


def valid_text(value: str) -> bool:
    return 1 <= len(value.strip()) <= 100
