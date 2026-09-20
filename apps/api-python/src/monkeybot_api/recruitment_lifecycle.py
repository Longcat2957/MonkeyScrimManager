import asyncio
import base64
import hashlib
import json
import uuid
from datetime import UTC, datetime, timedelta
from typing import Any, Literal

from fastapi import APIRouter, Request
from pydantic import BaseModel, ConfigDict, Field, ValidationError
from sqlalchemy import text
from sqlalchemy.exc import DBAPIError
from sqlalchemy.ext.asyncio import AsyncConnection
from starlette.responses import Response

from monkeybot_api.auth import require_csrf
from monkeybot_api.config import Settings, is_snowflake
from monkeybot_api.database import Database
from monkeybot_api.entry_mutation import MoveParticipant, apply_move
from monkeybot_api.entry_snapshot import entry_snapshot
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
    sync_actor,
    validate_actor,
)
from monkeybot_api.recruitment_members import BotRemove, RemoveMember
from monkeybot_api.recruitment_snapshot import recruitment_snapshot
from monkeybot_api.session import resolve_session
from monkeybot_api.workflow import require_guild_access

Action = Literal["Close", "Cancel", "Restore", "Reopen", "Apply", "Reset"]
ENTRY_ACTIONS = {"Reopen", "Apply", "Reset"}


class Revision(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)
    expectedRevision: int = Field(ge=-(2**63), le=2**63 - 1)


class EditRevision(Revision):
    operation: MoveParticipant


async def require_member(
    connection: AsyncConnection, params: dict[str, Any], action: Action, bot: bool
) -> None:
    active = await connection.scalar(
        text(
            "SELECT status FROM guild_members "
            "WHERE guild_id = :guild AND user_id = :actor_id FOR UPDATE"
        ),
        params,
    )
    if active != "ACTIVE":
        if bot:
            raise ApiError(
                403, "member_inactive", "비활성화된 길드 멤버는 이 작업을 수행할 수 없습니다."
            )
        message = (
            "해당 길드의 활성 멤버만 엔트리 편성을 변경할 수 있습니다."
            if action in ENTRY_ACTIONS
            else "해당 길드의 활성 멤버만 모집을 변경할 수 있습니다."
        )
        raise ApiError(403, "guild_membership_required", message)


async def require_operator(
    connection: AsyncConnection, params: dict[str, Any], role: str, actor: Actor | None
) -> None:
    authorized = (
        role in actor.roleIds
        if actor is not None
        else await connection.scalar(
            text(
                "SELECT true FROM guild_member_discord_roles "
                "WHERE guild_id = :guild AND user_id = :actor_id AND role_id = :role FOR SHARE"
            ),
            {**params, "role": role},
        )
    )
    if not authorized:
        raise ApiError(403, "operator_required", "길드 운영자 권한이 필요합니다.")


async def save_effects(
    connection: AsyncConnection,
    params: dict[str, Any],
    action: Action,
    before: dict[str, Any],
    snapshot: dict[str, Any],
    message: dict[str, Any],
    bot: bool,
) -> None:
    is_entry = action in ENTRY_ACTIONS
    target_type = "RECRUITMENT_ENTRY_DRAFT" if is_entry else "RECRUITMENT"
    revision = snapshot["draft"]["revision"] if is_entry else snapshot["recruitment"]["revision"]
    values = {
        **params,
        "before": json.dumps(before),
        "snapshot": json.dumps(snapshot),
        "target_type": target_type,
        "target": str(params["id"]),
    }
    await connection.execute(
        text("""
        INSERT INTO admin_audit_log
            (guild_id, id, principal_type, principal_id, action, target_type, target_id,
             request_id, before_summary, after_summary, result, created_at)
        VALUES (:guild, :audit_id, 'USER', :actor_id, :audit_action, :target_type, :target,
                :request_id, CAST(:before AS jsonb), CAST(:snapshot AS jsonb), 'APPLIED', :now)
    """),
        values,
    )
    events = [
        (
            params["event_id"],
            target_type,
            "recruitment-entry-draft" if is_entry else "recruitment",
            revision,
        )
    ]
    if action == "Reopen":
        events.append(
            (
                params["recruitment_event_id"],
                "RECRUITMENT",
                "recruitment",
                snapshot["recruitment"]["revision"],
            )
        )
    for event_id, aggregate, topic_part, event_revision in events:
        await connection.execute(
            text("""
            INSERT INTO outbox_events (id, guild_id, topic, aggregate_type, aggregate_id,
                aggregate_revision, event_type, payload, occurred_at, available_at)
            VALUES (:event, :guild, :topic, :aggregate, :target, :revision, :event_type,
                    CAST(:payload AS jsonb), :now, :now)
        """),
            {
                **values,
                "event": event_id,
                "aggregate": aggregate,
                "revision": event_revision,
                "topic": f"guild:{params['guild']}:{topic_part}:{params['id']}",
                "payload": json.dumps(
                    {
                        "type": "invalidate",
                        "resourceId": str(params["id"]),
                        "revision": event_revision,
                    }
                ),
            },
        )
    data = message["recruitment"]
    if (
        action not in {"Apply", "Reset"}
        and not bot
        and data["channelId"] is not None
        and (action == "Restore" or data["messageId"] is not None)
    ):
        verb, notification_type = {
            "Cancel": ("delete", "RECRUITMENT_MESSAGE_DELETE"),
            "Restore": ("restore", "RECRUITMENT_MESSAGE_CREATE"),
        }.get(action, ("update", "RECRUITMENT_MESSAGE_UPDATE"))
        await connection.execute(
            text("""
            INSERT INTO bot_notification_outbox (id, guild_id, deduplication_key, discord_nonce,
                channel_id, notification_type, payload, created_at, available_at)
            VALUES (:notification_id, :guild, :dedup, :nonce, :channel, :notification_type,
                    CAST(:payload AS jsonb), :now, :now)
            ON CONFLICT (guild_id, deduplication_key) DO NOTHING
        """),
            {
                **values,
                "dedup": f"recruitment:{data['id']}:message:{verb}:{data['revision']}",
                "channel": data["channelId"],
                "notification_type": notification_type,
                "nonce": base64.urlsafe_b64encode(params["notification_id"].bytes)
                .decode()
                .rstrip("="),
                "payload": json.dumps(
                    {"schemaVersion": 1, "recruitmentId": data["id"], "snapshot": message}
                ),
            },
        )
    completed = await connection.scalar(
        text("""
        UPDATE idempotency_records SET state = 'COMPLETED', status_code = 200,
            response_body = CAST(:snapshot AS jsonb), completed_at = :now
        WHERE id = :idem_id AND guild_id = :guild AND scope_key = :guild AND state = 'IN_PROGRESS'
        RETURNING id
    """),
        values,
    )
    if completed is None:
        raise ApiError.from_status(500)


async def mutate(
    connection: AsyncConnection, params: dict[str, Any], action: Action, actor: Actor | None
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
    # Axum's entry commands authorize before idempotency; recruitment lifecycle authorizes after.
    if action in ENTRY_ACTIONS:
        await require_member(connection, params, action, False)
        await require_operator(connection, params, role, None)
    try:
        replay = await acquire(connection, params, bot)
    except ApiError as error:
        if not bot and error.code == "idempotency_in_progress":
            raise ApiError(
                409,
                error.code,
                "같은 엔트리 편성 요청을 처리하고 있습니다."
                if action in ENTRY_ACTIONS
                else "같은 모집 변경 요청을 처리하고 있습니다.",
            ) from None
        raise
    if replay is not None:
        return replay
    if actor is not None:
        await sync_actor(connection, actor, params)
        await require_member(connection, params, action, True)
    if action == "Close":
        if actor is None:
            await require_member(connection, params, action, False)
        await require_operator(connection, params, role, actor)
    locked = (
        (
            await connection.execute(
                text("""
        SELECT status, revision, target_count, created_by, closed_at
        FROM recruitments WHERE guild_id = :guild AND id = :id FOR UPDATE
    """),
                params,
            )
        )
        .mappings()
        .one_or_none()
    )
    if locked is None:
        raise ApiError(404, "not_found", "요청한 대상을 찾을 수 없습니다.")
    if action in {"Cancel", "Restore"}:
        await require_member(connection, params, action, False)
        if locked["created_by"] != params["actor_id"]:
            await require_operator(connection, params, role, None)
    draft = None
    if action in ENTRY_ACTIONS:
        draft = await connection.scalar(
            text(
                "SELECT revision FROM recruitment_entry_drafts "
                "WHERE guild_id = :guild AND recruitment_id = :id FOR UPDATE"
            ),
            params,
        )
        if draft is None:
            raise ApiError.from_status(500)
    actual_revision = draft if action in {"Apply", "Reset"} else locked["revision"]
    if action != "Close" and actual_revision != params["expected"]:
        raise ApiError(
            409,
            "entry_draft_revision_conflict"
            if action in ENTRY_ACTIONS
            else "recruitment_revision_conflict",
            "다른 운영자가 엔트리 편성을 변경했습니다. 최신 상태를 확인해 주세요."
            if action in ENTRY_ACTIONS
            else "모집 상태가 변경되었습니다. 최신 상태를 확인해 주세요.",
            {"expectedRevision": params["expected"], "actualRevision": actual_revision},
        )
    before = None
    if action in ENTRY_ACTIONS:
        before = await entry_snapshot(connection, params["guild"], params["id"], params["actor_id"])
    allowed = {
        "Close": {"OPEN"},
        "Cancel": {"OPEN", "CLOSED"},
        "Restore": {"CANCELLED"},
        "Reopen": {"CLOSED"},
        "Apply": {"CLOSED"},
        "Reset": {"CLOSED"},
    }
    if locked["status"] not in allowed[action]:
        error_action = "Reopen" if action in ENTRY_ACTIONS else action
        code, message = {
            "Restore": ("recruitment_restore_unavailable", "취소된 모집만 복구할 수 있습니다."),
            "Reopen": (
                "entry_draft_not_editable",
                "현재 모집 상태에서는 엔트리 편성을 수정할 수 없습니다.",
            ),
        }.get(
            error_action,
            ("invalid_recruitment_state", "현재 모집 상태에서는 이 작업을 수행할 수 없습니다."),
        )
        raise ApiError(409, code, message, {"currentStatus": locked["status"]})
    if before is None:
        before = await recruitment_snapshot(connection, params["guild"], params["id"])
    if action == "Close":
        count = await connection.scalar(
            text(
                "SELECT count(*) FROM recruitment_participants "
                "WHERE guild_id = :guild AND recruitment_id = :id AND status = 'JOINED'"
            ),
            params,
        )
        if count != locked["target_count"]:
            raise ApiError(
                422,
                "recruitment_not_full",
                "정원을 채운 뒤 엔트리 편성을 시작할 수 있습니다.",
                {"joinedCount": count, "targetCount": locked["target_count"]},
            )
    if action in {"Apply", "Reset"}:
        if action == "Apply":
            await apply_move(connection, params)
        else:
            await connection.execute(
                text(
                    "DELETE FROM recruitment_entry_assignments "
                    "WHERE guild_id = :guild AND recruitment_id = :id"
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
        await connection.execute(
            text(
                "UPDATE recruitments SET updated_by = :actor_id, updated_at = :now "
                "WHERE guild_id = :guild AND id = :id"
            ),
            params,
        )
        snapshot = await entry_snapshot(
            connection, params["guild"], params["id"], params["actor_id"]
        )
        await save_effects(connection, params, action, before, snapshot, snapshot, False)
        return snapshot
    changes = {
        "Close": "status = 'CLOSED', closed_at = :now",
        "Cancel": "status = 'CANCELLED', cancelled_at = :now",
        "Restore": (
            "status = :restored, converted_at = NULL, cancelled_at = NULL, message_id = NULL"
        ),
        "Reopen": "status = 'OPEN', closed_at = NULL, cancelled_at = NULL",
    }[action]
    guard = " AND revision = :expected" if action in {"Cancel", "Restore"} else ""
    changed = await connection.scalar(
        text(f"""
        UPDATE recruitments SET {changes}, revision = revision + 1,
            updated_by = :actor_id, updated_at = :now
        WHERE guild_id = :guild AND id = :id {guard} RETURNING id
    """),
        {**params, "restored": "CLOSED" if locked["closed_at"] is not None else "OPEN"},
    )
    if changed is None:
        raise ApiError.from_status(500)
    if action == "Reopen":
        changed = await connection.scalar(
            text("""
            UPDATE recruitment_entry_drafts SET revision = revision + 1,
                updated_by = :actor_id, updated_at = :now
            WHERE guild_id = :guild AND recruitment_id = :id RETURNING recruitment_id
        """),
            params,
        )
        if changed is None:
            raise ApiError.from_status(500)
        message_snapshot = await recruitment_snapshot(connection, params["guild"], params["id"])
        snapshot = await entry_snapshot(
            connection, params["guild"], params["id"], params["actor_id"]
        )
    else:
        snapshot = await recruitment_snapshot(connection, params["guild"], params["id"])
        message_snapshot = snapshot
    await save_effects(connection, params, action, before, snapshot, message_snapshot, bot)
    return snapshot


def router(settings: Settings, database: Database) -> APIRouter:
    routes = APIRouter()

    async def execute(
        request: Request, guild_id: str, recruitment_id: str, action: Action
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
        for attempt in range(3):
            try:
                async with database.connection(transaction=True) as connection:
                    snapshot = await mutate(connection, params, action, actor)
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

    @routes.post("/api/guilds/{guild_id}/recruitments/{recruitment_id}/close")
    async def close(request: Request, guild_id: str, recruitment_id: str) -> Response:
        return await execute(request, guild_id, recruitment_id, "Close")

    @routes.delete("/api/guilds/{guild_id}/recruitments/{recruitment_id}")
    async def cancel(request: Request, guild_id: str, recruitment_id: str) -> Response:
        return await execute(request, guild_id, recruitment_id, "Cancel")

    @routes.post("/api/guilds/{guild_id}/recruitments/{recruitment_id}/restore")
    async def restore(request: Request, guild_id: str, recruitment_id: str) -> Response:
        return await execute(request, guild_id, recruitment_id, "Restore")

    @routes.post("/api/guilds/{guild_id}/recruitments/{recruitment_id}/reopen")
    async def reopen(request: Request, guild_id: str, recruitment_id: str) -> Response:
        return await execute(request, guild_id, recruitment_id, "Reopen")

    @routes.patch("/api/guilds/{guild_id}/recruitments/{recruitment_id}/entry-draft")
    async def edit(request: Request, guild_id: str, recruitment_id: str) -> Response:
        return await execute(request, guild_id, recruitment_id, "Apply")

    @routes.post("/api/guilds/{guild_id}/recruitments/{recruitment_id}/entry-draft/reset")
    async def reset(request: Request, guild_id: str, recruitment_id: str) -> Response:
        return await execute(request, guild_id, recruitment_id, "Reset")

    @routes.api_route(
        "/api/guilds/{guild_id}/recruitments/{recruitment_id}/entry-draft", methods=["GET", "HEAD"]
    )
    async def entry(request: Request, guild_id: str, recruitment_id: str) -> Response:
        try:
            if not is_snowflake(guild_id):
                raise ValueError("invalid guild")
            record_id = uuid.UUID(recruitment_id)
        except ValueError:
            raise ApiError.from_status(400) from None
        principal = await resolve_session(request, database, settings)
        async with database.connection() as connection:
            await require_guild_access(connection, principal["user_id"], guild_id)
        async with database.connection(transaction=True) as connection:
            await connection.execute(
                text("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            )
            snapshot = await entry_snapshot(connection, guild_id, record_id, principal["user_id"])
            if snapshot["recruitment"]["status"] not in {"OPEN", "CLOSED"}:
                raise ApiError(404, "not_found", "요청한 모집을 찾을 수 없습니다.")
        return success(request, snapshot)

    return routes
