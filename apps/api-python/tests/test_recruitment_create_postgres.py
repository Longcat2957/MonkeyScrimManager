import asyncio
import uuid

import httpx
import pytest
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncEngine
from test_auth_crypto import rust_auth
from test_context_postgres import GUILD_A, USER_ID, authenticated_setup
from test_foundation import assert_failure
from test_profile_postgres import csrf

pytestmark = pytest.mark.postgres
PATH = f"/api/guilds/{GUILD_A}/recruitments"
START = "2026-09-13T20:00:00.123456789+09:00"
BODY = {"bestOf": 3, "hardFearless": True, "expectedStartAt": START}
BOT = "isolated-bot-service-token-32-characters"


async def setup(owner: AsyncEngine, client: httpx.AsyncClient) -> dict[str, str]:
    material = await authenticated_setup(owner, client)
    async with owner.begin() as connection:
        await connection.execute(
            text("UPDATE guild_config SET recruitment_channel_id = '123456789'")
        )
        await connection.execute(
            text(
                "INSERT INTO guild_counters (guild_id, counter_key, next_value) "
                "VALUES (:guild, 'RECRUITMENT', 1)"
            ),
            {"guild": GUILD_A},
        )
    return {**csrf(material), "idempotency-key": "browser-create-1"}


async def counts(owner: AsyncEngine) -> tuple[int, ...]:
    async with owner.connect() as connection:
        return tuple(
            [
                await connection.scalar(text(f"SELECT count(*) FROM {table}"))
                for table in (
                    "recruitments",
                    "recruitment_entry_drafts",
                    "admin_audit_log",
                    "outbox_events",
                    "bot_notification_outbox",
                    "idempotency_records",
                )
            ]
        )


async def test_browser_create_atomic_snapshot_outboxes_and_concurrent_replay(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    headers = await setup(owner, runtime_client)
    responses = await asyncio.gather(
        *[runtime_client.post(PATH, json=BODY, headers=headers) for _ in range(2)]
    )
    assert [response.status_code for response in responses] == [201, 201]
    snapshot = responses[0].json()["data"]
    assert responses[1].json()["data"] == snapshot
    recruitment = snapshot["recruitment"]
    assert recruitment["title"] == "일반 내전 모집 #1 · 예상 시작 2026-09-13 20:00 (KST)"
    assert recruitment["status"] == "OPEN" and recruitment["bestOf"] == 3
    assert recruitment["hardFearless"] is True and recruitment["targetCount"] == 10
    assert snapshot["participants"] == []
    assert (await runtime_client.get(PATH + "/" + recruitment["id"])).json()["data"] == snapshot
    assert await counts(owner) == (1, 1, 1, 1, 1, 1)
    async with owner.connect() as connection:
        notification = (
            await connection.execute(
                text("SELECT payload, channel_id, notification_type FROM bot_notification_outbox")
            )
        ).one()
        assert notification.payload == {
            "schemaVersion": 1,
            "recruitmentId": recruitment["id"],
            "snapshot": snapshot,
        }
        assert notification.channel_id == "123456789"
        assert notification.notification_type == "RECRUITMENT_MESSAGE_CREATE"
        assert (
            await connection.scalar(text("SELECT after_summary FROM admin_audit_log")) == snapshot
        )
        event = await connection.scalar(text("SELECT payload FROM outbox_events"))
        assert event == {"type": "invalidate", "resourceId": recruitment["id"], "revision": 0}
    changed = await runtime_client.post(PATH, json={**BODY, "bestOf": 5}, headers=headers)
    assert_failure(
        changed, 409, "idempotency_conflict", "같은 Idempotency-Key가 다른 요청에 사용되었습니다."
    )
    assert await counts(owner) == (1, 1, 1, 1, 1, 1)


async def test_rust_python_idempotency_replay_both_directions(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    headers = await setup(owner, runtime_client)
    payload = {
        "operation": "recruitment_create",
        "guild_id": GUILD_A,
        "user_id": USER_ID,
        "key": "rust-first",
        "start": START,
        "best_of": 3,
        "hard_fearless": True,
    }
    rust_snapshot = await asyncio.to_thread(rust_auth, payload)
    response = await runtime_client.post(
        PATH, json=BODY, headers={**headers, "idempotency-key": "rust-first"}
    )
    assert response.status_code == 201 and response.json()["data"] == rust_snapshot
    python_response = await runtime_client.post(PATH, json=BODY, headers=headers)
    assert python_response.status_code == 201
    replay = await asyncio.to_thread(rust_auth, {**payload, "key": "browser-create-1"})
    assert replay == python_response.json()["data"]
    assert await counts(owner) == (2, 2, 2, 2, 2, 2)


@pytest.mark.parametrize(
    "mode,status,code,message",
    [
        (
            "channel",
            409,
            "recruitment_channel_not_configured",
            "Discord 모집 채널을 먼저 설정해야 내전을 만들 수 있습니다.",
        ),
        (
            "season",
            409,
            "no_active_season",
            "길드에 진행 중인 시즌이 없어 내전을 만들 수 없습니다.",
        ),
        (
            "member",
            403,
            "guild_membership_required",
            "해당 길드의 활성 멤버만 내전을 만들 수 있습니다.",
        ),
        ("disabled", 409, "guild_inactive", "현재 길드에서는 command를 실행할 수 없습니다."),
        ("csrf", 403, "csrf_validation_failed", "요청 출처를 확인할 수 없습니다."),
        ("key", 400, "invalid_idempotency_key", "Idempotency-Key 헤더가 필요합니다."),
        ("late_failure", 500, "internal_error", "요청을 처리하는 중 내부 오류가 발생했습니다."),
    ],
)
async def test_failed_create_rolls_back_all_writes(
    owner: AsyncEngine,
    runtime_client: httpx.AsyncClient,
    mode: str,
    status: int,
    code: str,
    message: str,
) -> None:
    headers = await setup(owner, runtime_client)
    async with owner.begin() as connection:
        if mode == "channel":
            await connection.execute(text("UPDATE guild_config SET recruitment_channel_id = NULL"))
        elif mode == "season":
            await connection.execute(text("UPDATE seasons SET status = 'CLOSED', ended_at = now()"))
        elif mode == "member":
            await connection.execute(text("UPDATE guild_members SET status = 'INACTIVE'"))
        elif mode == "disabled":
            await connection.execute(text("UPDATE guilds SET status = 'DISABLED'"))
        elif mode == "late_failure":
            await connection.execute(
                text("REVOKE INSERT ON outbox_events FROM fastapi_test_runtime")
            )
    if mode == "csrf":
        headers.pop("x-csrf-token")
    if mode == "key":
        headers.pop("idempotency-key")
    try:
        response = await runtime_client.post(PATH, json=BODY, headers=headers)
        assert_failure(response, status, code, message)
        assert await counts(owner) == (0, 0, 0, 0, 0, 0)
        async with owner.connect() as connection:
            assert await connection.scalar(text("SELECT next_value FROM guild_counters")) == 1
    finally:
        if mode == "late_failure":
            async with owner.begin() as connection:
                await connection.execute(
                    text("GRANT INSERT ON outbox_events TO fastapi_test_runtime")
                )


async def test_bot_creation_syncs_actor_and_skips_browser_notification(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    await setup(owner, runtime_client)
    actor = {
        "id": "123456789123456789",
        "globalDisplayName": " Bot User ",
        "displayName": " Guild User ",
        "avatarHash": None,
        "roleIds": ["123", "123"],
    }
    headers = {"authorization": f"Bearer {BOT}", "idempotency-key": "987654321"}
    response = await runtime_client.post(PATH, json={**BODY, "actor": actor}, headers=headers)
    assert response.status_code == 201
    assert response.json()["data"]["recruitment"]["createdById"] == actor["id"]
    assert await counts(owner) == (1, 1, 1, 1, 0, 1)
    async with owner.connect() as connection:
        assert (
            await connection.scalar(
                text("SELECT display_name FROM users WHERE user_id = :id"), {"id": actor["id"]}
            )
            == "Bot User"
        )
        assert await connection.scalar(text("SELECT count(*) FROM guild_member_discord_roles")) == 1
    replay = await runtime_client.post(PATH, json={**BODY, "actor": actor}, headers=headers)
    assert replay.json()["data"] == response.json()["data"]
    conflict = await runtime_client.post(
        PATH, json={**BODY, "actor": actor, "bestOf": 5}, headers=headers
    )
    assert_failure(
        conflict, 409, "idempotency_conflict", "같은 interaction ID가 다른 요청에 사용되었습니다."
    )
    denied = await runtime_client.post(
        PATH, json=BODY, headers={**headers, "authorization": "Bearer incorrect"}
    )
    assert_failure(denied, 401, "unauthorized", "인증이 필요합니다.")


async def test_bot_identity_conflict_rolls_back_sync_and_command(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    await setup(owner, runtime_client)
    discord_id = "123456789123456789"
    existing_id = "123456789123456788"
    async with owner.begin() as connection:
        await connection.execute(
            text(
                "INSERT INTO users "
                "(user_id, identity_kind, display_name, created_at, updated_at) "
                "VALUES (:id, 'DISCORD', 'Existing', now(), now())"
            ),
            {"id": existing_id},
        )
        await connection.execute(
            text(
                "INSERT INTO user_discord_identities "
                "(user_id, discord_id, created_at, updated_at) "
                "VALUES (:id, :discord, now(), now())"
            ),
            {"id": existing_id, "discord": discord_id},
        )
    actor = {"id": discord_id, "globalDisplayName": "New", "displayName": "New", "roleIds": []}
    response = await runtime_client.post(
        PATH,
        json={**BODY, "actor": actor},
        headers={"authorization": f"Bearer {BOT}", "idempotency-key": "987654321"},
    )
    assert_failure(
        response,
        409,
        "user_identity_conflict",
        "이 Discord 계정 식별자는 다른 사용자 유형에서 이미 사용 중입니다.",
    )
    assert await counts(owner) == (0, 0, 0, 0, 0, 0)
    async with owner.connect() as connection:
        assert (
            await connection.scalar(
                text("SELECT count(*) FROM users WHERE user_id = :id"), {"id": discord_id}
            )
            == 0
        )


async def test_axum_replay_precedes_membership_check(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    headers = await setup(owner, runtime_client)
    created = await runtime_client.post(PATH, json=BODY, headers=headers)
    assert created.status_code == 201
    async with owner.begin() as connection:
        await connection.execute(text("UPDATE guild_members SET status = 'INACTIVE'"))
    replay = await runtime_client.post(PATH, json=BODY, headers=headers)
    assert replay.status_code == 201 and replay.json()["data"] == created.json()["data"]
    denied = await runtime_client.post(
        PATH, json=BODY, headers={**headers, "idempotency-key": "new-request"}
    )
    assert_failure(
        denied, 403, "guild_membership_required", "해당 길드의 활성 멤버만 내전을 만들 수 있습니다."
    )
    assert await counts(owner) == (1, 1, 1, 1, 1, 1)


@pytest.mark.parametrize(
    "body,code,message",
    [
        (
            {"bestOf": 2},
            "invalid_recruitment_best_of",
            "시리즈 형식은 BO1, BO3 또는 BO5만 선택할 수 있습니다.",
        ),
        (
            {"bestOf": "3"},
            "invalid_recruitment_create_request",
            "내전 생성 요청 형식이 올바르지 않습니다.",
        ),
        (
            {"actor": {}},
            "invalid_recruitment_create_request",
            "내전 생성 요청 형식이 올바르지 않습니다.",
        ),
        (
            {"expectedStartAt": "2026-09-13T20:00:00"},
            "invalid_expected_start_at",
            "예상 시작 시간은 시간대가 포함된 ISO 8601 형식이어야 합니다.",
        ),
    ],
)
async def test_create_validation_contract(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, body: dict, code: str, message: str
) -> None:
    headers = await setup(owner, runtime_client)
    response = await runtime_client.post(PATH, json={**BODY, **body}, headers=headers)
    assert_failure(response, 422, code, message)
    assert await counts(owner) == (0, 0, 0, 0, 0, 0)


async def test_defaults_and_expired_idempotency_key(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    headers = await setup(owner, runtime_client)
    first = await runtime_client.post(PATH, json={"expectedStartAt": START}, headers=headers)
    assert first.status_code == 201
    assert first.json()["data"]["recruitment"]["bestOf"] == 3
    async with owner.begin() as connection:
        await connection.execute(
            text(
                "UPDATE idempotency_records "
                "SET created_at = now() - interval '2 days', "
                "expires_at = now() - interval '1 day'"
            )
        )
    second = await runtime_client.post(PATH, json=BODY, headers=headers)
    assert second.status_code == 201
    assert second.json()["data"]["recruitment"]["id"] != first.json()["data"]["recruitment"]["id"]
    uuid.UUID(second.json()["data"]["recruitment"]["id"])
    assert await counts(owner) == (2, 2, 2, 2, 2, 1)
