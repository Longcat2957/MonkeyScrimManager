"""Dummy groups preserve ordering, revisions, audit, and global idempotency."""

import uuid

import pytest
from sqlalchemy import text

from tests.support.assertions import assert_failure
from tests.support.auth.oauth import cookies, finish_login, start_login
from tests.support.guilds.context import USER_ID
from tests.support.users.profile import csrf

pytestmark = pytest.mark.postgres
PATH = "/api/dev/dummy-groups"


async def setup(owner, client):
    state, _ = await start_login(client)
    response = await finish_login(client, state)
    assert response.status_code == 303
    headers = csrf(cookies(response))
    first, second = f"dmy_{uuid.uuid7()}", f"dmy_{uuid.uuid7()}"
    async with owner.begin() as c:
        await c.execute(
            text("""
            INSERT INTO users (user_id, identity_kind, display_name, created_at, updated_at)
            VALUES (:first, 'DUMMY', 'First dummy', now(), now()),
                   (:second, 'DUMMY', 'Second dummy', now(), now())
        """),
            {"first": first, "second": second},
        )
        await c.execute(
            text("""
            INSERT INTO dummy_user_profiles
              (user_id, stable_key, state, revision, created_by, updated_by, created_at, updated_at)
            VALUES (:first, :first_key, 'ACTIVE', 0, :actor, :actor, now(), now()),
                   (:second, :second_key, 'ACTIVE', 0, :actor, :actor, now(), now())
        """),
            {
                "first": first,
                "second": second,
                "first_key": f"g_{uuid.uuid7().hex}",
                "second_key": f"g_{uuid.uuid7().hex}",
                "actor": USER_ID,
            },
        )
    return headers, first, second


async def test_group_create_update_replace_delete_and_replay(owner, dummy_enabled_client):
    headers, first, second = await setup(owner, dummy_enabled_client)
    created_response = await dummy_enabled_client.post(
        PATH,
        headers={**headers, "Idempotency-Key": "dummy-group-create-1"},
        json={"name": "Top pool", "memberIds": [first, second]},
    )
    assert created_response.status_code == 201, created_response.text
    group = created_response.json()["data"]
    assert group["revision"] == 0
    assert group["memberCount"] == 2
    assert [member["userId"] for member in group["members"]] == [first, second]
    assert [member["position"] for member in group["members"]] == [1, 2]
    replay = await dummy_enabled_client.post(
        PATH,
        headers={**headers, "Idempotency-Key": "dummy-group-create-1"},
        json={"name": "Top pool", "memberIds": [first, second]},
    )
    assert replay.status_code == 201
    assert replay.json()["data"] == group

    listed = await dummy_enabled_client.get(PATH)
    assert listed.status_code == 200, listed.text
    assert listed.json()["data"]["groups"] == [group]
    group_path = f"{PATH}/{group['id']}"
    updated = await dummy_enabled_client.patch(
        group_path,
        headers={**headers, "Idempotency-Key": "dummy-group-update-1"},
        json={"expectedRevision": 0, "name": "Mid pool", "description": "Practice"},
    )
    assert updated.status_code == 200, updated.text
    assert updated.json()["data"]["revision"] == 1
    assert updated.json()["data"]["description"] == "Practice"
    replaced = await dummy_enabled_client.put(
        group_path + "/members",
        headers={**headers, "Idempotency-Key": "dummy-group-members-1"},
        json={"expectedRevision": 1, "memberIds": [second, first]},
    )
    assert replaced.status_code == 200, replaced.text
    assert replaced.json()["data"]["revision"] == 2
    assert [member["userId"] for member in replaced.json()["data"]["members"]] == [second, first]
    deleted = await dummy_enabled_client.delete(
        group_path, headers={**headers, "Idempotency-Key": "dummy-group-delete-1"}
    )
    assert deleted.status_code == 200, deleted.text
    assert deleted.json()["data"] == {"deleted": True}
    replay_deleted = await dummy_enabled_client.delete(
        group_path, headers={**headers, "Idempotency-Key": "dummy-group-delete-1"}
    )
    assert replay_deleted.status_code == 200
    assert replay_deleted.json()["data"] == {"deleted": True}
    assert (await dummy_enabled_client.get(PATH)).json()["data"]["groups"] == []
    async with owner.connect() as c:
        actions = (
            (
                await c.execute(
                    text("SELECT action FROM dummy_user_audit_log ORDER BY created_at, id")
                )
            )
            .scalars()
            .all()
        )
    assert actions == [
        "DUMMY_GROUP_CREATED",
        "DUMMY_GROUP_UPDATED",
        "DUMMY_GROUP_MEMBERS_REPLACED",
        "DUMMY_GROUP_DELETED",
    ]


async def test_group_rejects_stale_revision_and_archived_member(owner, dummy_enabled_client):
    headers, first, second = await setup(owner, dummy_enabled_client)
    response = await dummy_enabled_client.post(
        PATH,
        headers={**headers, "Idempotency-Key": "dummy-group-invalid-1"},
        json={"name": "Duplicate", "memberIds": [first, first]},
    )
    assert_failure(response, 422, "invalid_dummy_user_input", "더미 사용자 입력값을 확인해 주세요.")
    assert response.json()["error"]["details"] == {"field": "memberIds"}
    async with owner.begin() as c:
        await c.execute(
            text("UPDATE dummy_user_profiles SET state='ARCHIVED' WHERE user_id=:user"),
            {"user": second},
        )
    response = await dummy_enabled_client.post(
        PATH,
        headers={**headers, "Idempotency-Key": "dummy-group-invalid-2"},
        json={"name": "Archived", "memberIds": [second]},
    )
    assert_failure(
        response,
        409,
        "dummy_group_member_unavailable",
        "그룹에는 활성 상태인 더미 사용자만 추가할 수 있습니다.",
    )
    created = await dummy_enabled_client.post(
        PATH,
        headers={**headers, "Idempotency-Key": "dummy-group-valid-1"},
        json={"name": "Valid", "memberIds": [first]},
    )
    assert created.status_code == 201, created.text
    group_id = created.json()["data"]["id"]
    response = await dummy_enabled_client.put(
        f"{PATH}/{group_id}/members",
        headers={**headers, "Idempotency-Key": "dummy-group-stale-1"},
        json={"expectedRevision": 2, "memberIds": []},
    )
    assert_failure(
        response,
        409,
        "revision_conflict",
        "더미 사용자 데이터가 변경되었습니다. 최신 상태를 다시 확인해 주세요.",
    )
    assert response.json()["error"]["details"] == {
        "expectedRevision": 2,
        "currentRevision": 0,
    }


async def test_group_patch_null_and_omission_replay_the_same_command(owner, dummy_enabled_client):
    headers, _, _ = await setup(owner, dummy_enabled_client)
    created = await dummy_enabled_client.post(
        PATH,
        headers={**headers, "Idempotency-Key": f"group-create-{uuid.uuid7()}"},
        json={"name": f"Pool {uuid.uuid7().hex[:8]}", "description": "Keep me"},
    )
    assert created.status_code == 201, created.text
    group_path = f"{PATH}/{created.json()['data']['id']}"
    key = f"group-patch-{uuid.uuid7()}"
    unchanged = await dummy_enabled_client.patch(
        group_path,
        headers={**headers, "Idempotency-Key": key},
        json={"expectedRevision": 0},
    )
    assert unchanged.status_code == 200, unchanged.text
    assert unchanged.json()["data"]["description"] == "Keep me"
    replay = await dummy_enabled_client.patch(
        group_path,
        headers={**headers, "Idempotency-Key": key},
        json={"expectedRevision": 0, "description": None},
    )
    assert replay.status_code == 200, replay.text
    assert replay.json()["data"] == unchanged.json()["data"]
