from datetime import UTC, datetime
from typing import Any

from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.text import trim
from monkeybot_api.core.time import timestamp
from monkeybot_api.db.database import Database
from monkeybot_api.db.transactions import repeatable_read
from monkeybot_api.repositories.effects import audit
from monkeybot_api.repositories.guilds import settings as repository
from monkeybot_api.repositories.permissions import access_failure, authorize
from monkeybot_api.schemas.guilds.settings import Catalog, CatalogChannel, UpdateSettings


def field_error(code: str, field: str) -> ApiError:
    message = {
        "invalid_guild_settings": "길드 설정 값을 확인해 주세요.",
        "invalid_settings_selection": "현재 Discord 서버에서 선택할 수 없는 설정입니다.",
        "invalid_discord_catalog": "Discord 역할 및 채널 목록이 올바르지 않습니다.",
    }[code]
    return ApiError(422, code, message, {"field": field})


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


async def update_settings(
    c: AsyncConnection, p: dict[str, Any], body: UpdateSettings
) -> dict[str, Any]:
    guild, actor = p["guild"], p["actor_id"]
    row = await repository.locked_settings(c, guild, actor)
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
    before = repository.settings_values(row)
    after = body.model_dump(exclude={"expectedRevision"})
    for field in list(repository.VALUES.values())[1:-1]:
        if before[field] != after[field] and after[field] is not None:
            if not await repository.selection_exists(c, guild, field, after[field]):
                raise field_error("invalid_settings_selection", field)
    revision = row["revision"] + 1
    if revision >= 2**63:
        raise ApiError.from_status(500)
    p["now"] = datetime.now(UTC)
    changed = await repository.persist_settings(c, p, after, row["revision"])
    if changed != 1:
        raise ApiError.from_status(500)
    await audit(c, p, "GUILD_SETTINGS_UPDATED", "GUILD_CONFIG", guild, before, after)
    return await repository.snapshot(c, guild)


async def get_settings(database: Database, guild: str, actor: str) -> dict[str, Any]:
    async with database.connection(transaction=True) as c:
        await repeatable_read(c)
        await authorize(c, guild, actor)
        return await repository.snapshot(c, guild)
