"""Reusable dummy-user group workflows."""

import hashlib
import logging
import uuid
from datetime import UTC, datetime, timedelta
from typing import Any

from pydantic import ValidationError
from pydantic_core import to_json
from sqlalchemy.exc import IntegrityError
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError
from monkeybot_api.db.database import Database
from monkeybot_api.repositories.dev import groups as repository
from monkeybot_api.schemas.dev.groups import (
    CreateGroup,
    DeletedGroup,
    DummyGroup,
    ReplaceGroupMembers,
    StoredGroup,
    UpdateGroup,
)

logger = logging.getLogger(__name__)
CREATE_ROUTE = "/api/dev/dummy-groups"
UPDATE_ROUTE = "/api/dev/dummy-groups/{groupId}"
MEMBERS_ROUTE = "/api/dev/dummy-groups/{groupId}/members"


def invalid(field: str) -> ApiError:
    return ApiError(
        422, "invalid_dummy_user_input", "더미 사용자 입력값을 확인해 주세요.", {"field": field}
    )


def validate_group(name: str, description: str | None) -> None:
    if name.strip() != name or not 1 <= len(name) <= 100:
        raise invalid("name")
    if description is not None and (description.strip() != description or len(description) > 500):
        raise invalid("description")


def validate_member_ids(member_ids: list[str]) -> None:
    if len(member_ids) > 100 or len(set(member_ids)) != len(member_ids):
        raise invalid("memberIds")


def validate_revision(expected: int, actual: int | None = None) -> None:
    if expected < 0:
        raise ApiError(422, "invalid_expected_revision", "expectedRevision은 0 이상이어야 합니다.")
    if actual is not None and expected != actual:
        raise ApiError(
            409,
            "revision_conflict",
            "더미 사용자 데이터가 변경되었습니다. 최신 상태를 다시 확인해 주세요.",
            {"expectedRevision": expected, "currentRevision": actual},
        )


def normalized_description(value: str | None) -> str | None:
    return value if value else None


def request_hash(value: Any) -> bytes:
    return hashlib.sha256(to_json(value)).digest()


def params(
    actor: str, request_id: str, key: str, method: str, route: str, payload: Any
) -> dict[str, Any]:
    now = datetime.now(UTC)
    return {
        "actor": actor,
        "request_id": request_id,
        "key": key,
        "method": method,
        "route": route,
        "hash": request_hash(payload),
        "idem_id": uuid.uuid7(),
        "audit_id": uuid.uuid7(),
        "now": now,
        "expires": now + timedelta(hours=24),
    }


def snapshot(value: dict[str, Any], request_id: str) -> dict[str, Any]:
    try:
        return DummyGroup.model_validate(value).model_dump(mode="json")
    except ValidationError:
        logger.error("dummy_group_projection_invalid", extra={"request_id": request_id})
        raise ApiError(
            500,
            "dummy_user_invariant_failed",
            "저장된 더미 사용자 데이터의 일관성을 확인할 수 없습니다.",
        ) from None


def replay_group(value: dict[str, Any], request_id: str) -> dict[str, Any]:
    try:
        return StoredGroup.model_validate(value).group.model_dump(mode="json")
    except ValidationError:
        logger.error("dummy_group_replay_invalid", extra={"request_id": request_id})
        raise ApiError(
            500,
            "dummy_user_invariant_failed",
            "저장된 더미 사용자 데이터의 일관성을 확인할 수 없습니다.",
        ) from None


def duplicate_name(error: IntegrityError) -> Exception:
    original = error.orig
    constraint = getattr(original, "constraint_name", None) or getattr(
        getattr(original, "__cause__", None), "constraint_name", None
    )
    if constraint == "dummy_user_groups_name_ci_key":
        return ApiError(
            409, "dummy_group_name_conflict", "같은 이름의 더미 사용자 그룹이 이미 있습니다."
        )
    return error


async def ensure_active_members(c: AsyncConnection, member_ids: list[str]) -> None:
    if await repository.active_member_count(c, member_ids) != len(member_ids):
        raise ApiError(
            409,
            "dummy_group_member_unavailable",
            "그룹에는 활성 상태인 더미 사용자만 추가할 수 있습니다.",
        )


async def list_groups(database: Database, request_id: str) -> dict[str, Any]:
    async with database.connection() as c:
        groups = [
            snapshot(await repository.fetch(c, identifier), request_id)
            for identifier in await repository.list_ids(c)
        ]
    return {"groups": groups}


async def create_group(
    database: Database, actor: str, request_id: str, key: str, body: CreateGroup
) -> dict[str, Any]:
    validate_group(body.name, body.description)
    validate_member_ids(body.memberIds)
    p = params(actor, request_id, key, "POST", CREATE_ROUTE, body.model_dump(mode="json"))
    async with database.connection(transaction=True) as c:
        replay = await repository.acquire(c, p)
        if replay is not None:
            return replay_group(replay, request_id)
        await ensure_active_members(c, body.memberIds)
        group_id = uuid.uuid7()
        p["id"] = group_id
        p["target"] = str(group_id)
        p["name"] = body.name
        p["description"] = normalized_description(body.description)
        try:
            await repository.insert(c, p)
        except IntegrityError as error:
            raise duplicate_name(error) from None
        await repository.replace_members(c, group_id, body.memberIds, p["now"])
        await repository.audit(
            c,
            p,
            "DUMMY_GROUP_CREATED",
            None,
            {"name": body.name, "memberIds": body.memberIds},
        )
        result = snapshot(await repository.fetch(c, group_id), request_id)
        await repository.complete(c, p, {"group": result}, 201)
        return result


async def update_group(
    database: Database,
    group_id: uuid.UUID,
    actor: str,
    request_id: str,
    key: str,
    body: UpdateGroup,
) -> dict[str, Any]:
    validate_revision(body.expectedRevision)
    p = params(
        actor,
        request_id,
        key,
        "PATCH",
        UPDATE_ROUTE,
        [str(group_id), body.model_dump(mode="json")],
    )
    p["id"] = group_id
    p["target"] = str(group_id)
    async with database.connection(transaction=True) as c:
        replay = await repository.acquire(c, p)
        if replay is not None:
            return replay_group(replay, request_id)
        current = await repository.lock(c, group_id)
        validate_revision(body.expectedRevision, current["revision"])
        name = body.name if body.name is not None else current["name"]
        description = body.description if body.description is not None else current["description"]
        validate_group(name, description)
        p["name"] = name
        p["description"] = normalized_description(description)
        try:
            await repository.update(c, p)
        except IntegrityError as error:
            raise duplicate_name(error) from None
        await repository.audit(
            c,
            p,
            "DUMMY_GROUP_UPDATED",
            {
                "name": current["name"],
                "description": current["description"],
                "revision": current["revision"],
            },
            {"name": name, "description": description, "revision": current["revision"] + 1},
        )
        result = snapshot(await repository.fetch(c, group_id), request_id)
        await repository.complete(c, p, {"group": result}, 200)
        return result


async def replace_members(
    database: Database,
    group_id: uuid.UUID,
    actor: str,
    request_id: str,
    key: str,
    body: ReplaceGroupMembers,
) -> dict[str, Any]:
    validate_revision(body.expectedRevision)
    validate_member_ids(body.memberIds)
    p = params(
        actor,
        request_id,
        key,
        "PUT",
        MEMBERS_ROUTE,
        [str(group_id), body.model_dump(mode="json")],
    )
    p["id"] = group_id
    p["target"] = str(group_id)
    async with database.connection(transaction=True) as c:
        replay = await repository.acquire(c, p)
        if replay is not None:
            return replay_group(replay, request_id)
        current = await repository.lock(c, group_id)
        validate_revision(body.expectedRevision, current["revision"])
        await ensure_active_members(c, body.memberIds)
        await repository.replace_members(c, group_id, body.memberIds, p["now"])
        await repository.bump_revision(c, p)
        await repository.audit(
            c,
            p,
            "DUMMY_GROUP_MEMBERS_REPLACED",
            None,
            {"memberIds": body.memberIds, "revision": current["revision"] + 1},
        )
        result = snapshot(await repository.fetch(c, group_id), request_id)
        await repository.complete(c, p, {"group": result}, 200)
        return result


async def delete_group(
    database: Database, group_id: uuid.UUID, actor: str, request_id: str, key: str
) -> dict[str, bool]:
    p = params(actor, request_id, key, "DELETE", UPDATE_ROUTE, str(group_id))
    p["id"] = group_id
    p["target"] = str(group_id)
    async with database.connection(transaction=True) as c:
        replay = await repository.acquire(c, p)
        if replay is not None:
            try:
                return DeletedGroup.model_validate(replay).model_dump()
            except ValidationError:
                logger.error("dummy_group_delete_replay_invalid", extra={"request_id": request_id})
                raise ApiError(
                    500,
                    "dummy_user_invariant_failed",
                    "저장된 더미 사용자 데이터의 일관성을 확인할 수 없습니다.",
                ) from None
        current = await repository.lock(c, group_id)
        await repository.delete(c, group_id)
        await repository.audit(
            c,
            p,
            "DUMMY_GROUP_DELETED",
            {"name": current["name"], "revision": current["revision"]},
            None,
        )
        result = {"deleted": True}
        await repository.complete(c, p, result, 200)
        return result
