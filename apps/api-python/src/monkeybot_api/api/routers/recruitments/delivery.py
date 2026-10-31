import hashlib
import json
import uuid
from datetime import UTC, datetime, timedelta
from typing import Any

from fastapi import APIRouter, Request
from starlette.responses import Response

import monkeybot_api.services.recruitments.conversion as conversion
from monkeybot_api.api.authentication import require_bot
from monkeybot_api.api.body import read_json
from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.requests import command_key
from monkeybot_api.api.sessions import resolve_session
from monkeybot_api.core.config import Settings, is_snowflake
from monkeybot_api.core.errors import ApiError, success
from monkeybot_api.core.security import require_csrf
from monkeybot_api.db.database import Database
from monkeybot_api.db.transactions import mutation
from monkeybot_api.schemas.bot import BOT_ID, validate_actor
from monkeybot_api.schemas.recruitments.conversion import Conversion
from monkeybot_api.schemas.recruitments.creation import canonical_start
from monkeybot_api.schemas.recruitments.delivery import Action, BindMessage, SeriesCreate
from monkeybot_api.schemas.recruitments.members import RemoveMember
from monkeybot_api.services.recruitments.delivery import mutate

router = APIRouter()


async def execute(
    request: Request,
    guild: str,
    record: str | None,
    action: Action,
    *,
    settings: Settings,
    database: Database,
) -> Response:
    try:
        if not is_snowflake(guild):
            raise ValueError("invalid guild")
        record_id = uuid.UUID(record) if record is not None else None
    except ValueError:
        raise ApiError.from_status(400) from None
    command: dict[str, Any]
    actor, expected, channel, message = None, None, None, None
    start = None
    if action == "bind":
        require_bot(request, settings)
        body = await read_json(request, BindMessage)
        actor, channel, message = body.actor, body.channelId, body.messageId
        validate_actor(actor)
        actor_id = actor.id
        command = {
            "type": "BIND_MESSAGE",
            "actor": actor.model_dump(),
            "recruitment_id": str(record_id),
            "channel_id": channel,
            "message_id": message,
        }
    else:
        principal = await resolve_session(request, database, settings)
        actor_id = principal["user_id"]
        if action == "convert":
            converted = await read_json(request, Conversion)
            expected = converted.expectedRevision
            if expected < 0:
                raise conversion.error("invalid_entry_auction_conversion_request")
            start, canonical = canonical_start(converted.expectedStartAt)
            command = {
                "sourceRecruitmentId": str(record_id),
                "expectedRevision": expected,
                "expectedStartAt": canonical,
            }
        elif action == "series":
            creation = await read_json(request, SeriesCreate)
            record_id, expected = (
                uuid.UUID(creation.sourceRecruitmentId),
                creation.expectedRevision,
            )
            if expected < 0:
                raise ApiError(
                    422,
                    "invalid_series_create_request",
                    "시리즈 시작 요청 형식이 올바르지 않습니다.",
                )
            command = {"sourceRecruitmentId": str(record_id), "expectedRevision": expected}
        else:
            await read_json(request, RemoveMember)
            command = {"recruitment_id": str(record_id)}
        require_csrf(request, settings, principal["csrf_secret_hash"])
    key = command_key(request, actor is not None)
    route = (
        "/api/guilds/{guildId}/series"
        if action == "series"
        else (
            "/api/guilds/{guildId}/recruitments/{id}/"
            + {
                "bind": "message",
                "dm": "participant-confirmation-dms",
                "convert": "auction-recruitment",
            }[action]
        )
    )
    now = datetime.now(UTC)
    p = {
        "guild": guild,
        "id": record_id,
        "actor_id": actor_id,
        "expected": expected,
        "channel": channel,
        "message": message,
        "series_id": uuid.uuid7(),
        "auction_id": uuid.uuid7(),
        "start": start,
        "method": request.method,
        "route": route,
        "key": key,
        "principal_type": "BOT" if actor else "USER",
        "principal": BOT_ID if actor else actor_id,
        "hash": hashlib.sha256(
            json.dumps([guild, command], ensure_ascii=False, separators=(",", ":")).encode()
        ).digest(),
        "now": now,
        "expires": now + timedelta(hours=24),
        "idem_id": uuid.uuid7(),
        "audit_id": uuid.uuid7(),
        "request_id": request.state.request_id,
    }
    result = await mutation(database, lambda connection: mutate(connection, p, action, actor))
    response = success(request, result)
    response.status_code = 201 if action in {"series", "convert"} else 200
    return response


@router.put("/api/guilds/{guild_id}/recruitments/{recruitment_id}/message")
async def bind(
    request: Request,
    guild_id: str,
    recruitment_id: str,
    *,
    settings: SettingsDep,
    database: DatabaseDep,
) -> Response:
    return await execute(
        request, guild_id, recruitment_id, "bind", settings=settings, database=database
    )


@router.post("/api/guilds/{guild_id}/recruitments/{recruitment_id}/participant-confirmation-dms")
async def dm(
    request: Request,
    guild_id: str,
    recruitment_id: str,
    *,
    settings: SettingsDep,
    database: DatabaseDep,
) -> Response:
    return await execute(
        request, guild_id, recruitment_id, "dm", settings=settings, database=database
    )


@router.post("/api/guilds/{guild_id}/series")
async def series(
    request: Request, guild_id: str, *, settings: SettingsDep, database: DatabaseDep
) -> Response:
    return await execute(request, guild_id, None, "series", settings=settings, database=database)


@router.post("/api/guilds/{guild_id}/recruitments/{recruitment_id}/auction-recruitment")
async def convert(
    request: Request,
    guild_id: str,
    recruitment_id: str,
    *,
    settings: SettingsDep,
    database: DatabaseDep,
) -> Response:
    return await execute(
        request, guild_id, recruitment_id, "convert", settings=settings, database=database
    )
