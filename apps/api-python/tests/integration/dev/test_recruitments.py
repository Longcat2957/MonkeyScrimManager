"""Development-only dummy participants preserve normal recruitment side effects."""

import uuid

import pytest
from sqlalchemy import text

from tests.support.assertions import assert_failure
from tests.support.guilds.context import GUILD_A, USER_ID
from tests.support.recruitments.members import setup_members

pytestmark = pytest.mark.postgres


async def test_dummy_recruitment_preview_apply_replay_and_remove(owner, dummy_enabled_client):
    path, headers = await setup_members(owner, dummy_enabled_client)
    recruitment = path.split("/")[-2]
    dev_path = f"/api/dev/guilds/{GUILD_A}/recruitments/{recruitment}/participants"
    dummy = f"dmy_{uuid.uuid7()}"
    async with owner.begin() as c:
        await c.execute(
            text("""
            INSERT INTO users (user_id, identity_kind, display_name, created_at, updated_at)
            VALUES (:dummy, 'DUMMY', 'Dummy MID', now(), now())
        """),
            {"dummy": dummy},
        )
        await c.execute(
            text("""
            INSERT INTO dummy_user_profiles (
                user_id, stable_key, state, revision, created_by, updated_by,
                created_at, updated_at
            ) VALUES (:dummy, :stable, 'ACTIVE', 0, :actor, :actor, now(), now())
        """),
            {"dummy": dummy, "stable": f"recruitment_{uuid.uuid7().hex}", "actor": USER_ID},
        )
        await c.execute(
            text("""
            INSERT INTO guild_members (
                guild_id, user_id, display_name, status, main_lane, first_seen_at
            ) VALUES (:guild, :dummy, 'Dummy MID', 'ACTIVE', 'MID', now())
        """),
            {"guild": GUILD_A, "dummy": dummy},
        )

    overview = await dummy_enabled_client.get(dev_path)
    assert overview.status_code == 200, overview.text
    candidate = overview.json()["data"]["candidates"][0]
    assert candidate["userId"] == dummy
    assert candidate["participationState"] == "AVAILABLE"
    preview = await dummy_enabled_client.post(
        dev_path + "/preview", json={"action": "ADD", "userIds": [dummy]}
    )
    assert preview.status_code == 200, preview.text
    data = preview.json()["data"]
    assert data["canApply"] is True
    assert data["currentRevision"] == 0
    assert data["projectedJoinedCount"] == 1
    add_headers = {**headers, "idempotency-key": "dummy-add-1"}
    assert_failure(
        await dummy_enabled_client.post(
            dev_path,
            headers={key: value for key, value in headers.items() if key != "x-csrf-token"},
            json={"userIds": [dummy], "expectedRevision": 0},
        ),
        403,
        "csrf_validation_failed",
        "요청 출처를 확인할 수 없습니다.",
    )
    assert_failure(
        await dummy_enabled_client.post(
            dev_path,
            headers=add_headers,
            json={"userIds": [dummy, dummy], "expectedRevision": 0},
        ),
        422,
        "invalid_dummy_participant_selection",
        "더미 참가자는 중복 없이 1명 이상 10명 이하로 선택해야 합니다.",
    )
    added = await dummy_enabled_client.post(
        dev_path,
        headers=add_headers,
        json={"userIds": [dummy], "expectedRevision": data["currentRevision"]},
    )
    assert added.status_code == 200, added.text
    added_data = added.json()["data"]
    assert added_data["action"] == "ADD"
    assert added_data["changedUserIds"] == [dummy]
    assert added_data["overview"]["recruitment"]["revision"] == 1
    assert added_data["overview"]["candidates"][0]["participationState"] == "JOINED"
    replay = await dummy_enabled_client.post(
        dev_path,
        headers=add_headers,
        json={"userIds": [dummy], "expectedRevision": data["currentRevision"]},
    )
    assert replay.status_code == 200
    assert replay.json()["data"] == added_data
    assert_failure(
        await dummy_enabled_client.post(
            dev_path,
            headers={**headers, "idempotency-key": "dummy-stale-1"},
            json={"userIds": [dummy], "expectedRevision": 0},
        ),
        409,
        "revision_conflict",
        "모집 참가자 정보가 변경되었습니다. 최신 상태를 다시 확인해 주세요.",
    )
    assert_failure(
        await dummy_enabled_client.post(
            dev_path,
            headers={**headers, "idempotency-key": "dummy-add-2"},
            json={"userIds": [dummy], "expectedRevision": 1},
        ),
        409,
        "already_participant",
        "이미 모집에 참가 중인 더미 사용자입니다.",
    )
    removed = await dummy_enabled_client.request(
        "DELETE",
        dev_path,
        headers={**headers, "idempotency-key": "dummy-remove-1"},
        json={"userIds": [dummy], "expectedRevision": 1},
    )
    assert removed.status_code == 200, removed.text
    assert removed.json()["data"]["overview"]["candidates"][0]["participationState"] == "WITHDRAWN"
    async with owner.connect() as c:
        participant = (
            await c.execute(
                text("""
            SELECT status FROM recruitment_participants
            WHERE guild_id=:guild AND recruitment_id=:recruitment AND user_id=:dummy
        """),
                {"guild": GUILD_A, "recruitment": recruitment, "dummy": dummy},
            )
        ).scalar_one()
        draft_revision = await c.scalar(
            text("""
            SELECT revision FROM recruitment_entry_drafts
            WHERE guild_id=:guild AND recruitment_id=:recruitment
        """),
            {"guild": GUILD_A, "recruitment": recruitment},
        )
        audit_count = await c.scalar(
            text("""
            SELECT count(*) FROM admin_audit_log
            WHERE guild_id=:guild AND action LIKE 'DUMMY_RECRUITMENT_%'
        """),
            {"guild": GUILD_A},
        )
    assert participant == "WITHDRAWN"
    assert draft_revision == 2
    assert audit_count == 2
