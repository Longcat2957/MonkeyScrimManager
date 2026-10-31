from typing import Any

from fastapi import APIRouter, Request
from pydantic import ValidationError
from starlette.responses import Response

from monkeybot_api.api.authentication import command_principal, require_bot
from monkeybot_api.api.body import read_json
from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.paths import path
from monkeybot_api.api.requests import command_json, command_key
from monkeybot_api.core.config import Settings
from monkeybot_api.core.errors import success
from monkeybot_api.core.security import require_csrf
from monkeybot_api.core.text import trim
from monkeybot_api.db.database import Database
from monkeybot_api.schemas.auctions.cancellation import BotCancel, Cancel, error
from monkeybot_api.schemas.bot import Actor, invalid_bot, validate_actor
from monkeybot_api.schemas.recruitments.members import BotRemove
from monkeybot_api.services.auctions import commands as workflows
from monkeybot_api.services.auctions import reads

BASE = "/api/guilds/{guild_id}/auction-tournaments/{tournament_id}"


router = APIRouter()


@router.api_route(BASE + "/cancel-preview", methods=["GET", "HEAD"])
async def cancel_preview(
    request: Request,
    guild_id: str,
    tournament_id: str,
    *,
    settings: SettingsDep,
    database: DatabaseDep,
) -> Response:
    identifier = path(guild_id, tournament_id)
    assert identifier is not None
    bot, principal = await command_principal(request, settings, database)
    viewer = None if bot else principal["user_id"] if principal else None
    result = await reads.cancel_preview(database, guild_id, identifier, viewer)
    return success(request, result)


async def write(
    request: Request,
    guild: str,
    identifier: str,
    force: bool,
    *,
    settings: Settings,
    database: Database,
) -> Response:
    record = path(guild, identifier)
    actor: Actor | None = None
    fields: dict[str, Any] = {}
    if force:
        require_bot(request, settings)
        bot, principal = True, None
        actor = (await read_json(request, BotRemove)).actor
    else:
        bot, principal = await command_principal(request, settings, database)
        raw = await command_json(request)
        try:
            body = (BotCancel if bot else Cancel).model_validate(raw)
        except ValidationError:
            raise (
                invalid_bot("body") if bot else error("invalid_auction_tournament_cancel_request")
            ) from None
        if body.expectedRevision < 0:
            raise (
                invalid_bot("expectedRevision")
                if bot
                else error("invalid_auction_tournament_cancel_request")
            )
        # Axum uses the Bot validation response for invalid reason text in either branch.
        if not trim(body.reason) or len(body.reason) > 240:
            raise invalid_bot("reason")
        fields = {"expected": body.expectedRevision, "reason": trim(body.reason)}
        if isinstance(body, BotCancel):
            actor = body.actor
    if not bot:
        assert principal is not None
        require_csrf(request, settings, principal["csrf_secret_hash"])
    key = command_key(request, bot)
    if actor is not None:
        validate_actor(actor)
    actor_id = actor.id if actor is not None else principal["user_id"] if principal else ""
    result = await workflows.cancel(
        database,
        guild=guild,
        record=record,
        force=force,
        bot=bot,
        fields=fields,
        actor=actor,
        actor_id=actor_id,
        key=key,
        request_id=request.state.request_id,
    )
    return success(request, result)


@router.post(BASE + "/cancel")
async def cancel(
    request: Request,
    guild_id: str,
    tournament_id: str,
    *,
    settings: SettingsDep,
    database: DatabaseDep,
) -> Response:
    return await write(
        request, guild_id, tournament_id, False, settings=settings, database=database
    )


@router.post("/api/guilds/{guild_id}/auction-recruitments/{recruitment_id}/force-cancel")
async def force_cancel(
    request: Request,
    guild_id: str,
    recruitment_id: str,
    *,
    settings: SettingsDep,
    database: DatabaseDep,
) -> Response:
    return await write(
        request, guild_id, recruitment_id, True, settings=settings, database=database
    )
