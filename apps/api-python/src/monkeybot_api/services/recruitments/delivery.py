from typing import Any

from sqlalchemy.ext.asyncio import AsyncConnection

import monkeybot_api.repositories.recruitments.delivery as repository
import monkeybot_api.services.recruitments.conversion as conversion
from monkeybot_api.core.config import is_snowflake
from monkeybot_api.core.errors import ApiError
from monkeybot_api.repositories.effects import audit, complete, invalidate, notification
from monkeybot_api.repositories.guild_access import require_operator
from monkeybot_api.repositories.idempotency import acquire
from monkeybot_api.repositories.members import sync_actor
from monkeybot_api.repositories.recruitments.snapshots import recruitment_snapshot
from monkeybot_api.schemas.bot import Actor
from monkeybot_api.schemas.recruitments.delivery import Action
from monkeybot_api.services.series.creation import create_series


async def authorize(
    c: AsyncConnection, p: dict[str, Any], action: Action, role: str, actor: Actor | None
) -> None:
    member = await repository.lock_actor_membership(c, p)
    if member != "ACTIVE":
        if action == "convert":
            raise conversion.error("guild_membership_required")
        if actor is not None:
            raise ApiError(
                403, "member_inactive", "비활성화된 길드 멤버는 이 작업을 수행할 수 없습니다."
            )
        raise ApiError(
            403,
            "guild_membership_required",
            "해당 길드의 활성 멤버만 참가 확정 DM을 발송할 수 있습니다."
            if action == "dm"
            else "해당 길드의 활성 멤버만 이 작업을 수행할 수 있습니다.",
        )
    if actor is None:
        try:
            await require_operator(c, p, role, None)
        except ApiError as error:
            if action == "convert" and error.code == "operator_required":
                raise conversion.error(error.code) from None
            if action == "dm" and error.code == "operator_required":
                raise ApiError(
                    403, error.code, "참가 확정 DM 발송에는 현재 길드 운영자 권한이 필요합니다."
                ) from None
            raise


async def mutate(
    c: AsyncConnection, p: dict[str, Any], action: Action, actor: Actor | None
) -> dict[str, Any]:
    status = await repository.lock_guild(c, p)
    if status is None:
        raise ApiError(404, "not_found", "요청한 대상을 찾을 수 없습니다.")
    if status != "ACTIVE":
        raise ApiError(
            409,
            "guild_inactive",
            "현재 길드에서는 command를 실행할 수 없습니다.",
            {"currentStatus": status},
        )
    config = await repository.lock_config(c, p)
    if config is None:
        raise ApiError(409, "guild_setup_required", "길드 초기 설정을 먼저 완료해야 합니다.")
    role = config["operator_role_id"]
    if action in {"series", "convert"}:
        await authorize(c, p, action, role, actor)
    try:
        replay = await acquire(c, p, actor is not None)
    except ApiError as error:
        if error.code == "idempotency_in_progress" and actor is None:
            if action == "convert":
                raise conversion.error(error.code) from None
            raise ApiError(
                409,
                error.code,
                "같은 참가 확정 DM 요청을 처리하고 있습니다."
                if action == "dm"
                else "같은 시리즈 시작 요청을 처리하고 있습니다.",
            ) from None
        raise
    if replay is not None:
        return replay
    if actor is not None:
        await sync_actor(c, actor, p)
    if action not in {"series", "convert"}:
        await authorize(c, p, action, role, actor)
    if action == "convert" and config["recruitment_channel_id"] is None:
        raise conversion.error("recruitment_channel_not_configured")
    row = await repository.lock_recruitment(c, p)
    if row is None:
        if action == "dm":
            raise ApiError(404, "recruitment_not_found", "요청한 모집을 찾을 수 없습니다.")
        raise ApiError(404, "not_found", "요청한 대상을 찾을 수 없습니다.")
    if action == "convert":
        result = await conversion.convert(
            c, {**p, "recruitment_channel": config["recruitment_channel_id"]}, row
        )
    elif action == "series":
        result = await create_series(c, {**p, "entry_channel": config["entry_channel_id"]}, row)
    elif action == "bind":
        assert actor is not None
        if actor.id != row["created_by"]:
            await require_operator(c, p, role, actor)
        if row["status"] not in {"OPEN", "CLOSED"}:
            raise ApiError(
                409,
                "invalid_recruitment_state",
                "현재 모집 상태에서는 이 작업을 수행할 수 없습니다.",
                {"currentStatus": row["status"]},
            )
        before = await recruitment_snapshot(c, p["guild"], p["id"])
        await repository.bind_message(c, p)
        result = await recruitment_snapshot(c, p["guild"], p["id"])
        await audit(c, p, "RECRUITMENT_BIND_MESSAGE", "RECRUITMENT", str(p["id"]), before, result)
        await invalidate(
            c, p, "RECRUITMENT", str(p["id"]), row["revision"] + 1, "RECRUITMENT_MESSAGE_BOUND"
        )
    else:
        if row["status"] != "CLOSED":
            raise ApiError(
                409,
                "participant_confirmation_dm_unavailable",
                "마감된 모집에서만 참가 확정 DM을 발송할 수 있습니다.",
                {"currentStatus": row["status"]},
            )
        snapshot = await recruitment_snapshot(c, p["guild"], p["id"])
        result = {"queuedRecipientCount": 0, "alreadyQueuedRecipientCount": 0}
        for participant in snapshot["participants"]:
            recipient = participant["id"]
            if not is_snowflake(recipient):
                continue
            enqueued = await notification(
                c,
                p,
                "RECRUITMENT_PARTICIPANT_CONFIRMATION_DM_CREATE",
                recipient,
                f"recruitment:{p['id']}:participant-confirmation-dm:{row['revision']}:{recipient}",
                {
                    "schemaVersion": 1,
                    "guildId": p["guild"],
                    "recipientUserId": recipient,
                    "recruitmentId": str(p["id"]),
                    "recruitmentTitle": snapshot["recruitment"]["title"],
                    "recruitmentHref": snapshot["links"]["entry"],
                },
            )
            result["queuedRecipientCount" if enqueued else "alreadyQueuedRecipientCount"] += 1
        await audit(
            c,
            p,
            "RECRUITMENT_PARTICIPANT_CONFIRMATION_DMS_REQUESTED",
            "RECRUITMENT",
            str(p["id"]),
            None,
            result,
        )
    await complete(c, p, result, 201 if action in {"series", "convert"} else 200)
    return result
