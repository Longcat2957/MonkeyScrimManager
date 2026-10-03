import base64
import json
from typing import Any

from sqlalchemy.ext.asyncio import AsyncConnection

import monkeybot_api.repositories.recruitments.lifecycle as repository
from monkeybot_api.core.errors import ApiError
from monkeybot_api.repositories.guild_access import require_operator
from monkeybot_api.repositories.idempotency import acquire
from monkeybot_api.repositories.members import sync_actor
from monkeybot_api.repositories.recruitments.entries import entry_snapshot
from monkeybot_api.repositories.recruitments.snapshots import recruitment_snapshot
from monkeybot_api.schemas.bot import Actor
from monkeybot_api.schemas.recruitments.lifecycle import ENTRY_ACTIONS, Action
from monkeybot_api.services.recruitments.entries import apply_move


async def require_member(
    connection: AsyncConnection, params: dict[str, Any], action: Action, bot: bool
) -> None:
    active = await repository.lock_actor_membership(connection, params)
    if active != "ACTIVE":
        if bot:
            raise ApiError(
                403, "member_inactive", "비활성화된 길드 멤버는 이 작업을 수행할 수 없습니다."
            )
        message = (
            "해당 길드의 활성 멤버만 엔트리 편성을 변경할 수 있습니다."
            if action in ENTRY_ACTIONS
            else "해당 길드의 활성 멤버만 모집을 변경할 수 있습니다."
        )
        raise ApiError(403, "guild_membership_required", message)


async def save_effects(
    connection: AsyncConnection,
    params: dict[str, Any],
    action: Action,
    before: dict[str, Any],
    snapshot: dict[str, Any],
    message: dict[str, Any],
    bot: bool,
) -> None:
    is_entry = action in ENTRY_ACTIONS
    target_type = "RECRUITMENT_ENTRY_DRAFT" if is_entry else "RECRUITMENT"
    revision = snapshot["draft"]["revision"] if is_entry else snapshot["recruitment"]["revision"]
    values = {
        **params,
        "before": json.dumps(before),
        "snapshot": json.dumps(snapshot),
        "target_type": target_type,
        "target": str(params["id"]),
    }
    await repository.insert_audit(connection, values)
    events = [
        (
            params["event_id"],
            target_type,
            "recruitment-entry-draft" if is_entry else "recruitment",
            revision,
        )
    ]
    if action == "Reopen":
        events.append(
            (
                params["recruitment_event_id"],
                "RECRUITMENT",
                "recruitment",
                snapshot["recruitment"]["revision"],
            )
        )
    for event_id, aggregate, topic_part, event_revision in events:
        await repository.insert_event(
            connection,
            {
                **values,
                "event": event_id,
                "aggregate": aggregate,
                "revision": event_revision,
                "topic": f"guild:{params['guild']}:{topic_part}:{params['id']}",
                "payload": json.dumps(
                    {
                        "type": "invalidate",
                        "resourceId": str(params["id"]),
                        "revision": event_revision,
                    }
                ),
            },
        )
    data = message["recruitment"]
    if (
        action not in {"Apply", "Reset"}
        and not bot
        and data["channelId"] is not None
        and (action == "Restore" or data["messageId"] is not None)
    ):
        verb, notification_type = {
            "Cancel": ("delete", "RECRUITMENT_MESSAGE_DELETE"),
            "Restore": ("restore", "RECRUITMENT_MESSAGE_CREATE"),
        }.get(action, ("update", "RECRUITMENT_MESSAGE_UPDATE"))
        await repository.enqueue_message(
            connection,
            {
                **values,
                "dedup": f"recruitment:{data['id']}:message:{verb}:{data['revision']}",
                "channel": data["channelId"],
                "notification_type": notification_type,
                "nonce": base64.urlsafe_b64encode(params["notification_id"].bytes)
                .decode()
                .rstrip("="),
                "payload": json.dumps(
                    {"schemaVersion": 1, "recruitmentId": data["id"], "snapshot": message}
                ),
            },
        )
    completed = await repository.complete_lifecycle(connection, values)
    if completed is None:
        raise ApiError.from_status(500)


async def mutate(
    connection: AsyncConnection, params: dict[str, Any], action: Action, actor: Actor | None
) -> dict[str, Any]:
    bot = actor is not None
    status = await repository.lock_guild(connection, params)
    if status is None:
        raise ApiError(404, "not_found", "요청한 대상을 찾을 수 없습니다.")
    if status != "ACTIVE":
        raise ApiError(
            409,
            "guild_inactive",
            "현재 길드에서는 command를 실행할 수 없습니다.",
            {"currentStatus": status},
        )
    role = await repository.lock_operator_role(connection, params)
    if role is None:
        raise ApiError(409, "guild_setup_required", "길드 초기 설정을 먼저 완료해야 합니다.")
    # Axum's entry commands authorize before idempotency; recruitment lifecycle authorizes after.
    if action in ENTRY_ACTIONS:
        await require_member(connection, params, action, False)
        await require_operator(connection, params, role, None)
    try:
        replay = await acquire(connection, params, bot)
    except ApiError as error:
        if not bot and error.code == "idempotency_in_progress":
            raise ApiError(
                409,
                error.code,
                "같은 엔트리 편성 요청을 처리하고 있습니다."
                if action in ENTRY_ACTIONS
                else "같은 모집 변경 요청을 처리하고 있습니다.",
            ) from None
        raise
    if replay is not None:
        return replay
    if actor is not None:
        await sync_actor(connection, actor, params)
        await require_member(connection, params, action, True)
    if action == "Close":
        if actor is None:
            await require_member(connection, params, action, False)
        await require_operator(connection, params, role, actor)
    locked = await repository.lock_recruitment(connection, params)
    if locked is None:
        raise ApiError(404, "not_found", "요청한 대상을 찾을 수 없습니다.")
    if action in {"Cancel", "Restore"}:
        await require_member(connection, params, action, False)
        if locked["created_by"] != params["actor_id"]:
            await require_operator(connection, params, role, None)
    draft = None
    if action in ENTRY_ACTIONS:
        draft = await repository.lock_entry_revision(connection, params)
        if draft is None:
            raise ApiError.from_status(500)
    actual_revision = draft if action in {"Apply", "Reset"} else locked["revision"]
    if action != "Close" and actual_revision != params["expected"]:
        raise ApiError(
            409,
            "entry_draft_revision_conflict"
            if action in ENTRY_ACTIONS
            else "recruitment_revision_conflict",
            "다른 운영자가 엔트리 편성을 변경했습니다. 최신 상태를 확인해 주세요."
            if action in ENTRY_ACTIONS
            else "모집 상태가 변경되었습니다. 최신 상태를 확인해 주세요.",
            {"expectedRevision": params["expected"], "actualRevision": actual_revision},
        )
    before = None
    if action in ENTRY_ACTIONS:
        before = await entry_snapshot(connection, params["guild"], params["id"], params["actor_id"])
    allowed = {
        "Close": {"OPEN"},
        "Cancel": {"OPEN", "CLOSED"},
        "Restore": {"CANCELLED"},
        "Reopen": {"CLOSED"},
        "Apply": {"CLOSED"},
        "Reset": {"CLOSED"},
    }
    if locked["status"] not in allowed[action]:
        error_action = "Reopen" if action in ENTRY_ACTIONS else action
        code, message = {
            "Restore": ("recruitment_restore_unavailable", "취소된 모집만 복구할 수 있습니다."),
            "Reopen": (
                "entry_draft_not_editable",
                "현재 모집 상태에서는 엔트리 편성을 수정할 수 없습니다.",
            ),
        }.get(
            error_action,
            ("invalid_recruitment_state", "현재 모집 상태에서는 이 작업을 수행할 수 없습니다."),
        )
        raise ApiError(409, code, message, {"currentStatus": locked["status"]})
    if before is None:
        before = await recruitment_snapshot(connection, params["guild"], params["id"])
    if action == "Close":
        count = await repository.joined_count(connection, params)
        if count != locked["target_count"]:
            raise ApiError(
                422,
                "recruitment_not_full",
                "정원을 채운 뒤 엔트리 편성을 시작할 수 있습니다.",
                {"joinedCount": count, "targetCount": locked["target_count"]},
            )
    if action in {"Apply", "Reset"}:
        if action == "Apply":
            await apply_move(connection, params)
        else:
            await repository.clear_assignments(connection, params)
        await repository.increment_entry_revision(connection, params)
        await repository.touch_recruitment(connection, params)
        snapshot = await entry_snapshot(
            connection, params["guild"], params["id"], params["actor_id"]
        )
        await save_effects(connection, params, action, before, snapshot, snapshot, False)
        return snapshot
    changed = await repository.update_lifecycle(
        connection,
        {**params, "restored": "CLOSED" if locked["closed_at"] is not None else "OPEN"},
        action,
    )
    if changed is None:
        raise ApiError.from_status(500)
    if action == "Reopen":
        changed = await repository.reopen_entry_draft(connection, params)
        if changed is None:
            raise ApiError.from_status(500)
        message_snapshot = await recruitment_snapshot(connection, params["guild"], params["id"])
        snapshot = await entry_snapshot(
            connection, params["guild"], params["id"], params["actor_id"]
        )
    else:
        snapshot = await recruitment_snapshot(connection, params["guild"], params["id"])
        message_snapshot = snapshot
    await save_effects(connection, params, action, before, snapshot, message_snapshot, bot)
    return snapshot
