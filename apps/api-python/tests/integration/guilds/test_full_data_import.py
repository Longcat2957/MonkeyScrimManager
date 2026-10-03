"""Complete import must validate transactionally and reject stale previews."""

import pytest
from sqlalchemy import text

from tests.support.assertions import assert_failure
from tests.support.guilds.context import GUILD_A
from tests.support.guilds.full_data_transfer import export, setup

pytestmark = pytest.mark.postgres

PREVIEW = f"/api/guilds/{GUILD_A}/full-data-imports/preview"


async def test_full_import_preview_apply_replay_and_audit(owner, runtime_client):
    headers = await setup(owner, runtime_client)
    exported = await export(runtime_client, headers)
    assert exported.status_code == 200, exported.text
    archive = exported.json()["data"]["archive"]
    preview_response = await runtime_client.post(
        PREVIEW, headers=headers, json={"archive": archive}
    )
    assert preview_response.status_code == 200, preview_response.text
    preview = preview_response.json()["data"]
    assert preview["conflicts"] == []
    assert preview["usesTargetActiveSeason"] is True
    assert preview["remapsSourceGuild"] is False
    assert preview["summary"]["members"] == 1
    assert len(preview["digest"]) == len(preview["targetDigest"]) == 64
    async with owner.connect() as c:
        stored = (
            (
                await c.execute(
                    text("""
            SELECT status, conflict_count, archive_payload
            FROM guild_full_data_imports WHERE guild_id=:guild AND id=:import_id
        """),
                    {"guild": GUILD_A, "import_id": preview["importId"]},
                )
            )
            .mappings()
            .one()
        )
    assert stored["status"] == "PREVIEWED"
    assert stored["conflict_count"] == 0
    assert stored["archive_payload"]["source"] == archive["source"]

    path = f"/api/guilds/{GUILD_A}/full-data-imports/{preview['importId']}/apply"
    command = {
        "expectedDigest": preview["digest"],
        "expectedTargetDigest": preview["targetDigest"],
    }
    apply_headers = {**headers, "idempotency-key": "full-import-apply-1"}
    applied_response = await runtime_client.post(path, headers=apply_headers, json=command)
    assert applied_response.status_code == 200, applied_response.text
    applied = applied_response.json()["data"]
    assert applied["status"] == "APPLIED"
    assert applied["usedTargetActiveSeason"] is True
    assert applied["summary"] == preview["summary"]
    replay = await runtime_client.post(path, headers=apply_headers, json=command)
    assert replay.status_code == 200, replay.text
    assert replay.json()["data"] == applied
    async with owner.connect() as c:
        stored = (
            (
                await c.execute(
                    text("""
            SELECT status, archive_payload, applied_at
            FROM guild_full_data_imports WHERE guild_id=:guild AND id=:import_id
        """),
                    {"guild": GUILD_A, "import_id": preview["importId"]},
                )
            )
            .mappings()
            .one()
        )
        actions = (
            (
                await c.execute(
                    text("""
            SELECT action FROM admin_audit_log WHERE guild_id=:guild ORDER BY created_at
        """),
                    {"guild": GUILD_A},
                )
            )
            .scalars()
            .all()
        )
    assert stored["status"] == "APPLIED"
    assert stored["archive_payload"] is None
    assert stored["applied_at"] is not None
    assert actions == [
        "FULL_DATA_TRANSFER_EXPORTED",
        "FULL_DATA_IMPORT_PREVIEWED",
        "FULL_DATA_IMPORT_APPLIED",
    ]


async def test_full_import_rejects_invalid_archive_and_stale_target(owner, runtime_client):
    headers = await setup(owner, runtime_client)
    exported = await export(runtime_client, headers)
    archive = exported.json()["data"]["archive"]
    archive["tables"]["games"] = [{"guild_id": "200000000000000001"}]
    assert_failure(
        await runtime_client.post(PREVIEW, headers=headers, json={"archive": archive}),
        400,
        "invalid_full_archive",
        "지원되는 MonkeyBot 전체 데이터 백업 파일 형식이 아닙니다.",
    )
    archive["tables"]["games"] = []
    preview_response = await runtime_client.post(
        PREVIEW, headers=headers, json={"archive": archive}
    )
    assert preview_response.status_code == 200, preview_response.text
    preview = preview_response.json()["data"]
    async with owner.begin() as c:
        await c.execute(
            text("""
            INSERT INTO guild_counters (guild_id, counter_key, next_value)
            VALUES (:guild, 'SERIES', 2)
            ON CONFLICT (guild_id, counter_key) DO UPDATE SET next_value=2
        """),
            {"guild": GUILD_A},
        )
    path = f"/api/guilds/{GUILD_A}/full-data-imports/{preview['importId']}/apply"
    assert_failure(
        await runtime_client.post(
            path,
            headers={**headers, "idempotency-key": "full-import-stale-1"},
            json={
                "expectedDigest": preview["digest"],
                "expectedTargetDigest": preview["targetDigest"],
            },
        ),
        409,
        "full_import_target_changed",
        "검토 이후 대상 길드의 멤버 또는 데이터 상태가 변경되었습니다. 파일을 다시 검증해 주세요.",
    )
