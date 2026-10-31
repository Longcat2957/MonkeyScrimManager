"""Validation and workflows for development dummy-user reads and creation."""

import hashlib
import math
import re
import uuid
from datetime import UTC, datetime
from typing import Any

from pydantic_core import to_json
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.config import is_snowflake
from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.riot_identity import rust_trim
from monkeybot_api.db.database import Database
from monkeybot_api.db.transactions import mutation, repeatable_read
from monkeybot_api.repositories.dev import users as repository
from monkeybot_api.schemas.common import user_id as valid_user_id
from monkeybot_api.schemas.dev.users import CreateUser, ReplayUser

CREATE_ROUTE = "/api/dev/dummy-users"
BULK_ROUTE = "/api/dev/dummy-users/bulk"


def invalid(field: str) -> ApiError:
    return ApiError(
        422, "invalid_dummy_user_input", "더미 사용자 입력값을 확인해 주세요.", {"field": field}
    )


def _text(value: str, field: str, minimum: int, maximum: int) -> None:
    if value != rust_trim(value) or not minimum <= len(value) <= maximum:
        raise invalid(field)


def _optional(value: str | None, field: str, maximum: int, *, empty: bool = False) -> None:
    if value is not None and (
        value != rust_trim(value) or len(value) > maximum or (not empty and not value)
    ):
        raise invalid(field)


def validate_create(command: CreateUser) -> None:
    if re.fullmatch(r"[a-z0-9][a-z0-9_-]{0,63}", command.stableKey) is None:
        raise invalid("stableKey")
    _text(command.displayName, "displayName", 1, 100)
    _optional(command.avatarSeed, "avatarSeed", 128)
    _optional(command.notes, "notes", 1000, empty=True)
    guilds: set[str] = set()
    for membership in command.guildMemberships:
        if not is_snowflake(membership.guildId):
            raise invalid("guildMemberships")
        _text(membership.displayName, "displayName", 1, 100)
        if membership.guildId in guilds:
            raise invalid("guildMemberships")
        guilds.add(membership.guildId)
        if membership.laneMmr is not None:
            if any(
                not math.isfinite(value) or not 0 <= value <= 10_000
                for value in membership.laneMmr.model_dump().values()
            ):
                raise invalid("laneMmr")
    riot = command.riotAccount
    if riot is not None:
        for value, field, maximum in (
            (riot.platform, "platform", 16),
            (riot.routingRegion, "routingRegion", 16),
            (riot.gameName, "gameName", 64),
            (riot.tagLine, "tagLine", 16),
        ):
            _text(value, field, 1, maximum)
        if riot.profileIconId is not None and riot.profileIconId < 0:
            raise invalid("profileIconId")


def parse_filters(query_params: Any) -> dict[str, Any]:
    accepted = {"query", "state", "guildId", "groupId", "limit", "offset"}
    pairs = list(query_params.multi_items())
    if any(key not in accepted for key, _ in pairs) or len({key for key, _ in pairs}) != len(pairs):
        raise ApiError.from_status(400)
    values = dict(pairs)
    state = values.get("state")
    if state not in (None, "ACTIVE", "ARCHIVED"):
        raise ApiError.from_status(400)
    guild = values.get("guildId")
    if guild is not None and not is_snowflake(guild):
        raise ApiError.from_status(400)
    group_raw = values.get("groupId")
    try:
        group = uuid.UUID(group_raw) if group_raw is not None else None
    except ValueError:
        raise ApiError.from_status(400) from None
    try:
        limit = int(values.get("limit", "50"))
        offset = int(values.get("offset", "0"))
    except ValueError:
        raise ApiError.from_status(400) from None
    if not 1 <= limit <= 100 or offset < 0:
        raise invalid("pagination")
    search = values.get("query")
    return {
        "query": rust_trim(search) or None if search is not None else None,
        "state": state,
        "guild": guild,
        "group": group,
        "limit": limit,
        "offset": offset,
    }


def parse_user(value: str) -> str:
    try:
        return valid_user_id(value)
    except ValueError:
        raise ApiError.from_status(400) from None


async def list_users(database: Database, filters: dict[str, Any]) -> dict[str, Any]:
    async with database.connection(transaction=True) as c:
        await repeatable_read(c)
        base, total = await repository.list_base(c, filters)
        users = await repository.snapshots(c, base)
    return {"users": users, "total": total}


async def get_user(database: Database, user: str) -> dict[str, Any]:
    async with database.connection(transaction=True) as c:
        await repeatable_read(c)
        base = await repository.get_base(c, user)
        return (await repository.snapshots(c, [base]))[0]


async def catalog(database: Database) -> dict[str, Any]:
    async with database.connection() as c:
        return {"guilds": await repository.guild_catalog(c)}


async def create(
    database: Database,
    actor: str,
    request_id: str,
    key: str,
    commands: list[CreateUser],
    *,
    bulk: bool,
) -> dict[str, Any]:
    if bulk and not 1 <= len(commands) <= 100:
        raise invalid("users")
    keys: set[str] = set()
    for command in commands:
        validate_create(command)
        if command.stableKey in keys:
            raise ApiError(
                409, "dummy_stable_key_conflict", "이미 사용 중인 더미 사용자 식별 키입니다."
            )
        keys.add(command.stableKey)
    raw = [command.model_dump(mode="json") for command in commands]
    serialized = raw if bulk else raw[0]
    request_hash = hashlib.sha256(to_json(serialized)).digest()
    route = BULK_ROUTE if bulk else CREATE_ROUTE

    async def operation(c: AsyncConnection) -> dict[str, Any]:
        now = datetime.now(UTC)
        record, replay = await repository.acquire_create(c, actor, key, route, request_hash, now)
        if replay is not None:
            try:
                if bulk:
                    users = [
                        ReplayUser.model_validate(value).model_dump() for value in replay["users"]
                    ]
                    if not 1 <= len(users) <= 100:
                        raise ValueError("invalid total")
                    return {"total": len(users), "users": users}
                return ReplayUser.model_validate(replay["user"]).model_dump()
            except KeyError, TypeError, ValueError:
                raise repository.invariant() from None
        assert record is not None
        ids = [
            await repository.create_one(c, command, actor, request_id, now) for command in commands
        ]
        base = [await repository.get_base(c, user) for user in ids]
        users = await repository.snapshots(c, base)
        response = {"users": users} if bulk else {"user": users[0]}
        await repository.complete_create(c, record, response, now)
        return {"total": len(users), "users": users} if bulk else users[0]

    return await mutation(database, operation)
