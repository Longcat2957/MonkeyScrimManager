"""Dummy-user lifecycle keeps revisions, history, and deletion boundaries atomic."""

import httpx
import pytest
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncEngine

from tests.support.assertions import assert_failure
from tests.support.auth.session import TOKEN, seed_session
from tests.support.guilds.context import GUILD_A, ROLE_A, guild_fixture
from tests.support.guilds.operations import auction_fixture
from tests.support.recruitments.queries import recruitment_fixture

pytestmark = pytest.mark.postgres

BASE = "/api/dev/dummy-users"
SESSION = {"cookie": f"monkeybot_session={TOKEN}"}
CSRF = {
    "cookie": f"monkeybot_session={TOKEN}; monkeybot_csrf=integration-csrf",
    "origin": "http://localhost:5173",
    "x-csrf-token": "integration-csrf",
}


def headers(key: str) -> dict[str, str]:
    return {**CSRF, "idempotency-key": key}


async def create_user(client: httpx.AsyncClient, stable: str, *, guild: bool = False) -> dict:
    body = {
        "stableKey": stable,
        "displayName": "처음 이름",
        "avatarSeed": "first-seed",
        "notes": "처음 메모",
        "guildMemberships": [{"guildId": GUILD_A, "displayName": "길드 이름", "status": "ACTIVE"}]
        if guild
        else [],
        "riotAccount": {
            "platform": "KR",
            "routingRegion": "ASIA",
            "gameName": "Synthetic",
            "tagLine": "TEST",
        },
    }
    response = await client.post(BASE, json=body, headers=headers(f"create-{stable}"))
    assert response.status_code == 201, response.text
    return response.json()["data"]


async def test_patch_validation_revision_replay_and_audit(
    owner: AsyncEngine, dummy_enabled_client: httpx.AsyncClient
) -> None:
    await seed_session(owner)
    created = await create_user(dummy_enabled_client, "lifecycle-patch")
    path = f"{BASE}/{created['userId']}"
    assert_failure(
        await dummy_enabled_client.patch(
            path, json={"expectedRevision": -1}, headers=headers("negative")
        ),
        422,
        "invalid_expected_revision",
        "expectedRevision은 0 이상이어야 합니다.",
    )
    assert_failure(
        await dummy_enabled_client.patch(
            path, json={"expectedRevision": 0}, headers=headers("empty")
        ),
        422,
        "invalid_dummy_user_input",
        "더미 사용자 입력값을 확인해 주세요.",
    )
    assert_failure(
        await dummy_enabled_client.patch(
            path,
            json={"expectedRevision": 0, "avatarSeed": None, "notes": None},
            headers=headers("null-only"),
        ),
        422,
        "invalid_dummy_user_input",
        "더미 사용자 입력값을 확인해 주세요.",
    )
    assert_failure(
        await dummy_enabled_client.patch(
            path,
            json={"expectedRevision": 0, "displayName": " 공백"},
            headers=headers("invalid-name"),
        ),
        422,
        "invalid_dummy_user_input",
        "더미 사용자 입력값을 확인해 주세요.",
    )
    command = {
        "expectedRevision": 0,
        "displayName": "바뀐 이름",
        "avatarSeed": None,
        "notes": None,
        "privacy": {"showRiotIds": True},
    }
    response = await dummy_enabled_client.patch(path, json=command, headers=headers("patch-1"))
    assert response.status_code == 200, response.text
    updated = response.json()["data"]
    assert updated["revision"] == 1
    assert updated["displayName"] == "바뀐 이름"
    assert updated["avatarSeed"] == "first-seed"
    assert updated["notes"] == "처음 메모"
    assert updated["privacy"]["showRiotIds"] is True
    replay = await dummy_enabled_client.patch(path, json=command, headers=headers("patch-1"))
    assert replay.status_code == 200 and replay.json()["data"] == updated
    without_nulls = {field: value for field, value in command.items() if value is not None}
    replay_without_nulls = await dummy_enabled_client.patch(
        path, json=without_nulls, headers=headers("patch-1")
    )
    assert replay_without_nulls.status_code == 200
    assert replay_without_nulls.json()["data"] == updated
    assert_failure(
        await dummy_enabled_client.patch(
            path,
            json={"expectedRevision": 0, "displayName": "바뀐 이름"},
            headers=headers("patch-1"),
        ),
        409,
        "idempotency_conflict",
        "같은 Idempotency-Key가 다른 요청에 사용되었습니다.",
    )
    assert_failure(
        await dummy_enabled_client.patch(
            path, json={**command, "displayName": "다른 이름"}, headers=headers("patch-1")
        ),
        409,
        "idempotency_conflict",
        "같은 Idempotency-Key가 다른 요청에 사용되었습니다.",
    )
    conflict = await dummy_enabled_client.patch(path, json=command, headers=headers("patch-stale"))
    assert_failure(
        conflict,
        409,
        "revision_conflict",
        "더미 사용자 데이터가 변경되었습니다. 최신 상태를 다시 확인해 주세요.",
    )
    assert conflict.json()["error"]["details"] == {"expectedRevision": 0, "currentRevision": 1}
    assert_failure(
        await dummy_enabled_client.patch(path, json=command, headers=CSRF),
        400,
        "invalid_idempotency_key",
        "Idempotency-Key가 올바르지 않습니다.",
    )
    async with owner.connect() as c:
        audit = (
            await c.execute(
                text("""
                SELECT before_summary, after_summary FROM dummy_user_audit_log
                WHERE action='DUMMY_USER_UPDATED'
            """)
            )
        ).one()
        assert audit.before_summary["displayName"] == "처음 이름"
        assert audit.after_summary == {"revision": 1}


async def test_archive_blocks_joined_recruitment_then_inactivates_and_restores(
    owner: AsyncEngine, dummy_enabled_client: httpx.AsyncClient
) -> None:
    await seed_session(owner)
    await guild_fixture(owner, GUILD_A, "Alpha", ROLE_A)
    created = await create_user(dummy_enabled_client, "lifecycle-archive", guild=True)
    user = created["userId"]
    path = f"{BASE}/{user}"
    recruitment = await recruitment_fixture(owner, 55)
    auction, _ = await auction_fixture(owner, 56)
    async with owner.begin() as c:
        await c.execute(
            text("""
            INSERT INTO recruitment_participants
              (guild_id, recruitment_id, user_id, status, joined_at)
            VALUES (:guild, :recruitment, :user, 'JOINED', now())
        """),
            {"guild": GUILD_A, "recruitment": recruitment, "user": user},
        )
        await c.execute(
            text("""
            INSERT INTO auction_recruitment_participants
              (guild_id, recruitment_id, user_id, status, snapshot_origin, joined_at)
            VALUES (:guild, :recruitment, :user, 'JOINED', 'UNAVAILABLE', now())
        """),
            {"guild": GUILD_A, "recruitment": auction, "user": user},
        )
    blocked = await dummy_enabled_client.post(
        path + "/archive", json={"expectedRevision": 0}, headers=headers("archive-blocked")
    )
    assert_failure(
        blocked,
        409,
        "dummy_user_archive_blocked_by_recruitment",
        "진행 중인 일반 또는 경매 모집에 참가 중이라 보관할 수 없습니다. "
        "먼저 참가를 취소해 주세요.",
    )
    assert blocked.json()["error"]["details"] == {
        "joinedRecruitments": 1,
        "joinedAuctionRecruitments": 1,
    }
    async with owner.begin() as c:
        await c.execute(
            text("""
            UPDATE recruitment_participants SET status='WITHDRAWN', withdrawn_at=now()
            WHERE guild_id=:guild AND recruitment_id=:recruitment AND user_id=:user
        """),
            {"guild": GUILD_A, "recruitment": recruitment, "user": user},
        )
        await c.execute(
            text("""
            UPDATE auction_recruitment_participants
            SET status='WITHDRAWN', withdrawn_at=now()
            WHERE guild_id=:guild AND recruitment_id=:recruitment AND user_id=:user
        """),
            {"guild": GUILD_A, "recruitment": auction, "user": user},
        )
    archive = await dummy_enabled_client.post(
        path + "/archive", json={"expectedRevision": 0}, headers=headers("archive-1")
    )
    assert archive.status_code == 200, archive.text
    archived = archive.json()["data"]
    assert archived["state"] == "ARCHIVED" and archived["revision"] == 1
    assert archived["guildMemberships"][0]["status"] == "INACTIVE"
    replay = await dummy_enabled_client.post(
        path + "/archive", json={"expectedRevision": 0}, headers=headers("archive-1")
    )
    assert replay.status_code == 200 and replay.json()["data"] == archived
    noop = await dummy_enabled_client.post(
        path + "/archive", json={"expectedRevision": 1}, headers=headers("archive-noop")
    )
    assert noop.status_code == 200 and noop.json()["data"]["revision"] == 1
    restore = await dummy_enabled_client.post(
        path + "/restore", json={"expectedRevision": 1}, headers=headers("restore-1")
    )
    assert restore.status_code == 200, restore.text
    restored = restore.json()["data"]
    assert restored["state"] == "ACTIVE" and restored["revision"] == 2
    assert restored["guildMemberships"][0]["status"] == "INACTIVE"
    impact = await dummy_enabled_client.get(path + "/deletion-impact", headers=SESSION)
    assert impact.status_code == 200, impact.text
    assert impact.json()["data"]["blockingReferences"]["guildMemberships"] == 1
    assert impact.json()["data"]["canDelete"] is False
    blocked_delete = await dummy_enabled_client.delete(path, headers=headers("delete-blocked"))
    assert_failure(
        blocked_delete,
        409,
        "dummy_user_deletion_blocked",
        "연결된 내전 데이터가 있어 삭제할 수 없습니다. 더미 사용자를 보관해 주세요.",
    )
    assert blocked_delete.json()["error"]["details"]["impact"] == impact.json()["data"]
    async with owner.connect() as c:
        assert (
            await c.scalar(
                text(
                    "SELECT revision FROM guild_admin_state "
                    "WHERE guild_id=:guild AND scope='MEMBERS'"
                ),
                {"guild": GUILD_A},
            )
            == 2
        )
        guild_audit = (
            await c.execute(
                text("""
                SELECT before_summary, after_summary FROM admin_audit_log
                WHERE guild_id=:guild AND action='MEMBER_STATUS_CHANGED'
            """),
                {"guild": GUILD_A},
            )
        ).one()
        assert guild_audit.before_summary == {"status": "ACTIVE", "source": "DUMMY_USER_ARCHIVE"}
        assert guild_audit.after_summary == {"status": "INACTIVE", "source": "DUMMY_USER_ARCHIVE"}
        lifecycle_audits = (
            await c.execute(
                text("""
                SELECT action, after_summary FROM dummy_user_audit_log
                WHERE action IN ('DUMMY_USER_ARCHIVED', 'DUMMY_USER_RESTORED')
                ORDER BY created_at
            """)
            )
        ).all()
        assert [row.action for row in lifecycle_audits] == [
            "DUMMY_USER_ARCHIVED",
            "DUMMY_USER_RESTORED",
        ]
        assert lifecycle_audits[0].after_summary["inactivatedGuildIds"] == [GUILD_A]
        assert lifecycle_audits[1].after_summary["inactivatedGuildIds"] == []


async def test_delete_cleans_removable_references_and_replays(
    owner: AsyncEngine, dummy_enabled_client: httpx.AsyncClient
) -> None:
    await seed_session(owner)
    created = await create_user(dummy_enabled_client, "lifecycle-delete")
    user = created["userId"]
    path = f"{BASE}/{user}"
    response = await dummy_enabled_client.get(path + "/deletion-impact", headers=SESSION)
    assert response.status_code == 200, response.text
    impact = response.json()["data"]
    assert impact["canDelete"] is True
    assert impact["removableReferences"]["syntheticRiotAccounts"] == 1
    deleted = await dummy_enabled_client.delete(path, headers=headers("delete-1"))
    assert deleted.status_code == 200, deleted.text
    assert deleted.json()["data"] == {"deleted": True, "impact": impact}
    replay = await dummy_enabled_client.delete(path, headers=headers("delete-1"))
    assert replay.status_code == 200 and replay.json()["data"] == deleted.json()["data"]
    assert_failure(
        await dummy_enabled_client.get(path + "/deletion-impact", headers=SESSION),
        404,
        "dummy_user_not_found",
        "더미 사용자를 찾을 수 없습니다.",
    )
    async with owner.connect() as c:
        assert (
            await c.scalar(text("SELECT count(*) FROM users WHERE user_id=:user"), {"user": user})
            == 0
        )
        assert (
            await c.scalar(
                text("SELECT count(*) FROM riot_accounts WHERE user_id=:user"), {"user": user}
            )
            == 0
        )
        assert (
            await c.scalar(
                text("SELECT count(*) FROM dummy_user_audit_log WHERE action='DUMMY_USER_DELETED'")
            )
            == 1
        )
