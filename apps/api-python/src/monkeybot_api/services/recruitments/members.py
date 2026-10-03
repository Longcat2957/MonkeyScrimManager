import base64
import json
from typing import Any

from sqlalchemy.ext.asyncio import AsyncConnection

import monkeybot_api.repositories.recruitments.members as repository
from monkeybot_api.core.errors import ApiError
from monkeybot_api.repositories.idempotency import acquire
from monkeybot_api.repositories.members import sync_actor
from monkeybot_api.repositories.recruitments.snapshots import recruitment_snapshot
from monkeybot_api.schemas.bot import Actor


async def mutate(
    connection: AsyncConnection,
    params: dict[str, Any],
    actor: Actor | None,
    participant: Actor | None,
    joining: bool,
    lanes: list[str] | None = None,
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
    try:
        replay = await acquire(connection, params, bot)
    except ApiError as error:
        if not bot and error.code == "idempotency_in_progress":
            raise ApiError(409, error.code, "같은 모집 변경 요청을 처리하고 있습니다.") from None
        raise
    if replay is not None:
        return replay
    if actor is not None:
        await sync_actor(connection, actor, params)
    member_status = await repository.lock_actor_membership(connection, params)
    if member_status != "ACTIVE":
        if bot:
            raise ApiError(
                403, "member_inactive", "비활성화된 길드 멤버는 이 작업을 수행할 수 없습니다."
            )
        raise ApiError(
            403, "guild_membership_required", "해당 길드의 활성 멤버만 모집을 변경할 수 있습니다."
        )
    if actor is not None:
        authorized = actor.id == params["member"] or role in actor.roleIds
    else:
        authorized = await repository.operator_role_exists(connection, {**params, "role": role})
    if not authorized:
        raise ApiError(403, "operator_required", "길드 운영자 권한이 필요합니다.")
    locked = await repository.lock_recruitment(connection, params)
    if locked is None:
        raise ApiError(404, "not_found", "요청한 대상을 찾을 수 없습니다.")
    if locked["status"] != "OPEN":
        raise ApiError(
            409,
            "invalid_recruitment_state",
            "현재 모집 상태에서는 이 작업을 수행할 수 없습니다.",
            {"currentStatus": locked["status"]},
        )
    draft = await repository.lock_entry_revision(connection, params)
    if draft is None:
        raise ApiError.from_status(500)
    if joining:
        if participant is not None and participant.id != params["actor_id"]:
            await sync_actor(connection, participant, {**params, "actor_id": participant.id})
        target_status = await repository.lock_target_membership(connection, params)
        if target_status != "ACTIVE":
            if bot:
                raise ApiError(
                    403, "member_inactive", "비활성화된 길드 멤버는 이 작업을 수행할 수 없습니다."
                )
            if target_status is None:
                raise ApiError(
                    404, "guild_member_not_found", "길드에 소속된 사용자를 찾을 수 없습니다."
                )
            raise ApiError(
                409, "guild_member_inactive", "비활성화된 길드 멤버는 모집에 추가할 수 없습니다."
            )
    before = await recruitment_snapshot(connection, params["guild"], params["id"])
    if lanes is not None:
        existing = await repository.lock_participation(connection, params)
        if existing != "JOINED":
            raise ApiError(422, "not_participant", "현재 모집 참가자가 아닙니다.")
        await repository.clear_preferred_lanes(connection, params)
        for priority, lane in enumerate(lanes, 1):
            await repository.insert_preferred_lane(
                connection, {**params, "priority": priority, "lane": lane}
            )
    elif joining:
        existing = await repository.lock_participation(connection, params)
        if existing == "JOINED":
            raise ApiError(422, "already_participant", "이미 참가 중인 사용자입니다.")
        count = await repository.joined_count(connection, params)
        if count >= locked["target_count"]:
            raise ApiError(422, "recruitment_full", "모집 정원이 가득 찼습니다.")
        await repository.join_participant(connection, params)
    else:
        changed = await repository.withdraw_participant(connection, params)
        if changed is None:
            raise ApiError(422, "not_participant", "현재 모집 참가자가 아닙니다.")
        await repository.remove_assignment(connection, params)
    await repository.increment_recruitment_revision(connection, params)
    await repository.increment_entry_revision(connection, params)
    snapshot = await recruitment_snapshot(connection, params["guild"], params["id"])
    data = snapshot["recruitment"]
    values = {
        **params,
        "before": json.dumps(before),
        "snapshot": json.dumps(snapshot),
        "revision": data["revision"],
        "target": str(params["id"]),
    }
    await repository.insert_audit(connection, values)
    await repository.insert_event(
        connection,
        {
            **values,
            "topic": f"guild:{params['guild']}:recruitment:{params['id']}",
            "payload": json.dumps(
                {"type": "invalidate", "resourceId": data["id"], "revision": data["revision"]}
            ),
        },
    )
    if not bot and data["channelId"] is not None and data["messageId"] is not None:
        await repository.enqueue_message(
            connection,
            {
                **values,
                "dedup": f"recruitment:{data['id']}:message:update:{data['revision']}",
                "channel": data["channelId"],
                "nonce": base64.urlsafe_b64encode(params["notification_id"].bytes)
                .decode()
                .rstrip("="),
                "payload": json.dumps(
                    {"schemaVersion": 1, "recruitmentId": data["id"], "snapshot": snapshot}
                ),
            },
        )
    await repository.complete_membership(connection, values)
    return snapshot
