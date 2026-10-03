"""Operator member administration with atomic revision, ledger, and audit changes."""

import hashlib
import logging
import math
import uuid
from datetime import UTC, datetime, timedelta
from itertools import groupby
from typing import Any

from pydantic import TypeAdapter, ValidationError
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.text import trim
from monkeybot_api.core.time import timestamp
from monkeybot_api.db.database import Database
from monkeybot_api.db.transactions import mutation, repeatable_read
from monkeybot_api.repositories.effects import complete
from monkeybot_api.repositories.guilds import members as repository
from monkeybot_api.repositories.idempotency import acquire
from monkeybot_api.repositories.permissions import authorize
from monkeybot_api.repositories.series.mmr import lock_mmr
from monkeybot_api.schemas.guilds.members import AdjustMmr, MembersSnapshot, SetMemberStatus
from monkeybot_api.schemas.users import LANES

logger = logging.getLogger(__name__)

MMR_ROUTE = "/api/guilds/{guildId}/members/{memberId}/mmr"
MMR_REQUEST = TypeAdapter(tuple[str, str, AdjustMmr])


def invariant_failure(request_id: str, reason: str) -> ApiError:
    logger.error(reason, extra={"request_id": request_id})
    return ApiError.from_status(500)


def not_found() -> ApiError:
    return ApiError(404, "not_found", "요청한 길드 멤버를 찾을 수 없습니다.")


def invalid_adjustment() -> ApiError:
    return ApiError(
        422,
        "invalid_mmr_adjustment",
        "MMR 증감값과 조정 사유를 확인해 주세요.",
        {"noteMaxLength": 500},
    )


def validate_revision(revision: int) -> None:
    if revision < 0:
        raise ApiError(422, "invalid_expected_revision", "expectedRevision은 0 이상이어야 합니다.")


async def require_operator(c: AsyncConnection, guild: str, actor: str) -> None:
    try:
        await authorize(c, guild, actor)
    except ApiError as exc:
        if exc.code == "not_found":
            raise not_found() from None
        raise


async def snapshot(c: AsyncConnection, guild: str, request_id: str) -> dict[str, Any]:
    metadata = await repository.metadata(c, guild)
    if metadata is None:
        raise not_found()
    members = []
    for user, group in groupby(await repository.member_lanes(c, guild), key=lambda r: r["user_id"]):
        rows = list(group)
        if len(rows) != 5 or {r["mmr_lane"] for r in rows} != set(LANES):
            raise invariant_failure(request_id, "member_lane_projection_invalid")
        row = rows[0]
        members.append(
            {
                "id": user,
                "identityKind": row["identity_kind"],
                "discordName": row["display_name"],
                "riotId": row["riot_id"],
                "role": "OPERATOR" if row["is_operator"] else "MEMBER",
                "mainLane": row["main_lane"],
                "laneMmr": {r["mmr_lane"]: r["mmr"] for r in rows},
                "registered": row["registered"],
                "status": row["status"],
                "lastActiveAt": timestamp(row["last_active_at"]) if row["last_active_at"] else None,
            }
        )
    return validate_snapshot(
        {
            "revision": metadata["revision"],
            "updatedAt": timestamp(metadata["updated_at"]),
            "updatedBy": metadata["updated_by"],
            "members": members,
        },
        request_id,
    )


def validate_snapshot(value: dict[str, Any], request_id: str) -> dict[str, Any]:
    try:
        return MembersSnapshot.model_validate(value).model_dump()
    except ValidationError:
        logger.exception("guild_members_projection_failed", extra={"request_id": request_id})
        raise ApiError.from_status(500) from None


async def get_members(
    database: Database, guild: str, actor: str, request_id: str
) -> dict[str, Any]:
    async with database.connection(transaction=True) as c:
        await repeatable_read(c)
        await require_operator(c, guild, actor)
        return await snapshot(c, guild, request_id)


async def lock_revision(c: AsyncConnection, p: dict[str, Any], expected: int) -> int:
    state = await repository.lock_state(c, p)
    current: int = state["revision"]
    if expected != current:
        raise ApiError(
            409,
            "revision_conflict",
            "최신 상태를 다시 확인해야 합니다.",
            {
                "expectedRevision": expected,
                "currentRevision": current,
                "updatedAt": timestamp(state["updated_at"]),
                "updatedBy": state["updated_by"],
            },
        )
    return current


async def increment_revision(c: AsyncConnection, p: dict[str, Any], current: int) -> None:
    if current == 2**63 - 1 or await repository.increment_revision(c, p, current) != 1:
        raise invariant_failure(p["request_id"], "members_revision_increment_failed")


async def adjust(c: AsyncConnection, p: dict[str, Any], body: AdjustMmr) -> dict[str, Any]:
    p["now"] = datetime.now(UTC)
    p["expires"] = p["now"] + timedelta(hours=24)
    await require_operator(c, p["guild"], p["actor_id"])
    try:
        replay = await acquire(c, p, False)
    except ApiError as exc:
        if exc.code == "idempotency_in_progress":
            raise ApiError(409, exc.code, "같은 요청을 처리하고 있습니다.") from None
        raise
    if replay is not None:
        return validate_snapshot(replay, p["request_id"])
    revision = await lock_revision(c, p, body.expectedRevision)
    if await repository.lock_member_status(c, p) is None:
        raise not_found()
    season = await repository.lock_active_season(c, p["guild"])
    if season is None:
        raise ApiError(409, "guild_setup_required", "길드의 활성 시즌을 먼저 설정해야 합니다.")
    p["season"] = season
    locked = (await lock_mmr(c, p, [{"user_id": p["member"], "lane": body.lane}]))[p["member"]]
    after = locked["mmr"] + body.delta
    if not math.isfinite(after):
        raise invalid_adjustment()
    if locked["revision"] == 2**63 - 1:
        raise invariant_failure(p["request_id"], "member_lane_revision_overflow")
    note = trim(body.note)
    values = {
        **p,
        "lane": body.lane,
        "lane_revision": locked["revision"] + 1,
        "before": locked["mmr"],
        "after": after,
        "delta": body.delta,
        "note": note,
    }
    await repository.append_adjustment(c, values)
    if await repository.update_mmr(c, values) != 1:
        raise invariant_failure(p["request_id"], "member_mmr_update_lost")
    await increment_revision(c, p, revision)
    await repository.append_audit(
        c,
        p,
        "MEMBER_MMR_ADJUSTED",
        "GUILD_MEMBER_MMR",
        {"lane": body.lane, "mmr": locked["mmr"], "laneRevision": locked["revision"]},
        {
            "lane": body.lane,
            "mmr": after,
            "delta": body.delta,
            "laneRevision": locked["revision"] + 1,
        },
        note,
    )
    result = await snapshot(c, p["guild"], p["request_id"])
    await complete(c, p, result, 200)
    return result


async def change_status(
    c: AsyncConnection, p: dict[str, Any], body: SetMemberStatus
) -> dict[str, Any]:
    p["now"] = datetime.now(UTC)
    await require_operator(c, p["guild"], p["actor_id"])
    if p["member"] == p["actor_id"] and body.status == "INACTIVE":
        raise ApiError(409, "self_deactivation_forbidden", "자기 자신은 비활성화할 수 없습니다.")
    revision = await lock_revision(c, p, body.expectedRevision)
    previous = await repository.lock_member_status(c, p)
    if previous is None:
        raise not_found()
    if await repository.set_status(c, p, body.status) != 1:
        raise invariant_failure(p["request_id"], "member_status_update_lost")
    await increment_revision(c, p, revision)
    await repository.append_audit(
        c, p, "MEMBER_STATUS_CHANGED", "GUILD_MEMBER", {"status": previous}, {"status": body.status}
    )
    return await snapshot(c, p["guild"], p["request_id"])


async def adjust_mmr(
    database: Database,
    guild: str,
    member: str,
    actor: str,
    request_id: str,
    key: str,
    body: AdjustMmr,
) -> dict[str, Any]:
    validate_revision(body.expectedRevision)
    if not math.isfinite(body.delta) or body.delta == 0 or not 1 <= len(trim(body.note)) <= 500:
        raise invalid_adjustment()
    p = {
        "guild": guild,
        "member": member,
        "actor_id": actor,
        "request_id": request_id,
        "principal": actor,
        "principal_type": "USER",
        "method": "PATCH",
        "route": MMR_ROUTE,
        "key": key,
        "hash": hashlib.sha256(MMR_REQUEST.dump_json((guild, member, body))).digest(),
        "idem_id": uuid.uuid7(),
        "ledger_id": uuid.uuid7(),
        "source_id": uuid.uuid7(),
        "audit_id": uuid.uuid7(),
    }
    return await mutation(database, lambda c: adjust(c, p, body))


async def set_status(
    database: Database,
    guild: str,
    member: str,
    actor: str,
    request_id: str,
    body: SetMemberStatus,
) -> dict[str, Any]:
    validate_revision(body.expectedRevision)
    p = {
        "guild": guild,
        "member": member,
        "actor_id": actor,
        "request_id": request_id,
        "audit_id": uuid.uuid7(),
    }
    return await mutation(database, lambda c: change_status(c, p, body))
