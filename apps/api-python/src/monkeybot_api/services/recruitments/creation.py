import base64
import hashlib
import json
from datetime import datetime, timedelta, timezone
from typing import Any

from sqlalchemy.ext.asyncio import AsyncConnection

import monkeybot_api.repositories.recruitments.creation as repository
from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.time import timestamp
from monkeybot_api.repositories.idempotency import acquire
from monkeybot_api.repositories.members import sync_actor
from monkeybot_api.schemas.bot import Actor

ROUTE = "/api/guilds/{guildId}/recruitments"


def request_hash(guild_id: str, rules: dict[str, Any], start: str, actor: Actor | None) -> bytes:
    command: dict[str, Any] = {}
    if actor is not None:
        command = {"type": "CREATE", "actor": actor.model_dump()}
    command.update({"rules": rules, "expected_start_at": start})
    canonical = json.dumps([guild_id, command], ensure_ascii=False, separators=(",", ":"))
    return hashlib.sha256(canonical.encode()).digest()


async def create_attempt(
    connection: AsyncConnection, params: dict[str, Any], actor: Actor | None, start: datetime
) -> dict[str, Any]:
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
    config = await repository.lock_config(connection, params)
    if config is None:
        raise ApiError(409, "guild_setup_required", "길드 초기 설정을 먼저 완료해야 합니다.")
    replay = await acquire(connection, params, actor is not None)
    if replay is not None:
        return replay
    if actor is not None:
        await sync_actor(connection, actor, params)
    membership = await repository.lock_actor_membership(connection, params)
    if membership != "ACTIVE":
        raise ApiError(
            403,
            "guild_membership_required",
            "해당 길드의 활성 멤버만 이 작업을 수행할 수 있습니다."
            if actor is not None
            else "해당 길드의 활성 멤버만 내전을 만들 수 있습니다.",
        )
    channel = config["recruitment_channel_id"] if actor is None else None
    if actor is None and channel is None:
        raise ApiError(
            409,
            "recruitment_channel_not_configured",
            "Discord 모집 채널을 먼저 설정해야 내전을 만들 수 있습니다.",
        )
    season = await repository.active_season(connection, params)
    if season is None:
        if actor is not None:
            raise ApiError(409, "guild_setup_required", "길드 초기 설정을 먼저 완료해야 합니다.")
        raise ApiError(
            409, "no_active_season", "길드에 진행 중인 시즌이 없어 내전을 만들 수 없습니다."
        )
    number = await repository.next_display_number(connection, params)
    if number is None:
        raise ApiError.from_status(500)
    label = start.astimezone(timezone(timedelta(hours=9))).strftime("%Y-%m-%d %H:%M")
    title = f"일반 내전 모집 #{number} · 예상 시작 {label} (KST)"
    values = {**params, "season": season, "number": number, "title": title}
    await repository.insert_recruitment(connection, values)
    await repository.insert_entry_draft(connection, values)
    snapshot = {
        "recruitment": {
            "id": str(params["id"]),
            "displayNumber": number,
            "title": title,
            "status": "OPEN",
            "targetCount": 10,
            "bestOf": params["best_of"],
            "hardFearless": params["fearless"],
            "revision": 0,
            "channelId": None,
            "messageId": None,
            "createdById": params["actor_id"],
            "updatedAt": timestamp(params["now"]),
        },
        "participants": [],
        "links": {"entry": f"/g/{params['guild']}/recruitments/{params['id']}/entry"},
    }
    values.update(snapshot=json.dumps(snapshot, ensure_ascii=False), target=str(params["id"]))
    if channel is not None:
        notification = {
            "schemaVersion": 1,
            "recruitmentId": str(params["id"]),
            "snapshot": snapshot,
        }
        await repository.enqueue_initial_message(
            connection,
            {
                **values,
                "dedup": f"recruitment:{params['id']}:initial-message",
                "nonce": base64.urlsafe_b64encode(params["notification_id"].bytes)
                .decode()
                .rstrip("="),
                "channel": channel,
                "payload": json.dumps(notification, ensure_ascii=False),
            },
        )
    await repository.insert_audit(connection, values)
    await repository.insert_event(
        connection,
        {
            **values,
            "topic": f"guild:{params['guild']}:recruitment:{params['id']}",
            "event": json.dumps(
                {"type": "invalidate", "resourceId": str(params["id"]), "revision": 0}
            ),
        },
    )
    completed = await repository.complete_creation(connection, values)
    if completed is None:
        raise ApiError.from_status(500)
    return snapshot
