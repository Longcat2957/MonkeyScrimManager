import hashlib
import json
import uuid
from datetime import UTC, datetime, timedelta
from typing import Any

from fastapi import APIRouter, Request
from pydantic import ValidationError
from starlette.responses import Response

from monkeybot_api.api.authentication import command_principal, require_bot
from monkeybot_api.api.body import read_json
from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.requests import command_json, command_key
from monkeybot_api.core.config import Settings, is_snowflake
from monkeybot_api.core.errors import ApiError, success
from monkeybot_api.core.security import require_csrf
from monkeybot_api.db.database import Database
from monkeybot_api.db.transactions import mutation
from monkeybot_api.schemas.bot import BOT_ID, invalid_bot, validate_actor
from monkeybot_api.schemas.common import user_id
from monkeybot_api.schemas.recruitments.members import (
    AddMember,
    BotAdd,
    BotRemove,
    PreferredLanes,
    RemoveMember,
)
from monkeybot_api.services.recruitments.members import mutate

router = APIRouter()


async def execute(
    request: Request,
    guild_id: str,
    recruitment_id: str,
    member_id: str | None,
    preferred: bool = False,
    *,
    settings: Settings,
    database: Database,
) -> Response:
    joining = request.method == "POST"
    try:
        if not is_snowflake(guild_id):
            raise ValueError("invalid guild")
        record_id = uuid.UUID(recruitment_id)
        if member_id is not None:
            member_id = user_id(member_id)
    except ValueError:
        raise ApiError.from_status(400) from None
    lanes: list[str] | None = None
    if preferred:
        require_bot(request, settings)
        parsed = await read_json(request, PreferredLanes)
        if (
            not parsed.preferredLanes
            or len(set(parsed.preferredLanes)) != len(parsed.preferredLanes)
            or len(parsed.preferredLanes) > 5
        ):
            raise ApiError(
                422,
                "invalid_preferred_lanes",
                "선호 라인은 중복 없이 1개 이상 선택해야 합니다.",
            )
        bot, principal = True, None
        lanes = list(parsed.preferredLanes)
        raw = {"actor": parsed.actor.model_dump()}
    else:
        bot, principal = await command_principal(request, settings, database)
        raw = await command_json(request)
    actor, participant = None, None
    try:
        if bot:
            if joining:
                bot_command = BotAdd.model_validate(raw)
                actor, participant = bot_command.actor, bot_command.member
                member_id = participant.id
            else:
                actor = BotRemove.model_validate(raw).actor
        elif joining:
            member_id = AddMember.model_validate(raw).memberId
        else:
            RemoveMember.model_validate(raw)
    except ValidationError:
        if bot:
            raise invalid_bot("body") from None
        raise ApiError(
            422,
            "invalid_recruitment_member_request",
            "모집 참가자 변경 요청 형식이 올바르지 않습니다.",
        ) from None
    if actor is not None:
        validate_actor(actor)
        if participant is not None:
            validate_actor(participant)
    else:
        assert principal is not None
        require_csrf(request, settings, principal["csrf_secret_hash"])
    key = command_key(request, bot)
    assert member_id is not None
    suffix = "" if joining else ("/{userId}" if bot else "/{memberId}")
    route = "/api/guilds/{guildId}/recruitments/{id}/members" + suffix
    if preferred:
        route += "/preferred-lanes"
    command: dict[str, Any]
    if actor is not None:
        command = {
            "type": "JOIN" if joining else "LEAVE",
            "actor": actor.model_dump(),
            "recruitment_id": str(record_id),
        }
        if preferred:
            command["type"] = "SET_PREFERRED_LANES"
            command["user_id"] = member_id
            command["lanes"] = lanes
        elif participant is not None:
            command["participant"] = participant.model_dump()
        else:
            command["user_id"] = member_id
    else:
        command = {
            "AddMember" if joining else "RemoveMember": {
                "recruitment_id": str(record_id),
                "member_id": member_id,
            }
        }
    actor_id = actor.id if actor is not None else principal["user_id"] if principal else ""
    now = datetime.now(UTC)
    params = {
        "guild": guild_id,
        "id": record_id,
        "member": member_id,
        "actor_id": actor_id,
        "method": request.method,
        "route": route,
        "key": key,
        "principal_type": "BOT" if bot else "USER",
        "principal": BOT_ID if bot else actor_id,
        "hash": hashlib.sha256(
            json.dumps([guild_id, command], ensure_ascii=False, separators=(",", ":")).encode()
        ).digest(),
        "now": now,
        "expires": now + timedelta(hours=24),
        "idem_id": uuid.uuid7(),
        "audit_id": uuid.uuid7(),
        "event_id": uuid.uuid7(),
        "notification_id": uuid.uuid7(),
        "request_id": request.state.request_id,
        "action": ("RECRUITMENT_JOIN" if joining else "RECRUITMENT_LEAVE")
        if bot
        else ("RECRUITMENT_MEMBER_ADDED" if joining else "RECRUITMENT_MEMBER_REMOVED"),
        "event_type": (
            "RECRUITMENT_PARTICIPANT_JOINED" if joining else "RECRUITMENT_PARTICIPANT_LEFT"
        )
        if bot
        else ("RECRUITMENT_MEMBER_ADDED" if joining else "RECRUITMENT_MEMBER_REMOVED"),
    }
    if preferred:
        params["action"] = "RECRUITMENT_SET_PREFERRED_LANES"
        params["event_type"] = "RECRUITMENT_PREFERRED_LANES_SET"
    snapshot = await mutation(
        database, lambda connection: mutate(connection, params, actor, participant, joining, lanes)
    )
    return success(request, snapshot)


@router.post("/api/guilds/{guild_id}/recruitments/{recruitment_id}/members")
async def join(
    request: Request,
    guild_id: str,
    recruitment_id: str,
    *,
    settings: SettingsDep,
    database: DatabaseDep,
) -> Response:
    return await execute(
        request, guild_id, recruitment_id, None, settings=settings, database=database
    )


@router.delete("/api/guilds/{guild_id}/recruitments/{recruitment_id}/members/{member_id}")
async def leave(
    request: Request,
    guild_id: str,
    recruitment_id: str,
    member_id: str,
    *,
    settings: SettingsDep,
    database: DatabaseDep,
) -> Response:
    return await execute(
        request, guild_id, recruitment_id, member_id, settings=settings, database=database
    )


@router.put(
    "/api/guilds/{guild_id}/recruitments/{recruitment_id}/members/{member_id}/preferred-lanes"
)
async def preferred_lanes(
    request: Request,
    guild_id: str,
    recruitment_id: str,
    member_id: str,
    *,
    settings: SettingsDep,
    database: DatabaseDep,
) -> Response:
    return await execute(
        request,
        guild_id,
        recruitment_id,
        member_id,
        preferred=True,
        settings=settings,
        database=database,
    )
