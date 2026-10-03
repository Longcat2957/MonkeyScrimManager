from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.time import timestamp
from monkeybot_api.db.queries import one, params
from monkeybot_api.repositories.guilds import queries as sql
from monkeybot_api.schemas.guilds.settings import Catalog

VALUES = {
    "display_name": "displayName",
    "operator_role_id": "operatorRoleId",
    "recruitment_channel_id": "recruitmentChannelId",
    "entry_channel_id": "entryChannelId",
    "result_channel_id": "resultChannelId",
    "log_channel_id": "logChannelId",
    "is_externally_public": "isExternallyPublic",
}


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


async def locked_settings(c: AsyncConnection, guild: str, actor: str) -> dict[str, Any] | None:
    return await one(c, sql.SETTINGS_LOCK, guild, actor)


async def selection_exists(c: AsyncConnection, guild: str, field: str, value: str) -> bool:
    query = sql.ROLE_EXISTS if field == "operatorRoleId" else sql.CHANNEL_EXISTS
    return bool(await c.scalar(text(query), params(guild, value)))


async def persist_settings(
    c: AsyncConnection, p: dict[str, Any], after: dict[str, Any], previous_revision: int
) -> int:
    updated = await c.execute(
        text(sql.SETTINGS_UPDATE),
        params(
            *[after[field] for field in VALUES.values()],
            previous_revision + 1,
            p["actor_id"],
            p["now"],
            p["guild"],
            previous_revision,
        ),
    )
    return updated.rowcount
