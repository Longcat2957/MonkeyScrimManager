from typing import Any

from fastapi import APIRouter, Request
from pydantic import ValidationError
from starlette.responses import Response

import monkeybot_api.schemas.auctions.recruitments as contracts
from monkeybot_api.api.authentication import command_principal
from monkeybot_api.api.body import read_json
from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.paths import path
from monkeybot_api.api.requests import command_json, command_key
from monkeybot_api.api.routers.recruitments.queries import actionable_query
from monkeybot_api.api.sessions import resolve_session
from monkeybot_api.core.config import Settings
from monkeybot_api.core.errors import ApiError, success
from monkeybot_api.core.security import require_csrf
from monkeybot_api.db.database import Database
from monkeybot_api.schemas.bot import Actor, invalid_bot, validate_actor
from monkeybot_api.schemas.common import Revision, user_id
from monkeybot_api.schemas.recruitments.creation import canonical_start
from monkeybot_api.schemas.recruitments.members import BotRemove, RemoveMember
from monkeybot_api.services.auctions import commands as workflows
from monkeybot_api.services.auctions import reads

BASE = "/api/guilds/{guild_id}/auction-recruitments"
ITEM = BASE + "/{recruitment_id}"


router = APIRouter()


async def write(
    request: Request,
    guild: str,
    identifier: str | None,
    action: str,
    target: str | None = None,
    *,
    settings: Settings,
    database: Database,
) -> Response:
    record = path(guild, identifier)
    if target is not None:
        try:
            target = user_id(target)
        except ValueError:
            raise ApiError.from_status(400) from None
    actor: Actor | None = None
    participant: Actor | None = None
    fields: dict[str, Any] = {}
    if action in {"create", "join", "leave"}:
        bot, principal = await command_principal(request, settings, database)
        raw = await command_json(request)
        try:
            if action == "create":
                body = (contracts.BotCreate if bot else contracts.Create).model_validate(raw)
                if isinstance(body, contracts.BotCreate):
                    actor = body.actor
                start, canonical = canonical_start(body.expectedStartAt)
                fields.update(
                    format=body.format,
                    rules=contracts.rules(body),
                    start=start,
                    canonical=canonical,
                )
            elif action == "join":
                if bot:
                    addition = contracts.BotAdd.model_validate(raw)
                    actor = addition.actor
                    participant = addition.member or actor.model_copy(deep=True)
                    target = participant.id
                    fields["lane"] = addition.declaredLane
                else:
                    member = contracts.Add.model_validate(raw)
                    target, fields["lane"] = member.memberId, member.declaredLane
            elif bot:
                actor = BotRemove.model_validate(raw).actor
            else:
                RemoveMember.model_validate(raw)
        except ValidationError:
            raise invalid_bot("body") if bot else contracts.invalid(action) from None
    else:
        bot = False
        principal = await resolve_session(request, database, settings)
        if action == "setup":
            setup = await read_json(request, contracts.Setup)
            fields.update(
                expected=setup.expectedRevision, teams=[t.model_dump() for t in setup.teams]
            )
        elif action in {"reopen", "cancel", "restore"}:
            fields["expected"] = (await read_json(request, Revision)).expectedRevision
        else:
            await read_json(request, RemoveMember)
        if fields.get("expected", 0) < 0:
            raise contracts.invalid(action)
    if not bot:
        assert principal is not None
        require_csrf(request, settings, principal["csrf_secret_hash"])
    key = command_key(request, bot)
    if actor is not None:
        validate_actor(actor)
    if participant is not None:
        validate_actor(participant)
    actor_id = actor.id if actor is not None else principal["user_id"] if principal else ""
    result = await workflows.recruitment(
        database,
        guild=guild,
        record=record,
        action=action,
        target=target,
        actor=actor,
        participant=participant,
        fields=fields,
        bot=bot,
        actor_id=actor_id,
        key=key,
        method=request.method,
        request_id=request.state.request_id,
    )
    response = success(request, result)
    response.status_code = 201 if action == "create" else 200
    return response


@router.post(BASE)
async def create(
    request: Request, guild_id: str, *, settings: SettingsDep, database: DatabaseDep
) -> Response:
    return await write(request, guild_id, None, "create", settings=settings, database=database)


@router.post(ITEM + "/members")
async def join(
    request: Request,
    guild_id: str,
    recruitment_id: str,
    *,
    settings: SettingsDep,
    database: DatabaseDep,
) -> Response:
    return await write(
        request, guild_id, recruitment_id, "join", settings=settings, database=database
    )


@router.delete(ITEM + "/members/{user_id}")
async def leave(
    request: Request,
    guild_id: str,
    recruitment_id: str,
    user_id: str,
    *,
    settings: SettingsDep,
    database: DatabaseDep,
) -> Response:
    return await write(
        request, guild_id, recruitment_id, "leave", user_id, settings=settings, database=database
    )


@router.post(ITEM + "/close")
async def close(
    request: Request,
    guild_id: str,
    recruitment_id: str,
    *,
    settings: SettingsDep,
    database: DatabaseDep,
) -> Response:
    return await write(
        request, guild_id, recruitment_id, "close", settings=settings, database=database
    )


@router.post(ITEM + "/reopen")
async def reopen(
    request: Request,
    guild_id: str,
    recruitment_id: str,
    *,
    settings: SettingsDep,
    database: DatabaseDep,
) -> Response:
    return await write(
        request, guild_id, recruitment_id, "reopen", settings=settings, database=database
    )


@router.delete(ITEM)
async def cancel(
    request: Request,
    guild_id: str,
    recruitment_id: str,
    *,
    settings: SettingsDep,
    database: DatabaseDep,
) -> Response:
    return await write(
        request, guild_id, recruitment_id, "cancel", settings=settings, database=database
    )


@router.post(ITEM + "/restore")
async def restore(
    request: Request,
    guild_id: str,
    recruitment_id: str,
    *,
    settings: SettingsDep,
    database: DatabaseDep,
) -> Response:
    return await write(
        request, guild_id, recruitment_id, "restore", settings=settings, database=database
    )


@router.patch(ITEM + "/setup-draft")
async def setup(
    request: Request,
    guild_id: str,
    recruitment_id: str,
    *,
    settings: SettingsDep,
    database: DatabaseDep,
) -> Response:
    return await write(
        request, guild_id, recruitment_id, "setup", settings=settings, database=database
    )


@router.post(ITEM + "/participant-confirmation-dms")
async def dms(
    request: Request,
    guild_id: str,
    recruitment_id: str,
    *,
    settings: SettingsDep,
    database: DatabaseDep,
) -> Response:
    return await write(
        request, guild_id, recruitment_id, "dm", settings=settings, database=database
    )


async def read(
    request: Request,
    guild: str,
    identifier: str,
    draft: bool,
    *,
    settings: Settings,
    database: Database,
) -> Response:
    record = path(guild, identifier)
    assert record is not None
    principal = await resolve_session(request, database, settings)
    result = await reads.recruitment_detail(
        database, guild, record, principal["user_id"], draft=draft
    )
    return success(request, result)


@router.api_route(ITEM, methods=["GET", "HEAD"])
async def detail(
    request: Request,
    guild_id: str,
    recruitment_id: str,
    *,
    settings: SettingsDep,
    database: DatabaseDep,
) -> Response:
    return await read(
        request, guild_id, recruitment_id, False, settings=settings, database=database
    )


@router.api_route(ITEM + "/setup-draft", methods=["GET", "HEAD"])
async def draft(
    request: Request,
    guild_id: str,
    recruitment_id: str,
    *,
    settings: SettingsDep,
    database: DatabaseDep,
) -> Response:
    return await read(request, guild_id, recruitment_id, True, settings=settings, database=database)


@router.api_route(BASE, methods=["GET", "HEAD"])
async def listing(
    request: Request, guild_id: str, *, settings: SettingsDep, database: DatabaseDep
) -> Response:
    path(guild_id)
    query = actionable_query(request)
    principal = await resolve_session(request, database, settings)
    result = await reads.recruitment_list(database, guild_id, principal["user_id"], query)
    return success(request, result)
