"""Legacy Discord member MMR archive export, preview, and atomic apply."""

import hashlib
import json
import logging
import math
import uuid
from datetime import UTC, datetime, timedelta
from itertools import groupby
from typing import Any

from pydantic import ValidationError
from pydantic_core import to_json
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.legacy_json import parse_float, parse_int
from monkeybot_api.core.time import timestamp
from monkeybot_api.db.database import Database
from monkeybot_api.db.transactions import mutation, repeatable_read
from monkeybot_api.repositories.effects import complete
from monkeybot_api.repositories.guilds import legacy_data_transfer as repository
from monkeybot_api.repositories.idempotency import acquire
from monkeybot_api.repositories.permissions import authorize
from monkeybot_api.schemas.guilds.legacy_data_transfer import (
    LANES,
    ApplyCommand,
    ImportApplyResult,
    SeedArchive,
    archive_valid,
    valid_text,
)

logger = logging.getLogger(__name__)
APPLY_ROUTE = "/api/guilds/{guildId}/data-imports/{importId}/apply"
MMR_EPSILON = 0.000_001


def invariant(request_id: str, reason: str) -> ApiError:
    logger.error(reason, extra={"request_id": request_id})
    return ApiError.from_status(500)


def invalid_archive() -> ApiError:
    return ApiError(
        400,
        "invalid_archive",
        "지원되는 MonkeyBot MMR 백업 파일 형식이 아닙니다.",
        {"maxMembers": 1000},
    )


def parse_archive(value: Any) -> SeedArchive:
    try:
        archive = SeedArchive.model_validate(value)
    except ValidationError:
        raise invalid_archive() from None
    if not archive_valid(archive):
        raise invalid_archive()
    return archive


def canonical_json(value: Any) -> str:
    # pydantic-core uses the same Ryu float spelling as serde_json (for example 1e-7).
    return to_json(value).decode("utf-8")


def archive_digest(archive: SeedArchive) -> bytes:
    return hashlib.sha256(canonical_json(archive.model_dump(mode="json")).encode()).digest()


def target_mmr_digest(season: uuid.UUID, entries: list[tuple[str, str, float]]) -> bytes:
    snapshot = {
        "seasonId": str(season),
        "entries": [
            {"userId": user, "lane": lane, "mmr": mmr}
            for user, lane, mmr in sorted(entries, key=lambda entry: (entry[0], entry[1]))
        ],
    }
    return hashlib.sha256(canonical_json(snapshot).encode()).digest()


def checked_delta(before: float, after: float) -> float:
    delta = after - before
    if not math.isfinite(delta):
        raise ApiError(
            422,
            "invalid_mmr_transfer",
            "백업 파일의 MMR 값을 대상 길드에 적용할 수 없습니다.",
        )
    return delta


async def operator(c: AsyncConnection, guild: str, actor: str) -> None:
    try:
        await authorize(c, guild, actor)
    except ApiError as exc:
        if exc.code == "not_found":
            raise ApiError(404, "not_found", "요청한 길드를 찾을 수 없습니다.") from None
        raise


async def active_season(c: AsyncConnection, guild: str, *, lock: bool = False) -> dict[str, Any]:
    row = await repository.season(c, guild, lock=lock)
    if row is None:
        raise ApiError(409, "no_active_season", "길드의 활성 시즌을 먼저 설정해야 합니다.")
    return row


async def export_archive(
    database: Database, guild: str, actor: str, request_id: str
) -> dict[str, Any]:
    now = datetime.now(UTC)
    async with database.connection(transaction=True) as c:
        await repeatable_read(c)
        await operator(c, guild, actor)
        season = await active_season(c, guild)
        rows = await repository.export_rows(c, guild, season["id"])
        members: list[dict[str, Any]] = []
        for user, group in groupby(rows, lambda row: row["user_id"]):
            entries = list(group)
            if len(members) == 1_000:
                raise ApiError(
                    422,
                    "archive_member_limit_exceeded",
                    "활성 Discord 멤버가 1,000명을 초과하여 MMR 백업을 만들 수 없습니다.",
                    {"maxMembers": 1000},
                )
            members.append(
                {
                    "discordId": user,
                    "displayName": entries[0]["display_name"],
                    "lanes": [{"lane": row["lane"], "mmr": row["mmr"]} for row in entries],
                }
            )
        guild_name = await repository.guild_name(c, guild)
        if guild_name is None or not valid_text(guild_name):
            raise invariant(request_id, "legacy_export_guild_name_invalid")
        data = {
            "archive": {
                "format": "MONKEYBOT_DATA_TRANSFER",
                "kind": "MEMBER_SEASON_MMR_SEED",
                "schemaVersion": 1,
                "exportedAt": timestamp(now),
                "source": {
                    "guildId": guild,
                    "guildName": guild_name,
                    "season": {"id": str(season["id"]), "name": season["name"]},
                },
                "members": members,
            },
            "fileName": f"monkeybot-mmr-{guild}-{season['id']}.json",
        }
        try:
            archive = SeedArchive.model_validate(data["archive"])
        except ValidationError:
            raise invariant(request_id, "legacy_export_archive_invalid") from None
        if not archive_valid(archive):
            raise invariant(request_id, "legacy_export_archive_invalid")
        await repository.audit(
            c,
            {
                "guild": guild,
                "actor_id": actor,
                "request_id": request_id,
                "audit_id": uuid.uuid7(),
                "now": now,
            },
            "DATA_TRANSFER_EXPORTED",
            "MEMBER_SEASON_MMR_SEED",
            str(season["id"]),
            {"archiveMembers": len(members), "seasonId": str(season["id"])},
            {"schemaVersion": 1, "kind": "MEMBER_SEASON_MMR_SEED"},
        )
        return data


async def analyze(
    c: AsyncConnection, guild: str, season: uuid.UUID, archive: SeedArchive
) -> tuple[dict[str, int], list[dict[str, str]], bytes]:
    ids = [member.discordId for member in archive.members]
    active = await repository.active_members(c, guild, ids)
    current = await repository.current_mmr(c, guild, season, ids)
    conflicts = [
        {
            "reason": "member_not_active",
            "message": "대상 길드에서 활성 Discord 멤버를 찾을 수 없습니다.",
            "discordId": member.discordId,
            "displayName": member.displayName,
        }
        for member in archive.members
        if member.discordId not in active
    ]
    entries: list[tuple[str, str, float]] = []
    changes = 0
    for member in archive.members:
        if member.discordId not in active:
            continue
        desired: dict[str, float] = {entry.lane: entry.mmr for entry in member.lanes}
        for lane in LANES:
            before = current.get((member.discordId, lane), 1500.0)
            entries.append((member.discordId, lane, before))
            if abs(checked_delta(before, desired[lane])) > MMR_EPSILON:
                changes += 1
    summary = {
        "archiveMembers": len(archive.members),
        "matchedMembers": len(archive.members) - len(conflicts),
        "laneEntries": len(archive.members) * len(LANES),
        "changes": changes,
    }
    return summary, conflicts, target_mmr_digest(season, entries)


async def preview_archive(
    database: Database, guild: str, actor: str, request_id: str, archive_value: Any
) -> dict[str, Any]:
    archive = parse_archive(archive_value)
    digest = archive_digest(archive)
    now = datetime.now(UTC)
    expires = now + timedelta(minutes=15)
    import_id = uuid.uuid7()
    async with database.connection(transaction=True) as c:
        await repeatable_read(c)
        await operator(c, guild, actor)
        season = await active_season(c, guild)
        revision = await repository.members_revision(c, guild)
        if revision is None:
            raise ApiError(404, "not_found", "요청한 길드를 찾을 수 없습니다.")
        summary, conflicts, mmr_digest = await analyze(c, guild, season["id"], archive)
        await repository.create_preview(
            c,
            {
                "guild": guild,
                "import_id": import_id,
                "actor_id": actor,
                "digest": digest,
                "archive": canonical_json(archive.model_dump(mode="json")),
                "source_guild": archive.source.guildId,
                "source_season": uuid.UUID(archive.source.season.id),
                "season": season["id"],
                "revision": revision,
                "mmr_digest": mmr_digest,
                "summary": canonical_json(summary),
                "conflict_count": len(conflicts),
                "request_id": request_id,
                "now": now,
                "expires": expires,
            },
        )
        await repository.audit(
            c,
            {
                "guild": guild,
                "actor_id": actor,
                "request_id": request_id,
                "audit_id": uuid.uuid7(),
                "now": now,
            },
            "DATA_IMPORT_PREVIEWED",
            "DATA_IMPORT",
            str(import_id),
            {
                "digest": digest.hex(),
                "sourceGuildId": archive.source.guildId,
                "sourceSeasonId": archive.source.season.id,
            },
            {
                "archiveMembers": summary["archiveMembers"],
                "matchedMembers": summary["matchedMembers"],
                "changes": summary["changes"],
                "conflictCount": len(conflicts),
            },
        )
        return {
            "importId": str(import_id),
            "digest": digest.hex(),
            "expiresAt": timestamp(expires),
            "source": archive.source.model_dump(mode="json"),
            "summary": summary,
            "conflicts": conflicts,
            "targetRevision": revision,
        }


async def apply_archive(
    database: Database,
    guild: str,
    actor: str,
    request_id: str,
    import_id: uuid.UUID,
    key: str,
    command: ApplyCommand,
) -> dict[str, Any]:
    if (
        command.expectedTargetRevision < 0
        or len(command.expectedDigest) != 64
        or any(character not in "0123456789abcdefABCDEF" for character in command.expectedDigest)
    ):
        raise ApiError(
            422, "invalid_import_apply_command", "가져오기 적용 확인 값이 올바르지 않습니다."
        )
    try:
        digest = bytes.fromhex(command.expectedDigest)
    except ValueError:
        raise ApiError(
            422, "invalid_import_apply_command", "가져오기 적용 확인 값이 올바르지 않습니다."
        ) from None
    payload = [guild, str(import_id), command.model_dump(mode="json")]
    now = datetime.now(UTC)
    p = {
        "guild": guild,
        "actor_id": actor,
        "principal_type": "USER",
        "principal": actor,
        "method": "POST",
        "route": APPLY_ROUTE,
        "key": key,
        "hash": hashlib.sha256(canonical_json(payload).encode()).digest(),
        "now": now,
        "expires": now + timedelta(hours=24),
        "idem_id": uuid.uuid7(),
        "audit_id": uuid.uuid7(),
        "request_id": request_id,
        "import_id": import_id,
    }
    return await mutation(database, lambda c: apply_attempt(c, p, command, digest))


async def apply_attempt(
    c: AsyncConnection, p: dict[str, Any], command: ApplyCommand, digest: bytes
) -> dict[str, Any]:
    p["now"] = datetime.now(UTC)
    await operator(c, p["guild"], p["actor_id"])
    try:
        replay = await acquire(c, p, False)
    except ApiError as exc:
        if exc.code == "idempotency_in_progress":
            raise ApiError(409, exc.code, "같은 가져오기 요청을 처리하고 있습니다.") from None
        raise
    if replay is not None:
        try:
            return ImportApplyResult.model_validate(replay).model_dump(mode="json")
        except ValidationError:
            raise invariant(p["request_id"], "legacy_import_replay_invalid") from None
    stored = await repository.lock_import(c, p["guild"], p["import_id"])
    if stored is None:
        raise ApiError(
            404,
            "preview_not_found",
            "가져오기 검토 결과를 찾을 수 없습니다. 파일을 다시 검증해 주세요.",
        )
    if stored["actor_id"] != p["actor_id"]:
        raise ApiError(
            403, "preview_actor_mismatch", "가져오기 검토를 만든 운영자만 적용할 수 있습니다."
        )
    if stored["status"] != "PREVIEWED":
        raise ApiError(
            409, "preview_already_applied", "이 가져오기 검토 결과는 이미 적용되었습니다."
        )
    if stored["expires_at"] <= p["now"]:
        raise ApiError(
            409,
            "preview_expired",
            "가져오기 검토 결과가 만료되었습니다. 파일을 다시 검증해 주세요.",
        )
    if digest != bytes(stored["archive_digest"]):
        raise ApiError(
            409, "preview_digest_mismatch", "검토한 백업 파일과 적용 요청이 일치하지 않습니다."
        )
    if stored["conflict_count"]:
        raise ApiError(
            409, "preview_conflicts", "대상 길드 멤버 상태가 검토 결과와 달라져 가져올 수 없습니다."
        )
    if stored["archive_payload"] is None:
        raise invariant(p["request_id"], "legacy_import_archive_missing")
    try:
        archive_value = json.loads(
            stored["archive_payload"], parse_float=parse_float, parse_int=parse_int
        )
        archive = parse_archive(archive_value)
    except ApiError, ValueError:
        raise invariant(p["request_id"], "legacy_import_archive_invalid") from None
    if archive_digest(archive) != bytes(stored["archive_digest"]):
        raise ApiError(
            409, "preview_digest_mismatch", "검토한 백업 파일과 적용 요청이 일치하지 않습니다."
        )
    revision = await repository.lock_members_state(c, p)
    if (
        command.expectedTargetRevision != stored["target_members_revision"]
        or revision != stored["target_members_revision"]
    ):
        raise ApiError(
            409,
            "revision_conflict",
            "길드 멤버 MMR 상태가 변경되었습니다. 파일을 다시 검증해 주세요.",
            {"expectedRevision": command.expectedTargetRevision, "currentRevision": revision},
        )
    season = await active_season(c, p["guild"], lock=True)
    if season["id"] != stored["target_season_id"]:
        raise ApiError(
            409,
            "target_season_changed",
            "대상 길드의 활성 시즌이 변경되었습니다. 파일을 다시 검증해 주세요.",
        )
    if await repository.has_active_workflow(c, p["guild"]):
        raise ApiError(
            409,
            "active_workflow_conflict",
            "진행 중인 모집 또는 내전이 있어 MMR 백업을 적용할 수 없습니다.",
        )
    summary, conflicts, _ = await analyze(c, p["guild"], season["id"], archive)
    if conflicts:
        raise ApiError(
            409, "preview_conflicts", "대상 길드 멤버 상태가 검토 결과와 달라져 가져올 수 없습니다."
        )
    if summary["changes"] == 0:
        raise ApiError(409, "no_import_changes", "현재 길드 MMR과 동일하여 적용할 변경이 없습니다.")
    if summary != stored["summary"]:
        raise ApiError(
            409,
            "revision_conflict",
            "길드 멤버 MMR 상태가 변경되었습니다. 파일을 다시 검증해 주세요.",
            {"expectedRevision": stored["target_members_revision"], "currentRevision": revision},
        )
    desired = {
        (member.discordId, entry.lane): entry.mmr
        for member in archive.members
        for entry in member.lanes
    }
    p["season"] = season["id"]
    locked = await repository.lock_mmr_rows(c, p, list(desired))
    if len(locked) != len(desired):
        raise invariant(p["request_id"], "legacy_import_mmr_rows_missing")
    entries = [(row["user_id"], row["lane"], row["mmr"]) for row in locked]
    if target_mmr_digest(season["id"], entries) != bytes(stored["target_mmr_digest"]):
        raise ApiError(
            409, "mmr_state_changed", "대상 길드의 MMR이 변경되었습니다. 파일을 다시 검증해 주세요."
        )
    changes = 0
    for row in locked:
        after = desired[(row["user_id"], row["lane"])]
        delta = checked_delta(row["mmr"], after)
        if abs(delta) <= MMR_EPSILON:
            continue
        if row["revision"] == 2**63 - 1:
            raise invariant(p["request_id"], "legacy_import_lane_revision_overflow")
        await repository.persist_mmr_change(
            c,
            {
                **p,
                "ledger_id": uuid.uuid7(),
                "user": row["user_id"],
                "lane": row["lane"],
                "lane_revision": row["revision"] + 1,
                "before": row["mmr"],
                "after": after,
                "delta": delta,
            },
        )
        changes += 1
    if changes != summary["changes"] or revision == 2**63 - 1:
        raise invariant(p["request_id"], "legacy_import_change_count_or_revision_invalid")
    p["revision"] = revision
    p["next_revision"] = revision + 1
    await repository.increment_members_revision(c, p)
    await repository.mark_applied(c, p)
    await repository.audit(
        c,
        p,
        "DATA_IMPORT_APPLIED",
        "DATA_IMPORT",
        str(p["import_id"]),
        {"digest": command.expectedDigest, "targetRevision": revision},
        {"changes": changes, "targetRevision": revision + 1},
    )
    result = {
        "importId": str(p["import_id"]),
        "status": "APPLIED",
        "appliedAt": timestamp(p["now"]),
        "summary": summary,
        "targetRevision": revision + 1,
    }
    await complete(c, p, result, 200)
    return result
