"""Development-only dummy recruitment participant workflow."""

import hashlib
import uuid
from datetime import UTC, datetime, timedelta
from typing import Any, Literal

from pydantic_core import to_json
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError
from monkeybot_api.db.database import Database
from monkeybot_api.db.transactions import mutation
from monkeybot_api.repositories.dev import recruitments as repository
from monkeybot_api.repositories.effects import audit, complete, invalidate, notification
from monkeybot_api.repositories.idempotency import acquire
from monkeybot_api.repositories.recruitments.snapshots import recruitment_snapshot
from monkeybot_api.schemas.dev.recruitments import ApplyParticipants, PreviewParticipants

Action = Literal["ADD", "REMOVE"]
ROUTE = "/api/dev/guilds/{guildId}/recruitments/{recruitmentId}/participants"


def _issue(user: str | None, code: str, message: str) -> dict[str, Any]:
    return {"userId": user, "code": code, "message": message}


def _valid_selection(users: list[str]) -> bool:
    return 1 <= len(users) <= 10 and len(set(users)) == len(users)


def build_preview(overview: dict[str, Any], action: Action, users: list[str]) -> dict[str, Any]:
    issues: list[dict[str, Any]] = []
    unique_users = list(dict.fromkeys(users))
    if not _valid_selection(users):
        issues.append(
            _issue(
                None,
                "invalid_dummy_participant_selection",
                "더미 참가자는 중복 없이 1명 이상 10명 이하로 선택해야 합니다.",
            )
        )
    candidates = {item["userId"]: item for item in overview["candidates"]}
    selected = [candidates[user] for user in unique_users if user in candidates]
    recruitment = overview["recruitment"]
    if recruitment["status"] not in {"OPEN", "CLOSED"}:
        issues.append(
            _issue(
                None,
                "recruitment_not_editable",
                "현재 모집 상태에서는 참가자를 변경할 수 없습니다.",
            )
        )
    for user in unique_users:
        candidate = candidates.get(user)
        if candidate is None:
            issues.append(
                _issue(
                    user,
                    "dummy_user_not_found",
                    "이 길드에 배치된 더미 사용자를 찾을 수 없습니다.",
                )
            )
        elif action == "ADD":
            if candidate["profileState"] == "ARCHIVED":
                issues.append(
                    _issue(
                        user,
                        "dummy_user_archived",
                        "보관된 더미 사용자는 모집에 추가할 수 없습니다.",
                    )
                )
            elif candidate["membershipStatus"] == "INACTIVE":
                issues.append(
                    _issue(
                        user,
                        "dummy_membership_inactive",
                        "비활성 길드 회원은 모집에 추가할 수 없습니다.",
                    )
                )
            elif candidate["participationState"] == "JOINED":
                issues.append(
                    _issue(
                        user,
                        "already_participant",
                        "이미 모집에 참가 중인 더미 사용자입니다.",
                    )
                )
        elif candidate["participationState"] != "JOINED":
            issues.append(
                _issue(
                    user,
                    "not_participant",
                    "현재 모집 참가자가 아닌 더미 사용자입니다.",
                )
            )
    if action == "ADD":
        changes = sum(
            item["profileState"] == "ACTIVE"
            and item["membershipStatus"] == "ACTIVE"
            and item["participationState"] != "JOINED"
            for item in selected
        )
        projected = recruitment["joinedCount"] + changes
        if projected > recruitment["targetCount"]:
            issues.append(
                _issue(
                    None,
                    "recruitment_capacity_exceeded",
                    "선택한 사용자를 추가하면 모집 정원을 초과합니다.",
                )
            )
    else:
        changes = sum(item["participationState"] == "JOINED" for item in selected)
        projected = recruitment["joinedCount"] - changes
    return {
        "action": action,
        "canApply": not issues,
        "currentRevision": recruitment["revision"],
        "projectedJoinedCount": projected,
        "remainingSlots": max(recruitment["targetCount"] - projected, 0),
        "selected": selected,
        "issues": issues,
    }


async def overview(
    database: Database, guild: str, recruitment: uuid.UUID, actor: str
) -> dict[str, Any]:
    async with database.connection() as c:
        await repository.authorize_operator(c, guild, actor)
        return await repository.overview(c, guild, recruitment)


async def preview(
    database: Database,
    guild: str,
    recruitment: uuid.UUID,
    actor: str,
    command: PreviewParticipants,
) -> dict[str, Any]:
    current = await overview(database, guild, recruitment, actor)
    return build_preview(current, command.action, command.userIds)


def _selected_error(action: Action, user: str, candidate: dict[str, Any] | None) -> ApiError | None:
    details = {"userId": user}
    if candidate is None:
        return ApiError(
            404,
            "dummy_user_not_found",
            "이 길드에 배치된 더미 사용자를 찾을 수 없습니다.",
            details,
        )
    if action == "ADD":
        if candidate["profileState"] == "ARCHIVED":
            return ApiError(
                409,
                "dummy_user_archived",
                "보관된 더미 사용자는 모집에 추가할 수 없습니다.",
                details,
            )
        if candidate["membershipStatus"] == "INACTIVE":
            return ApiError(
                409,
                "dummy_membership_inactive",
                "비활성 길드 회원은 모집에 추가할 수 없습니다.",
                details,
            )
        if candidate["participationState"] == "JOINED":
            return ApiError(
                409,
                "already_participant",
                "이미 모집에 참가 중인 더미 사용자입니다.",
                details,
            )
    elif candidate["participationState"] != "JOINED":
        return ApiError(
            409,
            "not_participant",
            "현재 모집 참가자가 아닌 더미 사용자입니다.",
            details,
        )
    return None


async def _apply(
    c: AsyncConnection,
    p: dict[str, Any],
    recruitment: uuid.UUID,
    action: Action,
    command: ApplyParticipants,
) -> dict[str, Any]:
    await repository.authorize_operator(c, p["guild"], p["actor_id"])
    locked = await repository.lock_recruitment(c, p["guild"], recruitment)
    try:
        replay = await acquire(c, p, False)
    except ApiError as exc:
        if exc.code == "idempotency_in_progress":
            raise ApiError(409, exc.code, "같은 더미 참가자 요청을 처리하고 있습니다.") from None
        raise
    if replay is not None:
        return replay
    if locked["revision"] != command.expectedRevision:
        raise ApiError(
            409,
            "revision_conflict",
            "모집 참가자 정보가 변경되었습니다. 최신 상태를 다시 확인해 주세요.",
            {"expectedRevision": command.expectedRevision, "currentRevision": locked["revision"]},
        )
    if locked["status"] not in {"OPEN", "CLOSED"}:
        raise ApiError(
            409,
            "recruitment_not_editable",
            "현재 모집 상태에서는 참가자를 변경할 수 없습니다.",
            {"currentStatus": locked["status"]},
        )
    before = await repository.overview(c, p["guild"], recruitment)
    selected = await repository.selected_candidates(c, p["guild"], recruitment, command.userIds)
    for user in command.userIds:
        error = _selected_error(action, user, selected.get(user))
        if error:
            raise error
    if action == "ADD" and locked["joined_count"] + len(selected) > locked["target_count"]:
        raise ApiError(
            422,
            "recruitment_capacity_exceeded",
            "선택한 사용자를 추가하면 모집 정원을 초과합니다.",
            {
                "joinedCount": locked["joined_count"],
                "requestedCount": len(selected),
                "targetCount": locked["target_count"],
            },
        )
    # Rust mutates selected candidates ordered by profile user ID.
    for user in sorted(selected):
        if action == "ADD":
            await repository.add_participant(c, p["guild"], recruitment, selected[user], p["now"])
        else:
            await repository.remove_participant(c, p["guild"], recruitment, user, p["now"])
    await repository.bump_revisions(c, p["guild"], recruitment, p["actor_id"], p["now"])
    after = await repository.overview(c, p["guild"], recruitment)
    result = {
        "action": action,
        "changedUserIds": sorted(selected),
        "overview": after,
    }
    await audit(
        c,
        p,
        "DUMMY_RECRUITMENT_PARTICIPANTS_ADDED"
        if action == "ADD"
        else "DUMMY_RECRUITMENT_PARTICIPANTS_REMOVED",
        "RECRUITMENT",
        str(recruitment),
        before,
        after,
    )
    await invalidate(
        c,
        p,
        "RECRUITMENT",
        str(recruitment),
        after["recruitment"]["revision"],
        "RECRUITMENT_PARTICIPANTS_CHANGED",
    )
    if locked["channel_id"] is not None and locked["message_id"] is not None:
        snapshot = await recruitment_snapshot(c, p["guild"], recruitment)
        await notification(
            c,
            p,
            "RECRUITMENT_MESSAGE_UPDATE",
            locked["channel_id"],
            f"recruitment:{recruitment}:message:update:{after['recruitment']['revision']}",
            {"schemaVersion": 1, "recruitmentId": str(recruitment), "snapshot": snapshot},
        )
    await complete(c, p, result, 200)
    return result


async def apply(
    database: Database,
    guild: str,
    recruitment: uuid.UUID,
    actor: str,
    request_id: str,
    key: str,
    action: Action,
    command: ApplyParticipants,
) -> dict[str, Any]:
    if not _valid_selection(command.userIds):
        raise ApiError(
            422,
            "invalid_dummy_participant_selection",
            "더미 참가자는 중복 없이 1명 이상 10명 이하로 선택해야 합니다.",
        )
    if command.expectedRevision < 0:
        raise ApiError(422, "invalid_expected_revision", "expectedRevision은 0 이상이어야 합니다.")
    now = datetime.now(UTC)
    p = {
        "guild": guild,
        "actor_id": actor,
        "principal": actor,
        "principal_type": "USER",
        "method": "POST" if action == "ADD" else "DELETE",
        "route": ROUTE,
        "key": key,
        "request_id": request_id,
        "now": now,
        "expires": now + timedelta(hours=24),
        "audit_id": uuid.uuid7(),
        "idem_id": uuid.uuid7(),
        "hash": hashlib.sha256(
            to_json(
                [
                    guild,
                    str(recruitment),
                    action,
                    {
                        "userIds": command.userIds,
                        "expectedRevision": command.expectedRevision,
                    },
                ]
            )
        ).digest(),
    }
    return await mutation(database, lambda c: _apply(c, p, recruitment, action, command))
