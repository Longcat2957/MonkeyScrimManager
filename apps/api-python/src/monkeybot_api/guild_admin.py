import asyncio
import uuid
from collections.abc import Awaitable, Callable
from datetime import UTC, datetime
from typing import Annotated, Any, Literal

from fastapi import APIRouter, Request
from pydantic import AfterValidator, Field
from sqlalchemy import text
from sqlalchemy.exc import DBAPIError
from sqlalchemy.ext.asyncio import AsyncConnection
from starlette.responses import JSONResponse

from monkeybot_api import guild_admin_queries as sql
from monkeybot_api.auth import require_csrf
from monkeybot_api.config import Settings, is_snowflake
from monkeybot_api.context import timestamp
from monkeybot_api.database import Database
from monkeybot_api.delivery_effects import audit
from monkeybot_api.errors import ApiError, success
from monkeybot_api.json_body import read_json
from monkeybot_api.notification_delivery import one, params, valid_message
from monkeybot_api.recruitment_create import require_bot
from monkeybot_api.series_contracts import Strict
from monkeybot_api.session import resolve_session

Snowflake = Annotated[str, AfterValidator(valid_message)]
# Rust str::trim uses Unicode White_Space (Python additionally strips U+001C..U+001F).
WHITESPACE = (
    "\t\n\v\f\r \x85\xa0\u1680\u2000\u2001\u2002\u2003\u2004\u2005"
    "\u2006\u2007\u2008\u2009\u200a\u2028\u2029\u202f\u205f\u3000"
)
VALUES = {
    "display_name": "displayName",
    "operator_role_id": "operatorRoleId",
    "recruitment_channel_id": "recruitmentChannelId",
    "entry_channel_id": "entryChannelId",
    "result_channel_id": "resultChannelId",
    "log_channel_id": "logChannelId",
    "is_externally_public": "isExternallyPublic",
}


class CatalogRole(Strict):
    id: Snowflake
    name: str
    position: int = Field(ge=-(2**31), le=2**31 - 1)
    managed: bool


class CatalogChannel(Strict):
    id: Snowflake
    name: str
    parentName: str | None = None
    position: int = Field(ge=-(2**31), le=2**31 - 1)
    kind: Literal["TEXT", "ANNOUNCEMENT"]


class Catalog(Strict):
    roles: list[CatalogRole]
    channels: list[CatalogChannel]


class UpdateSettings(Strict):
    displayName: str
    operatorRoleId: Snowflake
    recruitmentChannelId: Snowflake | None = None
    entryChannelId: Snowflake | None = None
    resultChannelId: Snowflake | None = None
    logChannelId: Snowflake | None = None
    isExternallyPublic: bool
    expectedRevision: int = Field(ge=-(2**63), le=2**63 - 1)


def field_error(code: str, field: str) -> ApiError:
    message = {
        "invalid_guild_settings": "길드 설정 값을 확인해 주세요.",
        "invalid_settings_selection": "현재 Discord 서버에서 선택할 수 없는 설정입니다.",
        "invalid_discord_catalog": "Discord 역할 및 채널 목록이 올바르지 않습니다.",
    }[code]
    return ApiError(422, code, message, {"field": field})


def trim(value: str) -> str:
    return value.strip(WHITESPACE)


def normalized(value: str, code: str, field: str) -> str:
    value = trim(value)
    if not 1 <= len(value) <= 100:
        raise field_error(code, field)
    return value


def normalize_catalog(guild: str, body: Catalog) -> None:
    code = "invalid_discord_catalog"
    if len(body.roles) > 250:
        raise field_error(code, "roles")
    if len(body.channels) > 500:
        raise field_error(code, "channels")
    for items, field in ((body.roles, "roles"), (body.channels, "channels")):
        seen = set()
        for item in items:
            if item.position < 0 or item.id in seen or (field == "roles" and item.id == guild):
                raise field_error(code, field)
            seen.add(item.id)
            item.name = normalized(item.name, code, field + ".name")
            if isinstance(item, CatalogChannel) and item.parentName is not None:
                item.parentName = normalized(item.parentName, code, "channels.parentName")


def normalize_settings(guild: str, body: UpdateSettings) -> None:
    if body.expectedRevision < 0:
        raise ApiError(422, "invalid_expected_revision", "expectedRevision은 0 이상이어야 합니다.")
    if body.recruitmentChannelId is None:
        raise ApiError(
            422,
            "recruitment_channel_required",
            "모집 채널은 반드시 설정해야 합니다.",
            {"field": "recruitmentChannelId"},
        )
    body.displayName = normalized(body.displayName, "invalid_guild_settings", "displayName")
    if body.operatorRoleId == guild:
        raise field_error("invalid_guild_settings", "operatorRoleId")


def guild_path(guild: str) -> None:
    if not is_snowflake(guild):
        raise ApiError.from_status(400)


async def access_failure(c: AsyncConnection, guild: str, actor: str) -> None:
    if await c.scalar(text(sql.MEMBER), params(guild, actor)):
        raise ApiError(403, "operator_required", "길드 운영자 권한이 필요합니다.")
    raise ApiError(404, "not_found", "요청한 길드를 찾을 수 없습니다.")


async def authorize(c: AsyncConnection, guild: str, actor: str) -> None:
    if not await c.scalar(text(sql.AUTHORIZE), params(guild, actor)):
        await access_failure(c, guild, actor)


def settings_values(row: dict[str, Any]) -> dict[str, Any]:
    return {wire: row[column] for column, wire in VALUES.items()}


async def snapshot(c: AsyncConnection, guild: str) -> dict[str, Any]:
    row = await one(c, sql.SETTINGS_GET, guild)
    if row is None:
        raise ApiError(404, "not_found", "요청한 길드를 찾을 수 없습니다.")
    synced = await c.scalar(text(sql.CATALOG_TIME), params(guild))
    roles = (await c.execute(text(sql.ROLES), params(guild))).mappings().all()
    channels = (await c.execute(text(sql.CHANNELS), params(guild))).mappings().all()
    return {
        "settings": settings_values(row),
        "revision": row["revision"],
        "updatedAt": timestamp(row["updated_at"]),
        "updatedBy": row["updated_by"],
        "catalogUpdatedAt": timestamp(synced) if synced else None,
        "roles": [dict(r) for r in roles],
        "channels": [
            {"id": r["id"], "name": r["name"], "parentName": r["parent_name"]} for r in channels
        ],
    }


async def replace_catalog(c: AsyncConnection, guild: str, body: Catalog) -> dict[str, Any]:
    if not await c.scalar(text(sql.CATALOG_LOCK), params(guild)):
        raise ApiError(404, "not_found", "요청한 길드를 찾을 수 없습니다.")
    synced = await c.scalar(text(sql.CLOCK))
    await c.execute(text(sql.DELETE_ROLES), params(guild))
    await c.execute(text(sql.DELETE_CHANNELS), params(guild))
    if body.roles:
        await c.execute(
            text("""
            INSERT INTO guild_discord_roles (guild_id, role_id, name, position, managed, updated_at)
            VALUES (:guild, :id, :name, :position, :managed, :now)
        """),
            [{**role.model_dump(), "guild": guild, "now": synced} for role in body.roles],
        )
    if body.channels:
        await c.execute(
            text("""
            INSERT INTO guild_discord_channels
                (guild_id, channel_id, name, parent_name, position, kind, updated_at)
            VALUES (:guild, :id, :name, :parentName, :position, :kind, :now)
        """),
            [{**channel.model_dump(), "guild": guild, "now": synced} for channel in body.channels],
        )
    await c.execute(text(sql.CATALOG_STATE), params(guild, synced))
    return {"syncedAt": timestamp(synced)}


async def update_settings(
    c: AsyncConnection, p: dict[str, Any], body: UpdateSettings
) -> dict[str, Any]:
    guild, actor = p["guild"], p["actor_id"]
    row = await one(c, sql.SETTINGS_LOCK, guild, actor)
    if row is None:
        await access_failure(c, guild, actor)
        raise AssertionError("unreachable")
    if row["revision"] != body.expectedRevision:
        raise ApiError(
            409,
            "revision_conflict",
            "최신 상태를 다시 확인해야 합니다.",
            {
                "expectedRevision": body.expectedRevision,
                "currentRevision": row["revision"],
                "updatedAt": timestamp(row["updated_at"]),
                "updatedBy": row["updated_by"],
            },
        )
    before = settings_values(row)
    after = body.model_dump(exclude={"expectedRevision"})
    for field in list(VALUES.values())[1:-1]:
        if before[field] != after[field] and after[field] is not None:
            query = sql.ROLE_EXISTS if field == "operatorRoleId" else sql.CHANNEL_EXISTS
            if not await c.scalar(text(query), params(guild, after[field])):
                raise field_error("invalid_settings_selection", field)
    revision = row["revision"] + 1
    if revision >= 2**63:
        raise ApiError.from_status(500)
    p["now"] = datetime.now(UTC)
    updated = await c.execute(
        text(sql.SETTINGS_UPDATE),
        params(
            *[after[field] for field in VALUES.values()],
            revision,
            actor,
            p["now"],
            guild,
            row["revision"],
        ),
    )
    if updated.rowcount != 1:
        raise ApiError.from_status(500)
    await audit(c, p, "GUILD_SETTINGS_UPDATED", "GUILD_CONFIG", guild, before, after)
    return await snapshot(c, guild)


async def mutation(
    database: Database, operation: Callable[[AsyncConnection], Awaitable[dict[str, Any]]]
) -> dict[str, Any]:
    for attempt in range(3):
        try:
            async with database.connection(transaction=True) as c:
                result = await operation(c)
            return result
        except DBAPIError as exc:
            if getattr(exc.orig, "sqlstate", "") != "40P01":
                raise
            if attempt == 2:
                raise ApiError(
                    503,
                    "database_busy",
                    "데이터베이스가 사용 중입니다. 잠시 후 다시 시도해 주세요.",
                ) from None
            await asyncio.sleep(0.025 * 2**attempt)
    raise AssertionError("unreachable")


def router(settings: Settings, database: Database) -> APIRouter:
    routes = APIRouter()

    @routes.put("/internal/guilds/{guild_id}/discord-catalog")
    async def catalog(request: Request, guild_id: str) -> JSONResponse:
        guild_path(guild_id)
        require_bot(request, settings)
        body = await read_json(request, Catalog)
        normalize_catalog(guild_id, body)
        return success(
            request, await mutation(database, lambda c: replace_catalog(c, guild_id, body))
        )

    @routes.api_route("/api/guilds/{guild_id}/settings", methods=["GET", "HEAD"])
    async def get_settings(request: Request, guild_id: str) -> JSONResponse:
        guild_path(guild_id)
        principal = await resolve_session(request, database, settings)
        async with database.connection(transaction=True) as c:
            await c.execute(text("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ"))
            await authorize(c, guild_id, principal["user_id"])
            result = await snapshot(c, guild_id)
        return success(request, result)

    @routes.put("/api/guilds/{guild_id}/settings")
    async def put_settings(request: Request, guild_id: str) -> JSONResponse:
        guild_path(guild_id)
        principal = await resolve_session(request, database, settings)
        body = await read_json(request, UpdateSettings)
        require_csrf(request, settings, principal["csrf_secret_hash"])
        normalize_settings(guild_id, body)
        p = {
            "guild": guild_id,
            "actor_id": principal["user_id"],
            "request_id": request.state.request_id,
            "audit_id": uuid.uuid7(),
        }
        return success(request, await mutation(database, lambda c: update_settings(c, p, body)))

    return routes
