import asyncio
import base64
import hashlib
import hmac
import json
import re
import uuid
from datetime import UTC, datetime, timedelta, timezone
from typing import Any

from fastapi import APIRouter, Request
from pydantic import BaseModel, ConfigDict, Field, ValidationError, field_validator
from sqlalchemy import text
from sqlalchemy.exc import DBAPIError
from sqlalchemy.ext.asyncio import AsyncConnection
from starlette.responses import Response

from monkeybot_api.auth import require_csrf
from monkeybot_api.config import Settings, is_snowflake
from monkeybot_api.context import timestamp
from monkeybot_api.database import Database
from monkeybot_api.errors import ApiError, success
from monkeybot_api.json_body import reject_json_constant
from monkeybot_api.session import resolve_session

ROUTE = "/api/guilds/{guildId}/recruitments"
BOT_ID = "monkeybot-discord-bot"


class Actor(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid")
    id: str
    globalDisplayName: str
    displayName: str
    avatarHash: str | None = None
    roleIds: list[str]

    @field_validator("id")
    @classmethod
    def valid_id(cls, value: str) -> str:
        if not is_snowflake(value):
            raise ValueError("invalid snowflake")
        return value

    @field_validator("roleIds")
    @classmethod
    def valid_roles(cls, value: list[str]) -> list[str]:
        if any(not is_snowflake(role) for role in value):
            raise ValueError("invalid role snowflake")
        return value


class BrowserCreate(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid")
    bestOf: int | None = Field(default=None, ge=-32768, le=32767)
    hardFearless: bool | None = None
    expectedStartAt: Any = None


class BotCreate(BrowserCreate):
    actor: Actor


def invalid_bot(field: str) -> ApiError:
    return ApiError(
        400, "invalid_bot_command", "Bot command 요청 형식이 올바르지 않습니다.", {"field": field}
    )


def canonical_start(value: Any) -> tuple[datetime, str]:
    if value is None or (isinstance(value, str) and not value.strip()):
        date = datetime.now(UTC) + timedelta(minutes=30)
        return date, timestamp(date)
    try:
        if not isinstance(value, str):
            raise ValueError("timestamp must be a string")
        match = re.fullmatch(
            r"(\d{4}-\d{2}-\d{2})[Tt](\d{2}:\d{2}:)(\d{2})(?:\.(\d+))?([Zz]|[+-]\d{2}:\d{2})",
            value.strip(),
        )
        if match is None:
            raise ValueError("invalid RFC3339")
        day, clock, second, fraction, offset = match.groups()
        fraction = (fraction or "")[:9]
        if second == "60":
            second, fraction = "59", "999999999"
        offset = "+00:00" if offset.lower() == "z" else offset
        if int(offset[1:3]) > 23 or int(offset[4:]) > 59:
            raise ValueError("invalid offset")
        date = datetime.fromisoformat(f"{day}T{clock}{second}.{fraction or '0'}{offset}")
        fraction = fraction.rstrip("0")
        canonical = f"{day}T{clock}{second}" + (f".{fraction}" if fraction else "")
        canonical += "Z" if date.utcoffset() == timedelta(0) else offset
        return date, canonical
    except ValueError, OverflowError:
        raise ApiError(
            422,
            "invalid_expected_start_at",
            "예상 시작 시간은 시간대가 포함된 ISO 8601 형식이어야 합니다.",
            {"field": "expectedStartAt"},
        ) from None


def request_hash(guild_id: str, rules: dict[str, Any], start: str, actor: Actor | None) -> bytes:
    command: dict[str, Any] = {}
    if actor is not None:
        command = {"type": "CREATE", "actor": actor.model_dump()}
    command.update({"rules": rules, "expected_start_at": start})
    canonical = json.dumps([guild_id, command], ensure_ascii=False, separators=(",", ":"))
    return hashlib.sha256(canonical.encode()).digest()


def in_progress(bot: bool) -> ApiError:
    return ApiError(
        409,
        "idempotency_in_progress",
        "같은 interaction 요청을 처리하고 있습니다."
        if bot
        else "같은 내전 생성 요청을 처리하고 있습니다.",
    )


async def acquire(
    connection: AsyncConnection, params: dict[str, Any], bot: bool
) -> dict[str, Any] | None:
    params.setdefault("method", "POST")
    locked = await connection.scalar(
        text("""
        SELECT pg_try_advisory_xact_lock(hashtextextended(jsonb_build_array(
            'monkeybot:idempotency:v1', CAST(:guild AS text), CAST(:principal_type AS text),
            CAST(:principal AS text), CAST(:method AS text),
            CAST(:route AS text), CAST(:key AS text)
        )::text, 0))
    """),
        params,
    )
    if not locked:
        raise in_progress(bot)
    where = """scope_key = :guild AND guild_id = :guild AND principal_type = :principal_type
               AND principal_id = :principal AND method = :method AND route_template = :route
               AND idempotency_key = :key"""
    await connection.execute(
        text(f"DELETE FROM idempotency_records WHERE {where} AND expires_at <= :now"), params
    )
    inserted = await connection.scalar(
        text("""
        INSERT INTO idempotency_records
          (id, guild_id, scope_key, principal_type, principal_id, method, route_template,
           idempotency_key, request_hash, state, created_at, expires_at)
        VALUES (:idem_id, :guild, :guild, :principal_type, :principal, :method, :route,
                :key, :hash, 'IN_PROGRESS', :now, :expires)
        ON CONFLICT ON CONSTRAINT idempotency_records_command_key DO NOTHING RETURNING id
    """),
        params,
    )
    if inserted is not None:
        return None
    row = (
        (
            await connection.execute(
                text(f"""
        SELECT request_hash, state, status_code, response_body FROM idempotency_records
        WHERE {where} FOR UPDATE
    """),
                params,
            )
        )
        .mappings()
        .one()
    )
    if row["request_hash"] != params["hash"]:
        raise ApiError(
            409,
            "idempotency_conflict",
            "같은 interaction ID가 다른 요청에 사용되었습니다."
            if bot
            else "같은 Idempotency-Key가 다른 요청에 사용되었습니다.",
        )
    if row["state"] == "IN_PROGRESS":
        raise in_progress(bot)
    if row["state"] != "COMPLETED" or not isinstance(row["response_body"], dict):
        raise ApiError.from_status(500)
    return dict(row["response_body"])


async def sync_actor(connection: AsyncConnection, actor: Actor, params: dict[str, Any]) -> None:
    values = {**params, **actor.model_dump()}
    persisted = await connection.scalar(
        text("""
        INSERT INTO users (user_id, identity_kind, display_name, avatar_hash,
                           created_at, updated_at, deleted_at)
        VALUES (:actor_id, 'DISCORD', :globalDisplayName, :avatarHash, :now, :now, NULL)
        ON CONFLICT (user_id) DO UPDATE
        SET display_name = EXCLUDED.display_name, avatar_hash = EXCLUDED.avatar_hash,
            updated_at = EXCLUDED.updated_at, deleted_at = NULL
        WHERE users.identity_kind = 'DISCORD' RETURNING user_id
    """),
        values,
    )
    identity = (
        await connection.scalar(
            text("""
        INSERT INTO user_discord_identities (user_id, discord_id, created_at, updated_at)
        VALUES (:actor_id, :actor_id, :now, :now)
        ON CONFLICT (discord_id) DO UPDATE SET updated_at = EXCLUDED.updated_at
        WHERE user_discord_identities.user_id = EXCLUDED.user_id RETURNING user_id
    """),
            values,
        )
        if persisted is not None
        else None
    )
    if identity != persisted or persisted is None:
        raise ApiError(
            409,
            "user_identity_conflict",
            "이 Discord 계정 식별자는 다른 사용자 유형에서 이미 사용 중입니다.",
        )
    await connection.execute(
        text("""
        INSERT INTO guild_members
            (guild_id, user_id, display_name, status, main_lane, first_seen_at, registered_at,
             discord_joined_at, last_active_at, last_synced_at, inactivated_at)
        VALUES (:guild, :actor_id, :displayName, 'ACTIVE', NULL, :now, :now, NULL, :now, :now, NULL)
        ON CONFLICT (guild_id, user_id) DO UPDATE
        SET display_name = EXCLUDED.display_name,
            registered_at = COALESCE(guild_members.registered_at, EXCLUDED.registered_at),
            last_active_at = EXCLUDED.last_active_at, last_synced_at = EXCLUDED.last_synced_at
    """),
        values,
    )
    await connection.execute(
        text(
            "DELETE FROM guild_member_discord_roles WHERE guild_id = :guild AND user_id = :actor_id"
        ),
        values,
    )
    for role in dict.fromkeys(actor.roleIds):
        await connection.execute(
            text("""
            INSERT INTO guild_member_discord_roles (guild_id, user_id, role_id, last_synced_at)
            VALUES (:guild, :actor_id, :role, :now)
        """),
            {**values, "role": role},
        )


async def create_attempt(
    connection: AsyncConnection, params: dict[str, Any], actor: Actor | None, start: datetime
) -> dict[str, Any]:
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
    config = (
        (
            await connection.execute(
                text(
                    "SELECT operator_role_id, recruitment_channel_id "
                    "FROM guild_config WHERE guild_id = :guild FOR UPDATE"
                ),
                params,
            )
        )
        .mappings()
        .one_or_none()
    )
    if config is None:
        raise ApiError(409, "guild_setup_required", "길드 초기 설정을 먼저 완료해야 합니다.")
    replay = await acquire(connection, params, actor is not None)
    if replay is not None:
        return replay
    if actor is not None:
        await sync_actor(connection, actor, params)
    membership = await connection.scalar(
        text(
            "SELECT status FROM guild_members "
            "WHERE guild_id = :guild AND user_id = :actor_id FOR UPDATE"
        ),
        params,
    )
    if membership != "ACTIVE":
        raise ApiError(
            403,
            "guild_membership_required",
            "해당 길드의 활성 멤버만 이 작업을 수행할 수 있습니다."
            if actor is not None
            else "해당 길드의 활성 멤버만 내전을 만들 수 있습니다.",
        )
    channel = config["recruitment_channel_id"] if actor is None else None
    if actor is None and channel is None:
        raise ApiError(
            409,
            "recruitment_channel_not_configured",
            "Discord 모집 채널을 먼저 설정해야 내전을 만들 수 있습니다.",
        )
    season = await connection.scalar(
        text("SELECT id FROM seasons WHERE guild_id = :guild AND status = 'ACTIVE'"), params
    )
    if season is None:
        if actor is not None:
            raise ApiError(409, "guild_setup_required", "길드 초기 설정을 먼저 완료해야 합니다.")
        raise ApiError(
            409, "no_active_season", "길드에 진행 중인 시즌이 없어 내전을 만들 수 없습니다."
        )
    number = await connection.scalar(
        text(
            "UPDATE guild_counters SET next_value = next_value + 1 "
            "WHERE guild_id = :guild AND counter_key = 'RECRUITMENT' "
            "RETURNING next_value - 1"
        ),
        params,
    )
    if number is None:
        raise ApiError.from_status(500)
    label = start.astimezone(timezone(timedelta(hours=9))).strftime("%Y-%m-%d %H:%M")
    title = f"일반 내전 모집 #{number} · 예상 시작 {label} (KST)"
    values = {**params, "season": season, "number": number, "title": title}
    await connection.execute(
        text("""
        INSERT INTO recruitments
            (guild_id, id, season_id, display_number, title, target_count, best_of, hard_fearless,
             status, revision, channel_id, message_id, created_by, created_at,
             updated_by, updated_at)
        VALUES (:guild, :id, :season, :number, :title, 10, :best_of, :fearless,
                'OPEN', 0, NULL, NULL, :actor_id, :now, :actor_id, :now)
    """),
        values,
    )
    await connection.execute(
        text("""
        INSERT INTO recruitment_entry_drafts
            (guild_id, recruitment_id, revision, updated_by, updated_at)
        VALUES (:guild, :id, 0, :actor_id, :now)
    """),
        values,
    )
    snapshot = {
        "recruitment": {
            "id": str(params["id"]),
            "displayNumber": number,
            "title": title,
            "status": "OPEN",
            "targetCount": 10,
            "bestOf": params["best_of"],
            "hardFearless": params["fearless"],
            "revision": 0,
            "channelId": None,
            "messageId": None,
            "createdById": params["actor_id"],
            "updatedAt": timestamp(params["now"]),
        },
        "participants": [],
        "links": {"entry": f"/g/{params['guild']}/recruitments/{params['id']}/entry"},
    }
    values.update(snapshot=json.dumps(snapshot, ensure_ascii=False), target=str(params["id"]))
    if channel is not None:
        notification = {
            "schemaVersion": 1,
            "recruitmentId": str(params["id"]),
            "snapshot": snapshot,
        }
        await connection.execute(
            text("""
            INSERT INTO bot_notification_outbox
              (id, guild_id, deduplication_key, discord_nonce, channel_id, notification_type,
               payload, created_at, available_at)
            VALUES (:notification_id, :guild, :dedup, :nonce, :channel,
                    'RECRUITMENT_MESSAGE_CREATE',
                    CAST(:payload AS jsonb), :now, :now)
            ON CONFLICT (guild_id, deduplication_key) DO NOTHING
        """),
            {
                **values,
                "dedup": f"recruitment:{params['id']}:initial-message",
                "nonce": base64.urlsafe_b64encode(params["notification_id"].bytes)
                .decode()
                .rstrip("="),
                "channel": channel,
                "payload": json.dumps(notification, ensure_ascii=False),
            },
        )
    await connection.execute(
        text("""
        INSERT INTO admin_audit_log
          (guild_id, id, principal_type, principal_id, action, target_type, target_id,
           checkpoint_id, request_id, before_summary, after_summary, result, note, created_at)
        VALUES (:guild, :audit_id, 'USER', :actor_id, 'RECRUITMENT_CREATE', 'RECRUITMENT', :target,
                NULL, :request_id, NULL, CAST(:snapshot AS jsonb), 'APPLIED', NULL, :now)
    """),
        values,
    )
    await connection.execute(
        text("""
        INSERT INTO outbox_events (id, guild_id, topic, aggregate_type, aggregate_id,
                                   aggregate_revision, event_type, payload,
                                   occurred_at, available_at)
        VALUES (:event_id, :guild, :topic, 'RECRUITMENT', :target, 0, 'RECRUITMENT_CREATED',
                CAST(:event AS jsonb), :now, :now)
    """),
        {
            **values,
            "topic": f"guild:{params['guild']}:recruitment:{params['id']}",
            "event": json.dumps(
                {"type": "invalidate", "resourceId": str(params["id"]), "revision": 0}
            ),
        },
    )
    completed = await connection.scalar(
        text("""
        UPDATE idempotency_records
        SET state = 'COMPLETED', status_code = 201, response_body = CAST(:snapshot AS jsonb),
            completed_at = :now
        WHERE id = :idem_id AND scope_key = :guild AND guild_id = :guild AND state = 'IN_PROGRESS'
        RETURNING id
    """),
        values,
    )
    if completed is None:
        raise ApiError.from_status(500)
    return snapshot


def require_bot(request: Request, settings: Settings) -> None:
    token = settings.bot_service_token
    if token is None or not token.get_secret_value().strip():
        raise ApiError(503, "bot_service_unavailable", "Bot 인증 서비스를 사용할 수 없습니다.")
    headers = request.headers.getlist("authorization")
    if (
        len(headers) != 1
        or not headers[0].startswith("Bearer ")
        or not hmac.compare_digest(headers[0][7:].encode(), token.get_secret_value().encode())
    ):
        raise ApiError.from_status(401)


async def command_principal(
    request: Request, settings: Settings, database: Database
) -> tuple[bool, dict[str, Any] | None]:
    bot = "authorization" in request.headers
    principal = None
    if bot:
        require_bot(request, settings)
    else:
        principal = await resolve_session(request, database, settings)
    return bot, principal


async def command_json(request: Request) -> Any:
    media = request.headers.get("content-type", "").split(";", 1)[0].strip().lower()
    if media != "application/json" and not (
        media.startswith("application/") and media.endswith("+json")
    ):
        raise ApiError.from_status(415)
    try:
        # Axum first decodes Json<Value>: duplicate object fields use the final value.
        raw = json.loads(
            (await request.body()).decode("utf-8"), parse_constant=reject_json_constant
        )
    except ValueError, UnicodeDecodeError:
        raise ApiError.from_status(400) from None
    return raw


def command_key(request: Request, bot: bool) -> str:
    keys = request.headers.getlist("idempotency-key")
    key = keys[0] if len(keys) == 1 else ""
    if (bot and not is_snowflake(key)) or (
        not bot and not re.fullmatch(r"[\x21-\x7e]{1,128}", key)
    ):
        raise ApiError(
            400,
            "invalid_idempotency_key",
            "Idempotency-Key는 Discord interaction ID여야 합니다."
            if bot
            else "Idempotency-Key 헤더가 필요합니다.",
            None if bot else {"maxLength": 128},
        )
    return key


def validate_actor(actor: Actor) -> None:
    for name in ("globalDisplayName", "displayName"):
        value = getattr(actor, name)
        if not value.strip() or len(value) > 100:
            raise invalid_bot(f"actor.{name}")
        setattr(actor, name, value.strip())
    if actor.avatarHash is not None and len(actor.avatarHash.encode()) > 128:
        raise invalid_bot("actor.avatarHash")
    if len(actor.roleIds) > 250:
        raise invalid_bot("actor.roleIds")


def router(settings: Settings, database: Database) -> APIRouter:
    routes = APIRouter()

    @routes.post("/api/guilds/{guild_id}/recruitments")
    async def create(request: Request, guild_id: str) -> Response:
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
        for attempt in range(3):
            try:
                async with database.connection(transaction=True) as connection:
                    snapshot = await create_attempt(connection, params, actor, start)
                response = success(request, snapshot)
                response.status_code = 201
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

    return routes
