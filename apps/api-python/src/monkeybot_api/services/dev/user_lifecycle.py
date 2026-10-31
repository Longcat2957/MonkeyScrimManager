"""Validation and transaction boundaries for dummy-user lifecycle operations."""

import hashlib
from datetime import UTC, datetime
from typing import Any

from pydantic_core import to_json
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.riot_identity import rust_trim
from monkeybot_api.db.database import Database
from monkeybot_api.db.transactions import mutation, repeatable_read
from monkeybot_api.repositories.dev import user_lifecycle as repository
from monkeybot_api.repositories.dev import users as users_repository
from monkeybot_api.schemas.dev.user_lifecycle import UpdateUser
from monkeybot_api.schemas.dev.users import ReplayUser
from monkeybot_api.services.dev.users import invalid


def _revision(value: int) -> None:
    if value < 0:
        raise ApiError(422, "invalid_expected_revision", "expectedRevision은 0 이상이어야 합니다.")


def _text(value: str, field: str, minimum: int, maximum: int) -> None:
    if value != rust_trim(value) or not minimum <= len(value) <= maximum:
        raise invalid(field)


def validate_update(command: UpdateUser) -> None:
    _revision(command.expectedRevision)
    if (
        command.displayName is None
        and command.avatarSeed is None
        and command.notes is None
        and command.privacy is None
    ):
        raise invalid("body")
    if command.displayName is not None:
        _text(command.displayName, "displayName", 1, 100)
    if command.avatarSeed is not None:
        _text(command.avatarSeed, "avatarSeed", 1, 128)
    if command.notes is not None:
        _text(command.notes, "notes", 0, 1000)


def _replay_user(body: dict[str, Any]) -> dict[str, Any]:
    try:
        return ReplayUser.model_validate(body["user"]).model_dump()
    except KeyError, TypeError, ValueError:
        raise users_repository.invariant() from None


async def update_user(
    database: Database,
    user: str,
    actor: str,
    request_id: str,
    key: str,
    command: UpdateUser,
) -> dict[str, Any]:
    validate_update(command)
    payload = [user, command.model_dump(mode="json")]
    request_hash = hashlib.sha256(to_json(payload)).digest()

    async def operation(c: AsyncConnection) -> dict[str, Any]:
        now = datetime.now(UTC)
        record, replay = await repository.acquire(
            c, actor, "PATCH", repository.ROUTE, key, request_hash, now
        )
        if replay is not None:
            return _replay_user(replay)
        assert record is not None
        result = await repository.apply_update(c, user, actor, request_id, command, now)
        await repository.complete(c, record, {"user": result}, now)
        return result

    return await mutation(database, operation)


async def set_archived(
    database: Database,
    user: str,
    actor: str,
    request_id: str,
    key: str,
    expected_revision: int,
    *,
    archived: bool,
) -> dict[str, Any]:
    _revision(expected_revision)
    route = repository.ROUTE + ("/archive" if archived else "/restore")
    request_hash = hashlib.sha256(to_json([user, expected_revision, archived])).digest()

    async def operation(c: AsyncConnection) -> dict[str, Any]:
        now = datetime.now(UTC)
        record, replay = await repository.acquire(c, actor, "POST", route, key, request_hash, now)
        if replay is not None:
            return _replay_user(replay)
        assert record is not None
        result = await repository.apply_archived(
            c, user, actor, request_id, expected_revision, archived, now
        )
        await repository.complete(c, record, {"user": result}, now)
        return result

    return await mutation(database, operation)


async def deletion_impact(database: Database, user: str) -> dict[str, Any]:
    async with database.connection(transaction=True) as c:
        await repeatable_read(c)
        await users_repository.get_base(c, user)
        return await repository.deletion_impact(c, user)


async def delete_user(
    database: Database, user: str, actor: str, request_id: str, key: str
) -> dict[str, Any]:
    request_hash = hashlib.sha256(to_json(user)).digest()

    async def operation(c: AsyncConnection) -> dict[str, Any]:
        now = datetime.now(UTC)
        record, replay = await repository.acquire(
            c, actor, "DELETE", repository.ROUTE, key, request_hash, now
        )
        if replay is not None:
            if not isinstance(replay.get("canDelete"), bool):
                raise users_repository.invariant()
            return replay
        assert record is not None
        impact = await repository.apply_delete(c, user, actor, request_id, now)
        await repository.complete(c, record, impact, now)
        return impact

    return await mutation(database, operation)
