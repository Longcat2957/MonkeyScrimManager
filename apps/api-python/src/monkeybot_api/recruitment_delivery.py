import asyncio
import hashlib
import json
import uuid
from datetime import UTC, datetime, timedelta
from typing import Any, Literal

from fastapi import APIRouter, Request
from pydantic import field_validator
from sqlalchemy import text
from sqlalchemy.exc import DBAPIError
from sqlalchemy.ext.asyncio import AsyncConnection
from starlette.responses import Response

from monkeybot_api import entry_auction_conversion as conversion
from monkeybot_api.auth import require_csrf
from monkeybot_api.config import Settings, is_snowflake
from monkeybot_api.database import Database
from monkeybot_api.delivery_effects import audit, complete, invalidate, notification
from monkeybot_api.errors import ApiError, success
from monkeybot_api.json_body import read_json
from monkeybot_api.recruitment_create import (
    BOT_ID,
    Actor,
    acquire,
    canonical_start,
    command_key,
    require_bot,
    sync_actor,
    validate_actor,
)
from monkeybot_api.recruitment_lifecycle import Revision, require_operator
from monkeybot_api.recruitment_members import BotRemove, RemoveMember
from monkeybot_api.recruitment_snapshot import recruitment_snapshot
from monkeybot_api.series_creation import create_series
from monkeybot_api.session import resolve_session

Action = Literal["bind", "dm", "series", "convert"]


class BindMessage(BotRemove):
    channelId: str
    messageId: str

    @field_validator("channelId", "messageId")
    @classmethod
    def validate_snowflake(cls, value: str) -> str:
        if not is_snowflake(value):
            raise ValueError("invalid snowflake")
        return value


class SeriesCreate(Revision):
    sourceRecruitmentId: str

    @field_validator("sourceRecruitmentId")
    @classmethod
    def validate_uuid(cls, value: str) -> str:
        return str(uuid.UUID(value))


async def authorize(
    c: AsyncConnection, p: dict[str, Any], action: Action, role: str, actor: Actor | None
) -> None:
    member = await c.scalar(
        text(
            "SELECT status FROM guild_members "
            "WHERE guild_id = :guild AND user_id = :actor_id FOR UPDATE"
        ),
        p,
    )
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
    status = await c.scalar(text("SELECT status FROM guilds WHERE guild_id = :guild FOR UPDATE"), p)
    if status is None:
        raise ApiError(404, "not_found", "요청한 대상을 찾을 수 없습니다.")
    if status != "ACTIVE":
        raise ApiError(
            409,
            "guild_inactive",
            "현재 길드에서는 command를 실행할 수 없습니다.",
            {"currentStatus": status},
        )
    config = (
        (
            await c.execute(
                text(
                    "SELECT operator_role_id, entry_channel_id, recruitment_channel_id "
                    "FROM guild_config "
                    "WHERE guild_id = :guild FOR UPDATE"
                ),
                p,
            )
        )
        .mappings()
        .one_or_none()
    )
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
    row = (
        (
            await c.execute(
                text(
                    "SELECT status, revision, created_by, season_id, target_count, "
                    "best_of, hard_fearless FROM recruitments "
                    "WHERE guild_id = :guild AND id = :id FOR UPDATE"
                ),
                p,
            )
        )
        .mappings()
        .one_or_none()
    )
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
        await c.execute(
            text(
                "UPDATE recruitments SET channel_id = :channel, message_id = :message, "
                "revision = revision + 1, updated_by = :actor_id, updated_at = :now "
                "WHERE guild_id = :guild AND id = :id"
            ),
            p,
        )
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


def router(settings: Settings, database: Database) -> APIRouter:
    routes = APIRouter()

    async def execute(request: Request, guild: str, record: str | None, action: Action) -> Response:
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
                converted = await read_json(request, conversion.Conversion)
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
        for attempt in range(3):
            try:
                async with database.connection(transaction=True) as c:
                    result = await mutate(c, p, action, actor)
                response = success(request, result)
                response.status_code = 201 if action in {"series", "convert"} else 200
                return response
            except DBAPIError as error:
                if getattr(error.orig, "sqlstate", "") != "40P01":
                    raise
                if attempt == 2:
                    raise ApiError(
                        503,
                        "database_busy",
                        "데이터베이스가 사용 중입니다. 잠시 후 다시 시도해 주세요.",
                    ) from None
                await asyncio.sleep(0.025 * 2**attempt)
        raise AssertionError("unreachable")

    @routes.put("/api/guilds/{guild_id}/recruitments/{recruitment_id}/message")
    async def bind(request: Request, guild_id: str, recruitment_id: str) -> Response:
        return await execute(request, guild_id, recruitment_id, "bind")

    @routes.post(
        "/api/guilds/{guild_id}/recruitments/{recruitment_id}/participant-confirmation-dms"
    )
    async def dm(request: Request, guild_id: str, recruitment_id: str) -> Response:
        return await execute(request, guild_id, recruitment_id, "dm")

    @routes.post("/api/guilds/{guild_id}/series")
    async def series(request: Request, guild_id: str) -> Response:
        return await execute(request, guild_id, None, "series")

    @routes.post("/api/guilds/{guild_id}/recruitments/{recruitment_id}/auction-recruitment")
    async def convert(request: Request, guild_id: str, recruitment_id: str) -> Response:
        return await execute(request, guild_id, recruitment_id, "convert")

    return routes
