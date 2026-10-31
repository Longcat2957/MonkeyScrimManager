import hashlib
import json
import uuid
from datetime import UTC, datetime, timedelta
from typing import Any

from fastapi import APIRouter, Request
from pydantic import ValidationError
from starlette.responses import Response

from monkeybot_api.api.authentication import command_principal
from monkeybot_api.api.body import read_json
from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.requests import command_json, command_key
from monkeybot_api.api.sessions import resolve_session
from monkeybot_api.core.config import Settings, is_snowflake
from monkeybot_api.core.errors import ApiError, success
from monkeybot_api.core.security import require_csrf
from monkeybot_api.db.database import Database
from monkeybot_api.db.transactions import mutation
from monkeybot_api.schemas.bot import BOT_ID, invalid_bot, validate_actor
from monkeybot_api.schemas.common import Revision
from monkeybot_api.schemas.recruitments.lifecycle import ENTRY_ACTIONS, Action, EditRevision
from monkeybot_api.schemas.recruitments.members import BotRemove, RemoveMember
from monkeybot_api.services.recruitments.lifecycle import mutate
from monkeybot_api.services.recruitments.queries import get_entry_draft

router = APIRouter()


async def execute(
    request: Request,
    guild_id: str,
    recruitment_id: str,
    action: Action,
    *,
    settings: Settings,
    database: Database,
) -> Response:
    try:
        if not is_snowflake(guild_id):
            raise ValueError("invalid guild")
        record_id = uuid.UUID(recruitment_id)
    except ValueError:
        raise ApiError.from_status(400) from None
    actor, expected = None, None
    operation = None
    if action == "Close":
        bot, principal = await command_principal(request, settings, database)
        raw = await command_json(request)
        try:
            if bot:
                actor = BotRemove.model_validate(raw).actor
            else:
                RemoveMember.model_validate(raw)
        except ValidationError:
            if bot:
                raise invalid_bot("body") from None
            raise ApiError(
                422,
                "invalid_recruitment_close_request",
                "엔트리 편성 시작 요청 형식이 올바르지 않습니다.",
            ) from None
        if actor is not None:
            validate_actor(actor)
    else:
        # These Axum routes accept sessions even when Authorization is present.
        bot = False
        principal = await resolve_session(request, database, settings)
        if action == "Apply":
            parsed = await read_json(request, EditRevision)
            expected = parsed.expectedRevision
            operation = parsed.operation.model_dump()
        else:
            expected = (await read_json(request, Revision)).expectedRevision
        if expected < 0:
            raise ApiError(
                422,
                "invalid_entry_draft_request"
                if action in ENTRY_ACTIONS
                else "invalid_recruitment_lifecycle_request",
                "엔트리 편성 요청 형식이 올바르지 않습니다."
                if action in ENTRY_ACTIONS
                else "모집 취소 또는 복구 요청 형식이 올바르지 않습니다.",
            )
    if not bot:
        assert principal is not None
        require_csrf(request, settings, principal["csrf_secret_hash"])
    key = command_key(request, bot)
    fields: dict[str, Any] = {"recruitment_id": str(record_id)}
    if expected is not None:
        fields["expected_revision"] = expected
    if operation is not None:
        fields["operation"] = operation
    command: dict[str, Any] = (
        {"type": "CLOSE", "actor": actor.model_dump(), **fields}
        if actor is not None
        else {action: fields}
    )
    actor_id = actor.id if actor is not None else principal["user_id"] if principal else ""
    now = datetime.now(UTC)
    suffix = "" if action == "Cancel" else "/" + action.lower()
    if action in {"Apply", "Reset"}:
        suffix = "/entry-draft" + ("/reset" if action == "Reset" else "")
    params = {
        "operation": operation,
        "guild": guild_id,
        "id": record_id,
        "actor_id": actor_id,
        "expected": expected,
        "method": request.method,
        "route": "/api/guilds/{guildId}/recruitments/{id}" + suffix,
        "principal_type": "BOT" if bot else "USER",
        "principal": BOT_ID if bot else actor_id,
        "key": key,
        "hash": hashlib.sha256(
            json.dumps([guild_id, command], ensure_ascii=False, separators=(",", ":")).encode()
        ).digest(),
        "now": now,
        "expires": now + timedelta(hours=24),
        "idem_id": uuid.uuid7(),
        "audit_id": uuid.uuid7(),
        "event_id": uuid.uuid7(),
        "recruitment_event_id": uuid.uuid7(),
        "notification_id": uuid.uuid7(),
        "request_id": request.state.request_id,
        "audit_action": "RECRUITMENT_CLOSE"
        if action == "Close"
        else "RECRUITMENT_"
        + {
            "Cancel": "CANCELLED",
            "Restore": "RESTORED",
            "Reopen": "REOPENED",
            "Apply": "ENTRY_DRAFT_UPDATED",
            "Reset": "ENTRY_DRAFT_RESET",
        }[action],
        "event_type": "RECRUITMENT_"
        + {
            "Close": "CLOSED",
            "Cancel": "CANCELLED",
            "Restore": "RESTORED",
            "Reopen": "REOPENED",
            "Apply": "ENTRY_DRAFT_UPDATED",
            "Reset": "ENTRY_DRAFT_RESET",
        }[action],
    }
    snapshot = await mutation(
        database, lambda connection: mutate(connection, params, action, actor)
    )
    return success(request, snapshot)


@router.post("/api/guilds/{guild_id}/recruitments/{recruitment_id}/close")
async def close(
    request: Request,
    guild_id: str,
    recruitment_id: str,
    *,
    settings: SettingsDep,
    database: DatabaseDep,
) -> Response:
    return await execute(
        request, guild_id, recruitment_id, "Close", settings=settings, database=database
    )


@router.delete("/api/guilds/{guild_id}/recruitments/{recruitment_id}")
async def cancel(
    request: Request,
    guild_id: str,
    recruitment_id: str,
    *,
    settings: SettingsDep,
    database: DatabaseDep,
) -> Response:
    return await execute(
        request, guild_id, recruitment_id, "Cancel", settings=settings, database=database
    )


@router.post("/api/guilds/{guild_id}/recruitments/{recruitment_id}/restore")
async def restore(
    request: Request,
    guild_id: str,
    recruitment_id: str,
    *,
    settings: SettingsDep,
    database: DatabaseDep,
) -> Response:
    return await execute(
        request, guild_id, recruitment_id, "Restore", settings=settings, database=database
    )


@router.post("/api/guilds/{guild_id}/recruitments/{recruitment_id}/reopen")
async def reopen(
    request: Request,
    guild_id: str,
    recruitment_id: str,
    *,
    settings: SettingsDep,
    database: DatabaseDep,
) -> Response:
    return await execute(
        request, guild_id, recruitment_id, "Reopen", settings=settings, database=database
    )


@router.patch("/api/guilds/{guild_id}/recruitments/{recruitment_id}/entry-draft")
async def edit(
    request: Request,
    guild_id: str,
    recruitment_id: str,
    *,
    settings: SettingsDep,
    database: DatabaseDep,
) -> Response:
    return await execute(
        request, guild_id, recruitment_id, "Apply", settings=settings, database=database
    )


@router.post("/api/guilds/{guild_id}/recruitments/{recruitment_id}/entry-draft/reset")
async def reset(
    request: Request,
    guild_id: str,
    recruitment_id: str,
    *,
    settings: SettingsDep,
    database: DatabaseDep,
) -> Response:
    return await execute(
        request, guild_id, recruitment_id, "Reset", settings=settings, database=database
    )


@router.api_route(
    "/api/guilds/{guild_id}/recruitments/{recruitment_id}/entry-draft", methods=["GET", "HEAD"]
)
async def entry(
    request: Request,
    guild_id: str,
    recruitment_id: str,
    *,
    settings: SettingsDep,
    database: DatabaseDep,
) -> Response:
    try:
        if not is_snowflake(guild_id):
            raise ValueError("invalid guild")
        record_id = uuid.UUID(recruitment_id)
    except ValueError:
        raise ApiError.from_status(400) from None
    principal = await resolve_session(request, database, settings)
    return success(
        request,
        await get_entry_draft(
            database,
            guild_id,
            principal["user_id"],
            record_id,
        ),
    )
