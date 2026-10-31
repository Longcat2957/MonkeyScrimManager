"""Legacy MMR seed export/import compatibility with the Rust route."""

import copy

import pytest
from sqlalchemy import text

from tests.support.assertions import assert_failure
from tests.support.guilds.context import GUILD_A, USER_ID
from tests.support.guilds.full_data_transfer import setup

pytestmark = pytest.mark.postgres

EXPORT = f"/api/guilds/{GUILD_A}/data-export"
PREVIEW = f"/api/guilds/{GUILD_A}/data-imports/preview"


def apply_path(preview: dict) -> str:
    return f"/api/guilds/{GUILD_A}/data-imports/{preview['importId']}/apply"


async def test_legacy_mmr_transfer_applies_five_lanes_once(owner, runtime_client):
    headers = await setup(owner, runtime_client)
    exported_response = await runtime_client.post(EXPORT, headers=headers)
    assert exported_response.status_code == 200, exported_response.text
    exported = exported_response.json()["data"]
    assert exported["fileName"].startswith(f"monkeybot-mmr-{GUILD_A}-")
    archive = exported["archive"]
    assert archive["format"] == "MONKEYBOT_DATA_TRANSFER"
    assert archive["kind"] == "MEMBER_SEASON_MMR_SEED"
    assert len(archive["members"]) == 1
    assert archive["members"][0]["discordId"] == USER_ID
    assert [entry["mmr"] for entry in archive["members"][0]["lanes"]] == [1500.0] * 5

    changed = copy.deepcopy(archive)
    for entry in changed["members"][0]["lanes"]:
        entry["mmr"] = 1650.0
    preview_response = await runtime_client.post(
        PREVIEW, headers=headers, json={"archive": changed}
    )
    assert preview_response.status_code == 200, preview_response.text
    preview = preview_response.json()["data"]
    assert preview["summary"] == {
        "archiveMembers": 1,
        "matchedMembers": 1,
        "laneEntries": 5,
        "changes": 5,
    }
    assert preview["conflicts"] == []
    assert preview["targetRevision"] == 0
    command = {
        "expectedDigest": preview["digest"],
        "expectedTargetRevision": preview["targetRevision"],
    }
    apply_headers = {**headers, "Idempotency-Key": "legacy-seed-one"}
    applied_response = await runtime_client.post(
        apply_path(preview), headers=apply_headers, json=command
    )
    assert applied_response.status_code == 200, applied_response.text
    applied = applied_response.json()["data"]
    assert applied["status"] == "APPLIED"
    assert applied["summary"]["changes"] == 5
    assert applied["targetRevision"] == 1
    replay = await runtime_client.post(apply_path(preview), headers=apply_headers, json=command)
    assert replay.status_code == 200, replay.text
    assert replay.json()["data"] == applied

    async with owner.connect() as c:
        mmr = (
            await c.execute(
                text("""
                SELECT mmr, revision FROM user_lane_mmr
                WHERE guild_id=:guild AND user_id=:user ORDER BY lane
            """),
                {"guild": GUILD_A, "user": USER_ID},
            )
        ).all()
        ledger_count = await c.scalar(
            text("""
            SELECT count(*) FROM mmr_ledger_entries
            WHERE guild_id=:guild AND source_type='DATA_IMPORT_SEED'
        """),
            {"guild": GUILD_A},
        )
        stored = (
            await c.execute(
                text("""
                SELECT status, archive_payload IS NULL AS scrubbed
                FROM guild_data_imports WHERE guild_id=:guild AND id=:id
            """),
                {"guild": GUILD_A, "id": preview["importId"]},
            )
        ).one()
    assert mmr == [(1650.0, 1)] * 5
    assert ledger_count == 5
    assert stored == ("APPLIED", True)


async def test_legacy_mmr_transfer_detects_stale_mmr_and_preserves_preview(owner, runtime_client):
    headers = await setup(owner, runtime_client)
    exported = (await runtime_client.post(EXPORT, headers=headers)).json()["data"]["archive"]
    for entry in exported["members"][0]["lanes"]:
        entry["mmr"] = 1650.0
    preview_response = await runtime_client.post(
        PREVIEW, headers=headers, json={"archive": exported}
    )
    assert preview_response.status_code == 200, preview_response.text
    preview = preview_response.json()["data"]
    async with owner.begin() as c:
        await c.execute(
            text("""
            INSERT INTO user_lane_mmr
              (guild_id, season_id, user_id, lane, mmr, games_played, wins, revision, updated_at)
            VALUES (:guild, :season, :user, 'MID', 1499, 0, 0, 1, now())
        """),
            {"guild": GUILD_A, "season": exported["source"]["season"]["id"], "user": USER_ID},
        )
    response = await runtime_client.post(
        apply_path(preview),
        headers={**headers, "Idempotency-Key": "legacy-stale-mmr"},
        json={"expectedDigest": preview["digest"], "expectedTargetRevision": 0},
    )
    assert_failure(
        response,
        409,
        "mmr_state_changed",
        "대상 길드의 MMR이 변경되었습니다. 파일을 다시 검증해 주세요.",
    )
    async with owner.connect() as c:
        status = await c.scalar(
            text("SELECT status FROM guild_data_imports WHERE guild_id=:guild AND id=:id"),
            {"guild": GUILD_A, "id": preview["importId"]},
        )
        ledger_count = await c.scalar(
            text("""
            SELECT count(*) FROM mmr_ledger_entries
            WHERE guild_id=:guild AND source_type='DATA_IMPORT_SEED'
        """),
            {"guild": GUILD_A},
        )
    assert status == "PREVIEWED"
    assert ledger_count == 0


async def test_legacy_mmr_transfer_rejects_invalid_archive_and_apply_command(owner, runtime_client):
    headers = await setup(owner, runtime_client)
    response = await runtime_client.post(PREVIEW, headers=headers, json={"archive": {}})
    assert_failure(
        response, 400, "invalid_archive", "지원되는 MonkeyBot MMR 백업 파일 형식이 아닙니다."
    )
    exported = (await runtime_client.post(EXPORT, headers=headers)).json()["data"]["archive"]
    preview = (
        await runtime_client.post(PREVIEW, headers=headers, json={"archive": exported})
    ).json()["data"]
    response = await runtime_client.post(
        apply_path(preview),
        headers={**headers, "Idempotency-Key": "legacy-invalid-command"},
        json={"expectedDigest": "not-a-digest", "expectedTargetRevision": 0},
    )
    assert_failure(
        response, 422, "invalid_import_apply_command", "가져오기 적용 확인 값이 올바르지 않습니다."
    )
