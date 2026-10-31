import asyncio
import uuid

import httpx
import pytest
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncEngine

from tests.support.assertions import assert_failure
from tests.support.auth.crypto import rust_auth
from tests.support.guilds.context import GUILD_A, USER_ID
from tests.support.recruitments.creation import BOT
from tests.support.recruitments.members import setup_members

pytestmark = pytest.mark.postgres


async def test_rust_python_member_command_replay_both_directions(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    path, headers = await setup_members(owner, runtime_client)
    payload = {
        "operation": "recruitment_member",
        "guild_id": GUILD_A,
        "user_id": USER_ID,
        "member_id": USER_ID,
        "recruitment_id": path.split("/")[-2],
        "key": "rust-add",
        "joining": True,
    }
    original = await asyncio.to_thread(rust_auth, payload)
    replay = await runtime_client.post(
        path, json={"memberId": USER_ID}, headers={**headers, "idempotency-key": "rust-add"}
    )
    assert replay.status_code == 200 and replay.json()["data"] == original
    removed = await runtime_client.request(
        "DELETE",
        path + "/" + USER_ID,
        json={},
        headers={**headers, "idempotency-key": "python-remove"},
    )
    assert removed.status_code == 200
    rust_replay = await asyncio.to_thread(
        rust_auth, {**payload, "key": "python-remove", "joining": False}
    )
    assert rust_replay == removed.json()["data"]


async def test_add_remove_rejoin_preserves_lanes_and_updates_revisions_and_notifications(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    path, headers = await setup_members(owner, runtime_client)
    joined = await runtime_client.post(path, json={"memberId": USER_ID}, headers=headers)
    assert joined.status_code == 200
    assert joined.json()["data"]["recruitment"]["revision"] == 1
    assert joined.json()["data"]["participants"] == [
        {"id": USER_ID, "displayName": "테스트 멤버", "preferredLanes": []}
    ]
    replay = await runtime_client.post(path, json={"memberId": USER_ID}, headers=headers)
    assert replay.json()["data"] == joined.json()["data"]
    duplicate = await runtime_client.post(
        path, json={"memberId": USER_ID}, headers={**headers, "idempotency-key": "duplicate-member"}
    )
    assert_failure(duplicate, 422, "already_participant", "이미 참가 중인 사용자입니다.")
    record_id = uuid.UUID(path.split("/")[-2])
    async with owner.begin() as connection:
        values = {"guild": GUILD_A, "id": record_id, "user": USER_ID}
        await connection.execute(
            text(
                "INSERT INTO recruitment_preferred_lanes "
                "(guild_id, recruitment_id, user_id, priority, lane) "
                "VALUES (:guild, :id, :user, 1, 'MID')"
            ),
            values,
        )
        await connection.execute(
            text(
                "INSERT INTO recruitment_entry_assignments "
                "(guild_id, recruitment_id, user_id, team, lane) "
                "VALUES (:guild, :id, :user, 'TEAM_1', 'MID')"
            ),
            values,
        )
    removed = await runtime_client.request(
        "DELETE",
        path + "/" + USER_ID,
        json={},
        headers={**headers, "idempotency-key": "remove-member"},
    )
    assert removed.status_code == 200
    assert removed.json()["data"]["recruitment"]["revision"] == 2
    assert removed.json()["data"]["participants"] == []
    async with owner.connect() as connection:
        assert (
            await connection.scalar(text("SELECT count(*) FROM recruitment_entry_assignments")) == 0
        )
        assert (
            await connection.scalar(text("SELECT count(*) FROM recruitment_preferred_lanes")) == 1
        )
        assert await connection.scalar(text("SELECT revision FROM recruitment_entry_drafts")) == 2
        assert await connection.scalar(text("SELECT count(*) FROM bot_notification_outbox")) == 3
    rejoined = await runtime_client.post(
        path, json={"memberId": USER_ID}, headers={**headers, "idempotency-key": "rejoin-member"}
    )
    assert rejoined.status_code == 200
    assert rejoined.json()["data"]["participants"][0]["preferredLanes"] == ["MID"]


@pytest.mark.parametrize(
    "mode,status,code,message",
    [
        ("operator", 403, "operator_required", "길드 운영자 권한이 필요합니다."),
        (
            "inactive",
            403,
            "guild_membership_required",
            "해당 길드의 활성 멤버만 모집을 변경할 수 있습니다.",
        ),
        (
            "closed",
            409,
            "invalid_recruitment_state",
            "현재 모집 상태에서는 이 작업을 수행할 수 없습니다.",
        ),
        ("missing", 404, "guild_member_not_found", "길드에 소속된 사용자를 찾을 수 없습니다."),
        ("csrf", 403, "csrf_validation_failed", "요청 출처를 확인할 수 없습니다."),
    ],
)
async def test_member_add_denial_preserves_recruitment(
    owner: AsyncEngine,
    runtime_client: httpx.AsyncClient,
    mode: str,
    status: int,
    code: str,
    message: str,
) -> None:
    path, headers = await setup_members(owner, runtime_client)
    async with owner.begin() as connection:
        if mode == "operator":
            await connection.execute(text("DELETE FROM guild_member_discord_roles"))
        elif mode == "inactive":
            await connection.execute(text("UPDATE guild_members SET status = 'INACTIVE'"))
        elif mode == "closed":
            await connection.execute(text("UPDATE recruitments SET status = 'CLOSED'"))
    if mode == "csrf":
        headers.pop("x-csrf-token")
    response = await runtime_client.post(
        path, json={"memberId": "987654321" if mode == "missing" else USER_ID}, headers=headers
    )
    assert_failure(response, status, code, message)
    async with owner.connect() as connection:
        assert await connection.scalar(text("SELECT revision FROM recruitments")) == 0
        assert await connection.scalar(text("SELECT count(*) FROM recruitment_participants")) == 0
        assert await connection.scalar(text("SELECT count(*) FROM idempotency_records")) == 1


async def test_bot_self_join_leave_and_cross_user_permission(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    path, _ = await setup_members(owner, runtime_client)
    actor = {
        "id": "123456789",
        "globalDisplayName": "Bot user",
        "displayName": "Bot member",
        "roleIds": [],
    }
    headers = {"authorization": f"Bearer {BOT}", "idempotency-key": "9001"}
    response = await runtime_client.post(
        path, json={"actor": actor, "member": actor}, headers=headers
    )
    assert response.status_code == 200
    denied = await runtime_client.post(
        path,
        json={"actor": actor, "member": {**actor, "id": "123456788"}},
        headers={**headers, "idempotency-key": "9002"},
    )
    assert_failure(denied, 403, "operator_required", "길드 운영자 권한이 필요합니다.")
    removed = await runtime_client.request(
        "DELETE",
        path + "/" + actor["id"],
        json={"actor": actor},
        headers={**headers, "idempotency-key": "9003"},
    )
    assert removed.status_code == 200 and removed.json()["data"]["participants"] == []
    replay = await runtime_client.request(
        "DELETE",
        path + "/" + actor["id"],
        json={"actor": actor},
        headers={**headers, "idempotency-key": "9003"},
    )
    assert replay.json()["data"] == removed.json()["data"]
    async with owner.connect() as connection:
        # Bot renders interaction responses itself: only the browser's original CREATE is queued.
        assert await connection.scalar(text("SELECT count(*) FROM bot_notification_outbox")) == 1


async def test_concurrent_last_slot_has_one_winner(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    path, headers = await setup_members(owner, runtime_client)
    record_id = uuid.UUID(path.split("/")[-2])
    async with owner.begin() as connection:
        await connection.execute(
            text("""
            INSERT INTO users (user_id, identity_kind, display_name, created_at, updated_at)
            SELECT (1000 + n)::text, 'DISCORD', 'Participant', now(), now()
            FROM generate_series(1,11) n
        """)
        )
        await connection.execute(
            text("""
            INSERT INTO guild_members (guild_id, user_id, display_name, status, first_seen_at)
            SELECT :guild, (1000 + n)::text, 'Participant', 'ACTIVE', now()
            FROM generate_series(1,11) n
        """),
            {"guild": GUILD_A},
        )
        await connection.execute(
            text("""
            INSERT INTO recruitment_participants
                (guild_id, recruitment_id, user_id, status, joined_at)
            SELECT :guild, :id, (1000 + n)::text, 'JOINED', now() FROM generate_series(1,9) n
        """),
            {"guild": GUILD_A, "id": record_id},
        )
    responses = await asyncio.gather(
        *[
            runtime_client.post(
                path,
                json={"memberId": member},
                headers={**headers, "idempotency-key": "last-slot-" + member},
            )
            for member in ("1010", "1011")
        ]
    )
    assert sorted(response.status_code for response in responses) == [200, 422]
    denied = next(response for response in responses if response.status_code == 422)
    assert_failure(denied, 422, "recruitment_full", "모집 정원이 가득 찼습니다.")
    async with owner.connect() as connection:
        assert (
            await connection.scalar(
                text("SELECT count(*) FROM recruitment_participants WHERE status = 'JOINED'")
            )
            == 10
        )
        assert await connection.scalar(text("SELECT revision FROM recruitments")) == 1
