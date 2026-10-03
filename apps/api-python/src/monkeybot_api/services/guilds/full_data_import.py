"""Preview and atomically apply complete guild snapshots."""

import copy
import hashlib
import json
import logging
import re
import uuid
from datetime import UTC, datetime, timedelta
from typing import Any

from pydantic import ValidationError
from pydantic_core import to_json
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.config import is_snowflake
from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.time import timestamp
from monkeybot_api.db.database import Database
from monkeybot_api.db.transactions import mutation, repeatable_read
from monkeybot_api.repositories.effects import audit, complete
from monkeybot_api.repositories.guilds import full_data_import as repository
from monkeybot_api.repositories.idempotency import acquire
from monkeybot_api.repositories.permissions import authorize
from monkeybot_api.schemas.guilds.full_data_transfer import (
    FullDataApplyCommand,
    FullDataApplyResult,
    FullDataPreview,
    FullSnapshotArchive,
)
from monkeybot_api.services.guilds.full_data_transfer import summary, validate_archive

logger = logging.getLogger(__name__)
PREVIEW_TTL = timedelta(minutes=15)
IDEMPOTENCY_TTL = timedelta(hours=24)
APPLY_ROUTE = "/api/guilds/{guildId}/full-data-imports/{importId}/apply"
CROSS_GUILD_DISCORD_LINKS = {
    "recruitments": ("channel_id", "message_id"),
    "series": ("end_card_channel_id", "end_card_message_id"),
    "auction_recruitments": ("channel_id", "message_id"),
    "auction_tournaments": ("end_card_channel_id", "end_card_message_id"),
}
DISPLAY_NAMESPACES = (
    ("recruitments", "RECRUITMENT", "일반 내전 모집 #"),
    ("series", "SERIES", "일반 내전 #"),
    ("auction_recruitments", "AUCTION_RECRUITMENT", "경매 내전 모집 #"),
    ("auction_tournaments", "AUCTION_TOURNAMENT", "경매 내전 #"),
)


def invalid_archive() -> ApiError:
    return ApiError(
        400,
        "invalid_full_archive",
        "지원되는 MonkeyBot 전체 데이터 백업 파일 형식이 아닙니다.",
        {"kind": "GUILD_FULL_SNAPSHOT", "schemaVersion": 1},
    )


def parse_archive(value: Any) -> dict[str, Any]:
    try:
        archive = FullSnapshotArchive.model_validate(value)
        exported_at = datetime.fromisoformat(archive.exportedAt.replace("Z", "+00:00"))
        if exported_at.tzinfo is None or not is_snowflake(archive.source.guildId):
            raise ValueError
    except ValidationError, ValueError, TypeError:
        raise invalid_archive() from None
    validate_archive(archive)
    return archive.model_dump()


def stable_bytes(value: Any, *, sort_keys: bool = False) -> bytes:
    # pydantic-core uses Ryu float notation, matching serde_json's digest bytes.
    return to_json(_sorted_json(value) if sort_keys else value)


def archive_digest(archive: dict[str, Any]) -> bytes:
    # serde_json sorts dynamically keyed maps, while preserving the archive struct's field order.
    ordered = {
        "format": archive["format"],
        "kind": archive["kind"],
        "schemaVersion": archive["schemaVersion"],
        "exportedAt": archive["exportedAt"],
        "source": {
            "guildId": archive["source"]["guildId"],
            "guildName": archive["source"]["guildName"],
        },
        "tables": {
            key: [_sorted_json(row) for row in archive["tables"][key]]
            for key in sorted(archive["tables"])
        },
    }
    return hashlib.sha256(stable_bytes(ordered)).digest()


def _sorted_json(value: Any) -> Any:
    if isinstance(value, dict):
        return {key: _sorted_json(value[key]) for key in sorted(value)}
    if isinstance(value, list):
        return [_sorted_json(item) for item in value]
    return value


def state_digest(state: repository.TargetState) -> bytes:
    value = {
        "members": state.members,
        "counters": state.counters,
        "adminState": state.admin_state,
        "seasons": state.seasons,
        "domainRows": state.domain_rows,
        "activeDomainRows": state.active_domain_rows,
        "targetActiveSeasonId": str(state.active_season_id) if state.active_season_id else None,
        "displayNumberBases": {
            "recruitment": state.display_number_bases["RECRUITMENT"],
            "series": state.display_number_bases["SERIES"],
            "auctionRecruitment": state.display_number_bases["AUCTION_RECRUITMENT"],
            "auctionTournament": state.display_number_bases["AUCTION_TOURNAMENT"],
        },
    }
    return hashlib.sha256(stable_bytes(value, sort_keys=True)).digest()


def _replace_exact(value: Any, source: str, target: str) -> Any:
    if isinstance(value, str):
        return target if value == source else value
    if isinstance(value, list):
        return [_replace_exact(item, source, target) for item in value]
    if isinstance(value, dict):
        return {key: _replace_exact(item, source, target) for key, item in value.items()}
    return value


def map_archive(
    archive: dict[str, Any], guild: str, state: repository.TargetState
) -> tuple[dict[str, Any], bool, bool, bool]:
    remapped = archive["source"]["guildId"] != guild
    mapped = copy.deepcopy(archive)
    if remapped:
        mapped = _replace_exact(mapped, archive["source"]["guildId"], guild)
        for table, fields in CROSS_GUILD_DISCORD_LINKS.items():
            for row in mapped["tables"][table]:
                for field in fields:
                    row[field] = None

    active_seasons = [row for row in mapped["tables"]["seasons"] if row.get("status") == "ACTIVE"]
    if len(active_seasons) > 1:
        raise invalid_archive()
    uses_target_season = False
    if state.active_season_id is not None and active_seasons:
        source_id = active_seasons[0].get("id")
        if not isinstance(source_id, str):
            raise invalid_archive()
        try:
            uuid.UUID(source_id)
        except ValueError:
            raise invalid_archive() from None
        mapped["tables"]["seasons"] = [
            row for row in mapped["tables"]["seasons"] if row.get("id") != source_id
        ]
        mapped["tables"] = _replace_exact(mapped["tables"], source_id, str(state.active_season_id))
        uses_target_season = True

    rebased = False
    for table, key, prefix in DISPLAY_NAMESPACES:
        base = state.display_number_bases[key]
        if base <= 0:
            continue
        maximum = None
        for row in mapped["tables"][table]:
            original = row.get("display_number")
            if not isinstance(original, int) or isinstance(original, bool) or original <= 0:
                raise invalid_archive()
            number = original + base
            if number > 2**63 - 1:
                raise invalid_archive()
            if row.get("title") == f"{prefix}{original}":
                row["title"] = f"{prefix}{number}"
            row["display_number"] = number
            maximum = number if maximum is None else max(maximum, number)
            rebased = True
        if maximum is None:
            continue
        matching = [
            row for row in mapped["tables"]["guild_counters"] if row.get("counter_key") == key
        ]
        if len(matching) > 1 or maximum == 2**63 - 1:
            raise invalid_archive()
        minimum_next = maximum + 1
        if matching:
            source_next = matching[0].get("next_value")
            if (
                not isinstance(source_next, int)
                or isinstance(source_next, bool)
                or source_next <= 0
                or source_next + base > 2**63 - 1
            ):
                raise invalid_archive()
            matching[0]["next_value"] = max(source_next + base, minimum_next)
        else:
            mapped["tables"]["guild_counters"].append(
                {"guild_id": guild, "counter_key": key, "next_value": minimum_next}
            )
    validate_archive(FullSnapshotArchive.model_validate(mapped))
    return mapped, remapped, uses_target_season, rebased


async def _audit(
    c: AsyncConnection,
    guild: str,
    actor: str,
    request_id: str,
    action: str,
    import_id: uuid.UUID,
    before: dict[str, Any],
    after: dict[str, Any],
    now: datetime,
) -> None:
    await audit(
        c,
        {
            "guild": guild,
            "actor_id": actor,
            "audit_id": uuid.uuid7(),
            "request_id": request_id,
            "now": now,
        },
        action,
        "FULL_DATA_IMPORT",
        str(import_id),
        before,
        after,
    )


async def preview(
    database: Database, guild: str, actor: str, request_id: str, value: Any
) -> dict[str, Any]:
    archive = parse_archive(value)
    digest = archive_digest(archive)
    import_summary = summary(archive["tables"])
    now = datetime.now(UTC)
    expires = now + PREVIEW_TTL
    async with database.connection(transaction=True) as c:
        await repeatable_read(c)
        await authorize(c, guild, actor)
        await repository.lock_guild(c, guild)
        state = await repository.target_state(c, guild)
        target_digest = state_digest(state)
        uses_target_season = False
        rebased = False
        if state.importable:
            mapped, remapped, uses_target_season, rebased = map_archive(archive, guild, state)
            await repository.validate_insert(c, mapped)
            conflicts: list[dict[str, Any]] = []
        else:
            remapped = archive["source"]["guildId"] != guild
            if state.active_season_id is None:
                conflicts = [
                    {
                        "reason": "target_active_season_missing",
                        "message": (
                            "대상 길드에 사용할 활성 시즌이 없습니다. "
                            "빈 활성 시즌을 만든 뒤 다시 검증해 주세요."
                        ),
                        "table": None,
                        "rowCount": state.domain_rows,
                    }
                ]
            else:
                conflicts = [
                    {
                        "reason": "target_active_season_not_empty",
                        "message": (
                            "대상 길드의 현재 활성 시즌에 이미 전적·MMR 또는 "
                            "진행 데이터가 있어 전체 스냅샷을 적용할 수 없습니다."
                        ),
                        "table": None,
                        "rowCount": state.active_domain_rows,
                    }
                ]
        import_id = uuid.uuid7()
        await repository.store_preview(
            c,
            {
                "guild": guild,
                "import_id": import_id,
                "actor_id": actor,
                "archive_digest": digest,
                "archive_payload": json.dumps(archive, ensure_ascii=False),
                "source_guild_id": archive["source"]["guildId"],
                "target_digest": target_digest,
                "summary": json.dumps(import_summary),
                "conflicts": json.dumps(conflicts, ensure_ascii=False),
                "conflict_count": len(conflicts),
                "request_id": request_id,
                "now": now,
                "expires": expires,
            },
        )
        await _audit(
            c,
            guild,
            actor,
            request_id,
            "FULL_DATA_IMPORT_PREVIEWED",
            import_id,
            {
                "digest": digest.hex(),
                "sourceGuildId": archive["source"]["guildId"],
                "targetGuildId": guild,
            },
            {
                "totalRows": import_summary["totalRows"],
                "conflictCount": len(conflicts),
                "remapsSourceGuild": remapped,
                "usesTargetActiveSeason": uses_target_season,
                "rebasesDisplayNumbers": rebased,
            },
            now,
        )
    result = {
        "importId": str(import_id),
        "digest": digest.hex(),
        "targetDigest": target_digest.hex(),
        "expiresAt": timestamp(expires),
        "source": archive["source"],
        "summary": import_summary,
        "conflicts": conflicts,
        "remapsSourceGuild": remapped,
        "usesTargetActiveSeason": uses_target_season,
        "rebasesDisplayNumbers": rebased,
    }
    return FullDataPreview.model_validate(result).model_dump()


def _digest(value: str) -> bytes:
    if not re.fullmatch(r"[0-9a-fA-F]{64}", value):
        raise ApiError(
            422,
            "invalid_full_import_apply_command",
            "전체 데이터 가져오기 적용 확인 값이 올바르지 않습니다.",
        )
    return bytes.fromhex(value)


def _conflict(code: str, message: str, status: int = 409) -> ApiError:
    return ApiError(status, code, message)


async def _apply(
    c: AsyncConnection,
    p: dict[str, Any],
    import_id: uuid.UUID,
    body: FullDataApplyCommand,
    expected_digest: bytes,
    expected_target: bytes,
) -> dict[str, Any]:
    await authorize(c, p["guild"], p["actor_id"])
    await repository.lock_guild(c, p["guild"])
    try:
        replay = await acquire(c, p, False)
    except ApiError as exc:
        if exc.code == "idempotency_in_progress":
            raise _conflict(
                exc.code, "같은 전체 데이터 가져오기 요청을 처리하고 있습니다."
            ) from None
        raise
    if replay is not None:
        try:
            return FullDataApplyResult.model_validate(replay).model_dump()
        except ValidationError:
            raise ApiError.from_status(500) from None
    stored = await repository.lock_preview(c, p["guild"], import_id)
    if stored["actor_id"] != p["actor_id"]:
        raise _conflict(
            "preview_actor_mismatch", "가져오기 검토를 만든 운영자만 적용할 수 있습니다.", 403
        )
    if stored["status"] != "PREVIEWED":
        raise _conflict(
            "preview_already_applied", "이 전체 데이터 가져오기 검토 결과는 이미 적용되었습니다."
        )
    if stored["expires_at"] <= p["now"]:
        raise _conflict(
            "preview_expired",
            "전체 데이터 가져오기 검토 결과가 만료되었습니다. 파일을 다시 검증해 주세요.",
        )
    if stored["archive_digest"] != expected_digest:
        raise _conflict(
            "preview_digest_mismatch", "검토한 전체 백업 파일과 적용 요청이 일치하지 않습니다."
        )
    if stored["target_state_digest"] != expected_target:
        raise _conflict(
            "full_import_target_changed",
            "검토 이후 대상 길드의 멤버 또는 데이터 상태가 변경되었습니다. "
            "파일을 다시 검증해 주세요.",
        )
    if stored["conflict_count"] != 0:
        raise _conflict(
            "preview_conflicts", "검토에서 확인된 충돌 때문에 전체 데이터를 적용할 수 없습니다."
        )
    archive = parse_archive(stored["archive_payload"])
    if (
        archive["source"]["guildId"] != stored["source_guild_id"]
        or archive_digest(archive) != stored["archive_digest"]
    ):
        raise _conflict(
            "preview_digest_mismatch", "검토한 전체 백업 파일과 적용 요청이 일치하지 않습니다."
        )
    state = await repository.target_state(c, p["guild"])
    if not state.importable or state_digest(state) != stored["target_state_digest"]:
        raise _conflict(
            "full_import_target_changed",
            "검토 이후 대상 길드의 멤버 또는 데이터 상태가 변경되었습니다. "
            "파일을 다시 검증해 주세요.",
        )
    mapped, remapped, used_season, rebased = map_archive(archive, p["guild"], state)
    await repository.import_rows(c, mapped)
    await repository.mark_applied(c, p["guild"], import_id, p["now"])
    import_summary = stored["summary"]
    await _audit(
        c,
        p["guild"],
        p["actor_id"],
        p["request_id"],
        "FULL_DATA_IMPORT_APPLIED",
        import_id,
        {
            "digest": body.expectedDigest,
            "sourceGuildId": archive["source"]["guildId"],
            "targetGuildId": p["guild"],
        },
        {
            "summary": import_summary,
            "remappedSourceGuild": remapped,
            "usedTargetActiveSeason": used_season,
            "rebasedDisplayNumbers": rebased,
        },
        p["now"],
    )
    result = FullDataApplyResult.model_validate(
        {
            "importId": str(import_id),
            "status": "APPLIED",
            "appliedAt": timestamp(p["now"]),
            "summary": import_summary,
            "remappedSourceGuild": remapped,
            "usedTargetActiveSeason": used_season,
            "rebasedDisplayNumbers": rebased,
        }
    ).model_dump()
    await complete(c, p, result, 200)
    return result


async def apply(
    database: Database,
    guild: str,
    actor: str,
    request_id: str,
    import_id_text: str,
    key: str,
    value: Any,
) -> dict[str, Any]:
    try:
        import_id = uuid.UUID(import_id_text)
    except ValueError:
        raise ApiError.from_status(400) from None
    try:
        body = FullDataApplyCommand.model_validate(value)
    except ValidationError:
        raise ApiError(
            422,
            "invalid_full_import_apply_command",
            "전체 데이터 가져오기 적용 확인 값이 올바르지 않습니다.",
        ) from None
    expected_digest = _digest(body.expectedDigest)
    expected_target = _digest(body.expectedTargetDigest)
    now = datetime.now(UTC)
    p = {
        "guild": guild,
        "actor_id": actor,
        "principal": actor,
        "principal_type": "USER",
        "method": "POST",
        "route": APPLY_ROUTE,
        "key": key,
        "request_id": request_id,
        "audit_id": uuid.uuid7(),
        "idem_id": uuid.uuid7(),
        "now": now,
        "expires": now + IDEMPOTENCY_TTL,
        "hash": hashlib.sha256(
            stable_bytes(
                [
                    guild,
                    str(import_id),
                    {
                        "expectedDigest": body.expectedDigest,
                        "expectedTargetDigest": body.expectedTargetDigest,
                    },
                ]
            )
        ).digest(),
    }
    return await mutation(
        database, lambda c: _apply(c, p, import_id, body, expected_digest, expected_target)
    )
