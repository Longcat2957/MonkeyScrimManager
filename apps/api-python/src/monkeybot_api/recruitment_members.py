import asyncio
import base64
import hashlib
import json
import re
import uuid
from datetime import UTC, datetime, timedelta
from typing import Any, Literal

from fastapi import APIRouter, Request
from pydantic import BaseModel, ConfigDict, ValidationError, field_validator
from sqlalchemy import text
from sqlalchemy.exc import DBAPIError
from sqlalchemy.ext.asyncio import AsyncConnection
from starlette.responses import Response

from monkeybot_api.auth import require_csrf
from monkeybot_api.config import Settings, is_snowflake
from monkeybot_api.database import Database
from monkeybot_api.errors import ApiError, success
from monkeybot_api.json_body import read_json
from monkeybot_api.recruitment_create import (
    BOT_ID,
    Actor,
    acquire,
    command_json,
    command_key,
    command_principal,
    invalid_bot,
    require_bot,
    sync_actor,
    validate_actor,
)
from monkeybot_api.recruitment_snapshot import recruitment_snapshot


def user_id(value: str) -> str:
    if is_snowflake(value) or re.fullmatch(
        r"dmy_[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}", value
    ):
        return value
    raise ValueError("invalid user ID")


class AddMember(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid")
    memberId: str

    @field_validator("memberId")
    @classmethod
    def validate_id(cls, value: str) -> str:
        return user_id(value)


class RemoveMember(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid")


class BotRemove(RemoveMember):
    actor: Actor


class BotAdd(BotRemove):
    member: Actor


class PreferredLanes(BaseModel):
    # The Axum outer request permits unknown fields; ActorRequest does not.
    model_config = ConfigDict(strict=True, extra="ignore")
    actor: Actor
    preferredLanes: list[Literal["TOP", "JUNGLE", "MID", "BOTTOM", "SUPPORT"]]


async def mutate(
    connection: AsyncConnection,
    params: dict[str, Any],
    actor: Actor | None,
    participant: Actor | None,
    joining: bool,
    lanes: list[str] | None = None,
) -> dict[str, Any]:
    bot = actor is not None
    status = await connection.scalar(
        text("SELECT status FROM guilds WHERE guild_id = :guild FOR UPDATE"), params
    )
    if status is None:
        raise ApiError(404, "not_found", "요청한 대상을 찾을 수 없습니다.")
    if status != "ACTIVE":
        raise ApiError(
            409,
            "guild_inactive",
            "현재 길드에서는 command를 실행할 수 없습니다.",
            {"currentStatus": status},
        )
    role = await connection.scalar(
        text("SELECT operator_role_id FROM guild_config WHERE guild_id = :guild FOR UPDATE"), params
    )
    if role is None:
        raise ApiError(409, "guild_setup_required", "길드 초기 설정을 먼저 완료해야 합니다.")
    try:
        replay = await acquire(connection, params, bot)
    except ApiError as error:
        if not bot and error.code == "idempotency_in_progress":
            raise ApiError(409, error.code, "같은 모집 변경 요청을 처리하고 있습니다.") from None
        raise
    if replay is not None:
        return replay
    if actor is not None:
        await sync_actor(connection, actor, params)
    member_status = await connection.scalar(
        text(
            "SELECT status FROM guild_members "
            "WHERE guild_id = :guild AND user_id = :actor_id FOR UPDATE"
        ),
        params,
    )
    if member_status != "ACTIVE":
        if bot:
            raise ApiError(
                403, "member_inactive", "비활성화된 길드 멤버는 이 작업을 수행할 수 없습니다."
            )
        raise ApiError(
            403, "guild_membership_required", "해당 길드의 활성 멤버만 모집을 변경할 수 있습니다."
        )
    if actor is not None:
        authorized = actor.id == params["member"] or role in actor.roleIds
    else:
        authorized = await connection.scalar(
            text(
                "SELECT true FROM guild_member_discord_roles "
                "WHERE guild_id = :guild AND user_id = :actor_id AND role_id = :role FOR SHARE"
            ),
            {**params, "role": role},
        )
    if not authorized:
        raise ApiError(403, "operator_required", "길드 운영자 권한이 필요합니다.")
    locked = (
        (
            await connection.execute(
                text(
                    "SELECT status, target_count FROM recruitments "
                    "WHERE guild_id = :guild AND id = :id FOR UPDATE"
                ),
                params,
            )
        )
        .mappings()
        .one_or_none()
    )
    if locked is None:
        raise ApiError(404, "not_found", "요청한 대상을 찾을 수 없습니다.")
    if locked["status"] != "OPEN":
        raise ApiError(
            409,
            "invalid_recruitment_state",
            "현재 모집 상태에서는 이 작업을 수행할 수 없습니다.",
            {"currentStatus": locked["status"]},
        )
    draft = await connection.scalar(
        text(
            "SELECT revision FROM recruitment_entry_drafts "
            "WHERE guild_id = :guild AND recruitment_id = :id FOR UPDATE"
        ),
        params,
    )
    if draft is None:
        raise ApiError.from_status(500)
    if joining:
        if participant is not None and participant.id != params["actor_id"]:
            await sync_actor(connection, participant, {**params, "actor_id": participant.id})
        target_status = await connection.scalar(
            text(
                "SELECT status FROM guild_members "
                "WHERE guild_id = :guild AND user_id = :member FOR UPDATE"
            ),
            params,
        )
        if target_status != "ACTIVE":
            if bot:
                raise ApiError(
                    403, "member_inactive", "비활성화된 길드 멤버는 이 작업을 수행할 수 없습니다."
                )
            if target_status is None:
                raise ApiError(
                    404, "guild_member_not_found", "길드에 소속된 사용자를 찾을 수 없습니다."
                )
            raise ApiError(
                409, "guild_member_inactive", "비활성화된 길드 멤버는 모집에 추가할 수 없습니다."
            )
    before = await recruitment_snapshot(connection, params["guild"], params["id"])
    if lanes is not None:
        existing = await connection.scalar(
            text(
                "SELECT status FROM recruitment_participants WHERE guild_id = :guild "
                "AND recruitment_id = :id AND user_id = :member FOR UPDATE"
            ),
            params,
        )
        if existing != "JOINED":
            raise ApiError(422, "not_participant", "현재 모집 참가자가 아닙니다.")
        await connection.execute(
            text(
                "DELETE FROM recruitment_preferred_lanes WHERE guild_id = :guild "
                "AND recruitment_id = :id AND user_id = :member"
            ),
            params,
        )
        for priority, lane in enumerate(lanes, 1):
            await connection.execute(
                text(
                    "INSERT INTO recruitment_preferred_lanes "
                    "(guild_id, recruitment_id, user_id, priority, lane) "
                    "VALUES (:guild, :id, :member, :priority, :lane)"
                ),
                {**params, "priority": priority, "lane": lane},
            )
    elif joining:
        existing = await connection.scalar(
            text(
                "SELECT status FROM recruitment_participants "
                "WHERE guild_id = :guild AND recruitment_id = :id AND user_id = :member FOR UPDATE"
            ),
            params,
        )
        if existing == "JOINED":
            raise ApiError(422, "already_participant", "이미 참가 중인 사용자입니다.")
        count = await connection.scalar(
            text(
                "SELECT count(*) FROM recruitment_participants "
                "WHERE guild_id = :guild AND recruitment_id = :id AND status = 'JOINED'"
            ),
            params,
        )
        if count >= locked["target_count"]:
            raise ApiError(422, "recruitment_full", "모집 정원이 가득 찼습니다.")
        await connection.execute(
            text("""
            INSERT INTO recruitment_participants
                (guild_id, recruitment_id, user_id, status, joined_at, withdrawn_at)
            VALUES (:guild, :id, :member, 'JOINED', :now, NULL)
            ON CONFLICT (guild_id, recruitment_id, user_id) DO UPDATE
            SET status = 'JOINED', joined_at = EXCLUDED.joined_at, withdrawn_at = NULL
        """),
            params,
        )
    else:
        changed = await connection.scalar(
            text("""
            UPDATE recruitment_participants SET status = 'WITHDRAWN', withdrawn_at = :now
            WHERE guild_id = :guild AND recruitment_id = :id AND user_id = :member
              AND status = 'JOINED' RETURNING user_id
        """),
            params,
        )
        if changed is None:
            raise ApiError(422, "not_participant", "현재 모집 참가자가 아닙니다.")
        await connection.execute(
            text(
                "DELETE FROM recruitment_entry_assignments "
                "WHERE guild_id = :guild AND recruitment_id = :id AND user_id = :member"
            ),
            params,
        )
    await connection.execute(
        text(
            "UPDATE recruitments SET revision = revision + 1, "
            "updated_by = :actor_id, updated_at = :now WHERE guild_id = :guild AND id = :id"
        ),
        params,
    )
    await connection.execute(
        text(
            "UPDATE recruitment_entry_drafts SET revision = revision + 1, "
            "updated_by = :actor_id, updated_at = :now "
            "WHERE guild_id = :guild AND recruitment_id = :id"
        ),
        params,
    )
    snapshot = await recruitment_snapshot(connection, params["guild"], params["id"])
    data = snapshot["recruitment"]
    values = {
        **params,
        "before": json.dumps(before),
        "snapshot": json.dumps(snapshot),
        "revision": data["revision"],
        "target": str(params["id"]),
    }
    await connection.execute(
        text("""
        INSERT INTO admin_audit_log
            (guild_id, id, principal_type, principal_id, action, target_type, target_id,
             request_id, before_summary, after_summary, result, created_at)
        VALUES (:guild, :audit_id, 'USER', :actor_id, :action, 'RECRUITMENT', :target, :request_id,
                CAST(:before AS jsonb), CAST(:snapshot AS jsonb), 'APPLIED', :now)
    """),
        values,
    )
    await connection.execute(
        text("""
        INSERT INTO outbox_events (id, guild_id, topic, aggregate_type, aggregate_id,
            aggregate_revision, event_type, payload, occurred_at, available_at)
        VALUES (:event_id, :guild, :topic, 'RECRUITMENT', :target, :revision, :event_type,
                CAST(:payload AS jsonb), :now, :now)
    """),
        {
            **values,
            "topic": f"guild:{params['guild']}:recruitment:{params['id']}",
            "payload": json.dumps(
                {"type": "invalidate", "resourceId": data["id"], "revision": data["revision"]}
            ),
        },
    )
    if not bot and data["channelId"] is not None and data["messageId"] is not None:
        await connection.execute(
            text("""
            INSERT INTO bot_notification_outbox (id, guild_id, deduplication_key, discord_nonce,
                channel_id, notification_type, payload, created_at, available_at)
            VALUES (:notification_id, :guild, :dedup, :nonce, :channel,
                    'RECRUITMENT_MESSAGE_UPDATE',
                    CAST(:payload AS jsonb), :now, :now)
            ON CONFLICT (guild_id, deduplication_key) DO NOTHING
        """),
            {
                **values,
                "dedup": f"recruitment:{data['id']}:message:update:{data['revision']}",
                "channel": data["channelId"],
                "nonce": base64.urlsafe_b64encode(params["notification_id"].bytes)
                .decode()
                .rstrip("="),
                "payload": json.dumps(
                    {"schemaVersion": 1, "recruitmentId": data["id"], "snapshot": snapshot}
                ),
            },
        )
    await connection.execute(
        text("""
        UPDATE idempotency_records SET state = 'COMPLETED', status_code = 200,
            response_body = CAST(:snapshot AS jsonb), completed_at = :now
        WHERE id = :idem_id AND guild_id = :guild AND scope_key = :guild AND state = 'IN_PROGRESS'
    """),
        values,
    )
    return snapshot


def router(settings: Settings, database: Database) -> APIRouter:
    routes = APIRouter()

    async def execute(
        request: Request,
        guild_id: str,
        recruitment_id: str,
        member_id: str | None,
        preferred: bool = False,
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
        for attempt in range(3):
            try:
                async with database.connection(transaction=True) as connection:
                    snapshot = await mutate(connection, params, actor, participant, joining, lanes)
                return success(request, snapshot)
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

    @routes.post("/api/guilds/{guild_id}/recruitments/{recruitment_id}/members")
    async def join(request: Request, guild_id: str, recruitment_id: str) -> Response:
        return await execute(request, guild_id, recruitment_id, None)

    @routes.delete("/api/guilds/{guild_id}/recruitments/{recruitment_id}/members/{member_id}")
    async def leave(
        request: Request, guild_id: str, recruitment_id: str, member_id: str
    ) -> Response:
        return await execute(request, guild_id, recruitment_id, member_id)

    @routes.put(
        "/api/guilds/{guild_id}/recruitments/{recruitment_id}/members/{member_id}/preferred-lanes"
    )
    async def preferred_lanes(
        request: Request, guild_id: str, recruitment_id: str, member_id: str
    ) -> Response:
        return await execute(request, guild_id, recruitment_id, member_id, preferred=True)

    return routes
