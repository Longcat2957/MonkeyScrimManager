import hashlib
import json
import uuid
from datetime import UTC, datetime, timedelta
from typing import Any

from fastapi import APIRouter, Request
from pydantic import ValidationError
from sqlalchemy import text
from starlette.responses import Response

from monkeybot_api import auction_contracts as contracts
from monkeybot_api import auction_queries as sql
from monkeybot_api.auction_mutations import mutate
from monkeybot_api.auction_snapshot import rows, rules, setup_snapshot, shape, snapshot
from monkeybot_api.auth import require_csrf
from monkeybot_api.config import Settings, is_snowflake
from monkeybot_api.context import timestamp
from monkeybot_api.database import Database
from monkeybot_api.errors import ApiError, success
from monkeybot_api.guild_admin import mutation
from monkeybot_api.json_body import read_json
from monkeybot_api.recruitment_create import (
    BOT_ID,
    Actor,
    canonical_start,
    command_json,
    command_key,
    command_principal,
    invalid_bot,
    validate_actor,
)
from monkeybot_api.recruitment_lifecycle import Revision
from monkeybot_api.recruitment_members import BotRemove, RemoveMember, user_id
from monkeybot_api.session import resolve_session
from monkeybot_api.workflow import (
    actionable_query,
    decode_cursor,
    encode_cursor,
    require_guild_access,
)

BASE = "/api/guilds/{guild_id}/auction-recruitments"
ITEM = BASE + "/{recruitment_id}"


def path(guild: str, identifier: str | None = None) -> uuid.UUID | None:
    try:
        if not is_snowflake(guild):
            raise ValueError("guild")
        return uuid.UUID(identifier) if identifier is not None else None
    except ValueError:
        raise ApiError.from_status(400) from None


def router(settings: Settings, database: Database) -> APIRouter:
    routes = APIRouter()

    async def write(
        request: Request, guild: str, identifier: str | None, action: str, target: str | None = None
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
        if action == "create":
            contracts.validate_rules(fields["format"], fields["rules"])
            command = {
                "format": fields["format"],
                "rules": fields["rules"],
                "expectedStartAt": fields.pop("canonical"),
            }
            record = uuid.uuid7()
        elif action in {"join", "leave"}:
            if actor is not None:
                command = {"type": action.upper(), "actor": actor.model_dump()}
                if action == "join":
                    assert participant is not None
                    command["participant"] = participant.model_dump()
                command["recruitment_id"] = str(record)
                command["declared_lane" if action == "join" else "user_id"] = (
                    fields.get("lane") if action == "join" else target
                )
            else:
                payload = {"recruitment_id": str(record), "member_id": target}
                if action == "join":
                    payload["declared_lane"] = fields["lane"]
                command = {"AddMember" if action == "join" else "RemoveMember": payload}
        elif action == "setup":
            command = {
                "recruitmentId": str(record),
                "expectedRevision": fields["expected"],
                "teams": fields["teams"],
            }
        elif action == "dm":
            command = {"recruitment_id": str(record)}
        else:
            payload = {"recruitment_id": str(record)}
            if "expected" in fields:
                payload["expected_revision"] = fields["expected"]
            command = {action.capitalize(): payload}
        suffix = {
            "create": "",
            "join": "/members",
            "leave": "/members/{userId}" if bot else "/members/{memberId}",
            "cancel": "",
            "setup": "/setup-draft",
            "dm": "/participant-confirmation-dms",
        }.get(action, "/" + action)
        route = (
            "/api/guilds/{guildId}/auction-recruitments"
            + ("" if action == "create" else "/{id}")
            + suffix
        )
        now = datetime.now(UTC)
        p = {
            **fields,
            "guild": guild,
            "id": record,
            "action": action,
            "mode": "bot"
            if bot
            else "setup"
            if action == "setup"
            else "dm"
            if action == "dm"
            else "web",
            "target": target,
            "actor_id": actor_id,
            "method": request.method,
            "route": route,
            "principal_type": "BOT" if bot else "USER",
            "principal": BOT_ID if bot else actor_id,
            "key": key,
            "hash": hashlib.sha256(
                json.dumps([guild, command], ensure_ascii=False, separators=(",", ":")).encode()
            ).digest(),
            "now": now,
            "expires": now + timedelta(hours=24),
            "idem_id": uuid.uuid7(),
            "audit_id": uuid.uuid7(),
            "event_id": uuid.uuid7(),
            "request_id": request.state.request_id,
        }
        result = await mutation(database, lambda c: mutate(c, p, actor, participant))
        response = success(request, result)
        response.status_code = 201 if action == "create" else 200
        return response

    @routes.post(BASE)
    async def create(request: Request, guild_id: str) -> Response:
        return await write(request, guild_id, None, "create")

    @routes.post(ITEM + "/members")
    async def join(request: Request, guild_id: str, recruitment_id: str) -> Response:
        return await write(request, guild_id, recruitment_id, "join")

    @routes.delete(ITEM + "/members/{user_id}")
    async def leave(request: Request, guild_id: str, recruitment_id: str, user_id: str) -> Response:
        return await write(request, guild_id, recruitment_id, "leave", user_id)

    @routes.post(ITEM + "/close")
    async def close(request: Request, guild_id: str, recruitment_id: str) -> Response:
        return await write(request, guild_id, recruitment_id, "close")

    @routes.post(ITEM + "/reopen")
    async def reopen(request: Request, guild_id: str, recruitment_id: str) -> Response:
        return await write(request, guild_id, recruitment_id, "reopen")

    @routes.delete(ITEM)
    async def cancel(request: Request, guild_id: str, recruitment_id: str) -> Response:
        return await write(request, guild_id, recruitment_id, "cancel")

    @routes.post(ITEM + "/restore")
    async def restore(request: Request, guild_id: str, recruitment_id: str) -> Response:
        return await write(request, guild_id, recruitment_id, "restore")

    @routes.patch(ITEM + "/setup-draft")
    async def setup(request: Request, guild_id: str, recruitment_id: str) -> Response:
        return await write(request, guild_id, recruitment_id, "setup")

    @routes.post(ITEM + "/participant-confirmation-dms")
    async def dms(request: Request, guild_id: str, recruitment_id: str) -> Response:
        return await write(request, guild_id, recruitment_id, "dm")

    async def read(request: Request, guild: str, identifier: str, draft: bool) -> Response:
        record = path(guild, identifier)
        assert record is not None
        principal = await resolve_session(request, database, settings)
        async with database.connection(transaction=True) as c:
            await c.execute(text("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY"))
            await require_guild_access(c, principal["user_id"], guild)
            try:
                result = await (setup_snapshot if draft else snapshot)(
                    c, guild, record, principal["user_id"]
                )
            except ApiError as exc:
                if exc.status == 404:
                    raise ApiError(404, "not_found", exc.message) from None
                raise
        return success(request, result)

    @routes.api_route(ITEM, methods=["GET", "HEAD"])
    async def detail(request: Request, guild_id: str, recruitment_id: str) -> Response:
        return await read(request, guild_id, recruitment_id, False)

    @routes.api_route(ITEM + "/setup-draft", methods=["GET", "HEAD"])
    async def draft(request: Request, guild_id: str, recruitment_id: str) -> Response:
        return await read(request, guild_id, recruitment_id, True)

    @routes.api_route(BASE, methods=["GET", "HEAD"])
    async def listing(request: Request, guild_id: str) -> Response:
        path(guild_id)
        query = actionable_query(request)
        principal = await resolve_session(request, database, settings)
        limit = int(query.get("limit", "50"))
        if not 1 <= limit <= 100:
            raise ApiError(400, "invalid_limit", "limit은 1 이상 100 이하이어야 합니다.")
        cursor_time, cursor_id = (
            decode_cursor(query["cursor"]) if "cursor" in query else (None, None)
        )
        async with database.connection() as c:
            await require_guild_access(c, principal["user_id"], guild_id)
            page = await rows(
                c, sql.LIST, principal["user_id"], guild_id, cursor_time, cursor_id, limit + 1
            )
        result = []
        for row in page[:limit]:
            if row["status"] not in {"OPEN", "CLOSED"}:
                raise ApiError(
                    500,
                    "invalid_auction_recruitment_projection",
                    "경매 모집 상태를 처리하지 못했습니다.",
                )
            _, teams = shape(row["format"])
            result.append(
                {
                    "id": str(row["id"]),
                    "displayNumber": row["display_number"],
                    "title": row["title"],
                    "format": row["format"],
                    "rules": rules(row),
                    "participantCount": row["target_count"],
                    "joinedParticipantCount": row["participant_count"],
                    "teamCount": teams,
                    "status": row["status"],
                    "revision": row["revision"],
                    "updatedAt": timestamp(row["updated_at"]),
                    "href": f"/g/{guild_id}/auction/recruitments/{row['id']}"
                    + ("/draft" if row["status"] == "CLOSED" else ""),
                }
            )
        cursor = (
            encode_cursor(page[limit - 1]["updated_at"], page[limit - 1]["id"])
            if len(page) > limit
            else None
        )
        return success(request, {"recruitments": result, "nextCursor": cursor})

    return routes
