"""Dummy-user linked-resource mutations with Rust-compatible validation."""

import hashlib
import math
import re
import uuid
from datetime import UTC, datetime, timedelta
from typing import Any

from pydantic import ValidationError
from pydantic_core import to_json
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.config import is_snowflake
from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.riot_identity import rust_trim
from monkeybot_api.db.database import Database
from monkeybot_api.db.transactions import mutation
from monkeybot_api.repositories.dev import user_links as repository
from monkeybot_api.repositories.dev import users
from monkeybot_api.schemas.common import user_id as valid_user_id
from monkeybot_api.schemas.dev.user_links import (
    ReplaceChampionPreferences,
    UpsertMembership,
    UpsertRiotAccount,
)
from monkeybot_api.schemas.dev.users import LANES, ReplayUser

MEMBERSHIP_ROUTE = "/api/dev/dummy-users/{userId}/guilds/{guildId}"
PREFERENCES_ROUTE = "/api/dev/dummy-users/{userId}/champion-preferences/{lane}"
RIOT_ROUTE = "/api/dev/dummy-users/{userId}/riot-account"


def parse_user(value: str) -> str:
    try:
        return valid_user_id(value)
    except ValueError:
        raise ApiError.from_status(400) from None


def parse_guild(value: str) -> str:
    if not is_snowflake(value):
        raise ApiError.from_status(400)
    return value


def parse_lane(value: str) -> str:
    if value not in LANES:
        raise invalid("lane")
    return value


def invalid(field: str) -> ApiError:
    return ApiError(
        422, "invalid_dummy_user_input", "더미 사용자 입력값을 확인해 주세요.", {"field": field}
    )


def validate_text(value: str, field: str, maximum: int) -> None:
    if value != rust_trim(value) or not 1 <= len(value) <= maximum:
        raise invalid(field)


def validate_membership(body: UpsertMembership) -> None:
    validate_text(body.displayName, "displayName", 100)
    if body.laneMmr is not None and any(
        not math.isfinite(value) or not 0 <= value <= 10_000
        for value in body.laneMmr.model_dump().values()
    ):
        raise invalid("laneMmr")


def validate_champions(body: ReplaceChampionPreferences) -> None:
    ids = body.championIds
    if (
        len(ids) > 10
        or len(set(ids)) != len(ids)
        or any(re.fullmatch(r"[A-Za-z0-9]{1,64}", value) is None for value in ids)
    ):
        raise invalid("championIds")


def validate_riot(body: UpsertRiotAccount) -> None:
    for value, field, maximum in (
        (body.platform, "platform", 16),
        (body.routingRegion, "routingRegion", 16),
        (body.gameName, "gameName", 64),
        (body.tagLine, "tagLine", 16),
    ):
        validate_text(value, field, maximum)
    if body.profileIconId is not None and body.profileIconId < 0:
        raise invalid("profileIconId")
    if body.mainPosition is not None and body.mainPosition not in (*LANES, "FLEX"):
        raise invalid("mainPosition")


def params(
    actor: str,
    request_id: str,
    key: str,
    method: str,
    route: str,
    payload: Any,
    *,
    guild: str | None = None,
) -> dict[str, Any]:
    now = datetime.now(UTC)
    return {
        "actor": actor,
        "request_id": request_id,
        "key": key,
        "method": method,
        "route": route,
        "scope": guild or "GLOBAL",
        "guild": guild,
        "hash": hashlib.sha256(to_json(payload)).digest(),
        "idem_id": uuid.uuid7(),
        "now": now,
        "expires": now + timedelta(hours=24),
    }


def replay(value: dict[str, Any]) -> dict[str, Any]:
    try:
        return ReplayUser.model_validate(value["user"]).model_dump(mode="json")
    except KeyError, ValueError, TypeError, ValidationError:
        raise users.invariant() from None


async def require_active(c: AsyncConnection, user: str) -> None:
    if await repository.lock_profile(c, user) == "ARCHIVED":
        raise ApiError(
            409,
            "dummy_user_archived",
            "보관된 더미 사용자는 복원한 뒤 수정하거나 배치할 수 있습니다.",
        )


async def upsert_membership(
    database: Database,
    user: str,
    guild: str,
    actor: str,
    request_id: str,
    key: str,
    body: UpsertMembership,
) -> dict[str, Any]:
    validate_membership(body)
    p = params(
        actor,
        request_id,
        key,
        "PUT",
        MEMBERSHIP_ROUTE,
        [user, guild, body.model_dump(mode="json")],
        guild=guild,
    )
    p.update({"user": user, "name": body.displayName, "status": body.status, "lane": body.mainLane})

    async def operation(c: AsyncConnection) -> dict[str, Any]:
        stored = await repository.acquire(c, p)
        if stored is not None:
            return replay(stored)
        await require_active(c, user)
        await repository.guild_exists(c, guild)
        previous = await repository.previous_membership(c, user, guild)
        await repository.upsert_membership(c, p)
        if body.laneMmr is not None:
            await repository.upsert_lane_mmr(c, p, body.laneMmr.model_dump())
        await repository.touch_members_state(c, p)
        action = "DUMMY_MEMBER_UPDATED" if previous is not None else "DUMMY_MEMBER_ATTACHED"
        await repository.guild_audit(
            c,
            p,
            action,
            previous,
            {
                "displayName": body.displayName,
                "status": body.status,
                "mainLane": body.mainLane,
                "initialMmrProvided": body.laneMmr is not None,
            },
        )
        await repository.dummy_audit(
            c,
            p,
            action,
            "GUILD_MEMBER",
            f"{guild}:{user}",
            previous,
            {
                "guildId": guild,
                "displayName": body.displayName,
                "status": body.status,
                "mainLane": body.mainLane,
            },
        )
        result = await repository.snapshot(c, user)
        await repository.complete(c, p, result)
        return result

    return await mutation(database, operation)


async def remove_membership(
    database: Database,
    user: str,
    guild: str,
    actor: str,
    request_id: str,
    key: str,
) -> dict[str, Any]:
    p = params(actor, request_id, key, "DELETE", MEMBERSHIP_ROUTE, [user, guild], guild=guild)
    p["user"] = user

    async def operation(c: AsyncConnection) -> dict[str, Any]:
        stored = await repository.acquire(c, p)
        if stored is not None:
            return replay(stored)
        await repository.lock_profile(c, user)
        await repository.remove_membership(c, p)
        await repository.touch_members_state(c, p)
        await repository.guild_audit(c, p, "DUMMY_MEMBER_DETACHED", None, None)
        await repository.dummy_audit(
            c, p, "DUMMY_MEMBER_DETACHED", "GUILD_MEMBER", f"{guild}:{user}", None, None
        )
        result = await repository.snapshot(c, user)
        await repository.complete(c, p, result)
        return result

    return await mutation(database, operation)


async def replace_preferences(
    database: Database,
    user: str,
    lane: str,
    actor: str,
    request_id: str,
    key: str,
    body: ReplaceChampionPreferences,
) -> dict[str, Any]:
    validate_champions(body)
    p = params(actor, request_id, key, "PUT", PREFERENCES_ROUTE, [user, lane, body.championIds])
    p.update({"user": user, "lane": lane})

    async def operation(c: AsyncConnection) -> dict[str, Any]:
        stored = await repository.acquire(c, p)
        if stored is not None:
            return replay(stored)
        await require_active(c, user)
        await repository.replace_preferences(c, p, body.championIds)
        await repository.bump_profile(c, p)
        await repository.dummy_audit(
            c,
            p,
            "DUMMY_CHAMPION_PREFERENCES_UPDATED",
            "DUMMY_USER",
            user,
            None,
            {"lane": lane, "championIds": body.championIds},
        )
        result = await repository.snapshot(c, user)
        await repository.complete(c, p, result)
        return result

    return await mutation(database, operation)


async def upsert_riot(
    database: Database,
    user: str,
    actor: str,
    request_id: str,
    key: str,
    body: UpsertRiotAccount,
) -> dict[str, Any]:
    validate_riot(body)
    p = params(actor, request_id, key, "PUT", RIOT_ROUTE, [user, body.model_dump(mode="json")])
    p.update(
        {
            "user": user,
            "platform": body.platform,
            "region": body.routingRegion,
            "game": body.gameName,
            "tag": body.tagLine,
            "icon": body.profileIconId,
            "position": body.mainPosition,
        }
    )

    async def operation(c: AsyncConnection) -> dict[str, Any]:
        stored = await repository.acquire(c, p)
        if stored is not None:
            return replay(stored)
        await require_active(c, user)
        await repository.upsert_riot(c, p)
        await repository.bump_profile(c, p)
        await repository.dummy_audit(
            c,
            p,
            "DUMMY_RIOT_ACCOUNT_UPSERTED",
            "DUMMY_USER",
            user,
            None,
            {
                "platform": body.platform,
                "routingRegion": body.routingRegion,
                "gameName": body.gameName,
                "tagLine": body.tagLine,
            },
        )
        result = await repository.snapshot(c, user)
        await repository.complete(c, p, result)
        return result

    return await mutation(database, operation)


async def remove_riot(
    database: Database,
    user: str,
    actor: str,
    request_id: str,
    key: str,
) -> dict[str, Any]:
    p = params(actor, request_id, key, "DELETE", RIOT_ROUTE, user)
    p["user"] = user

    async def operation(c: AsyncConnection) -> dict[str, Any]:
        stored = await repository.acquire(c, p)
        if stored is not None:
            return replay(stored)
        await repository.lock_profile(c, user)
        await repository.remove_riot(c, p)
        await repository.bump_profile(c, p)
        await repository.dummy_audit(
            c, p, "DUMMY_RIOT_ACCOUNT_REMOVED", "DUMMY_USER", user, None, None
        )
        result = await repository.snapshot(c, user)
        await repository.complete(c, p, result)
        return result

    return await mutation(database, operation)
