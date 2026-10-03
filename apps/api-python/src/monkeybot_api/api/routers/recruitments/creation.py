import uuid
from datetime import UTC, datetime, timedelta

from fastapi import APIRouter, Request
from pydantic import ValidationError
from starlette.responses import Response

from monkeybot_api.api.authentication import command_principal
from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.requests import command_json, command_key
from monkeybot_api.core.config import is_snowflake
from monkeybot_api.core.errors import ApiError, success
from monkeybot_api.core.security import require_csrf
from monkeybot_api.db.transactions import mutation
from monkeybot_api.schemas.bot import BOT_ID, invalid_bot, validate_actor
from monkeybot_api.schemas.recruitments.creation import BotCreate, BrowserCreate, canonical_start
from monkeybot_api.services.recruitments.creation import ROUTE, create_attempt, request_hash

router = APIRouter()


@router.post("/api/guilds/{guild_id}/recruitments")
async def create(
    request: Request, guild_id: str, *, settings: SettingsDep, database: DatabaseDep
) -> Response:
    if not is_snowflake(guild_id):
        raise ApiError.from_status(400)
    bot, principal = await command_principal(request, settings, database)
    raw = await command_json(request)
    try:
        command = BotCreate.model_validate(raw) if bot else BrowserCreate.model_validate(raw)
    except ValidationError:
        if bot:
            raise invalid_bot("body") from None
        raise ApiError(
            422,
            "invalid_recruitment_create_request",
            "내전 생성 요청 형식이 올바르지 않습니다.",
        ) from None
    start, canonical = canonical_start(command.expectedStartAt)
    actor = command.actor if isinstance(command, BotCreate) else None
    if actor is not None:
        validate_actor(actor)
    else:
        assert principal is not None
        require_csrf(request, settings, principal["csrf_secret_hash"])
    key = command_key(request, bot)
    best = command.bestOf if command.bestOf is not None else 3
    fearless = command.hardFearless if command.hardFearless is not None else True
    if best not in {1, 3, 5}:
        raise ApiError(
            422,
            "invalid_recruitment_best_of",
            "시리즈 형식은 BO1, BO3 또는 BO5만 선택할 수 있습니다.",
            {"field": "bestOf", "allowedValues": [1, 3, 5], "receivedValue": best},
        )
    actor_id = actor.id if actor is not None else principal["user_id"] if principal else ""
    now = datetime.now(UTC)
    params = {
        "guild": guild_id,
        "id": uuid.uuid7(),
        "actor_id": actor_id,
        "best_of": best,
        "fearless": fearless,
        "now": now,
        "expires": now + timedelta(hours=24),
        "idem_id": uuid.uuid7(),
        "audit_id": uuid.uuid7(),
        "event_id": uuid.uuid7(),
        "notification_id": uuid.uuid7(),
        "principal_type": "BOT" if bot else "USER",
        "principal": BOT_ID if bot else actor_id,
        "route": ROUTE,
        "key": key,
        "request_id": request.state.request_id,
        "hash": request_hash(
            guild_id, {"bestOf": best, "hardFearless": fearless}, canonical, actor
        ),
    }
    snapshot = await mutation(
        database, lambda connection: create_attempt(connection, params, actor, start)
    )
    response = success(request, snapshot)
    response.status_code = 201
    return response
