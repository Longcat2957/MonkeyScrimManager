import asyncio
import json
import uuid
from datetime import datetime

import pytest
from sqlalchemy import text

from monkeybot_api.repositories.effects import notification
from tests.support.assertions import assert_failure
from tests.support.guilds.context import GUILD_A
from tests.support.notifications import BOT_HEADERS, claim, lease, oracle, post, seed

pytestmark = pytest.mark.postgres


@pytest.mark.parametrize("rust_first", [False, True])
@pytest.mark.parametrize("auction,version", [(False, 1), (True, 1), (True, 2)])
async def test_typed_ack_cross_language_binding(
    owner, runtime_client, rust_first, auction, version
):
    await seed(owner, runtime_client, auction=auction, version=version)
    action = "auction-recruitment-message/ack" if auction else "recruitment-message/ack"
    if rust_first:
        rows = (await oracle("claim", {"instanceId": "bot:1"}))["notifications"]
        response = await post(runtime_client, action, lease(rows[0]))
        assert response.status_code == 200, response.text
        result = response.json()["data"]
    else:
        rows = await claim(runtime_client)
        result = await oracle(action, lease(rows[0]))
    assert len(rows) == 1
    assert rows[0]["attempts"] == 1
    assert len(rows[0]["discordNonce"]) == 22
    key = "auctionRecruitment" if auction else "recruitment"
    assert result["acknowledged"] is True and result[key]["revision"] == 1
    async with owner.connect() as c:
        table = "auction_recruitments" if auction else "recruitments"
        bound = (await c.execute(text(f"SELECT message_id, revision FROM {table}"))).one()
        assert bound == ("987", 1)
        assert await c.scalar(text("SELECT count(*) FROM bot_notification_outbox")) == 1
    assert_failure(
        await post(runtime_client, action, lease(rows[0])),
        409,
        "notification_lease_conflict",
        "알림 전송 lease가 만료되었거나 다른 Bot 인스턴스로 이전되었습니다.",
    )


@pytest.mark.parametrize("auction", [False, True])
@pytest.mark.parametrize("cancelled", [False, True])
async def test_ack_reconciles_changed_snapshot(owner, runtime_client, auction, cancelled):
    await seed(owner, runtime_client, auction=auction)
    row = (await claim(runtime_client))[0]
    table = "auction_recruitments" if auction else "recruitments"
    async with owner.begin() as c:
        await c.execute(
            text(f"UPDATE {table} SET revision = 3, status = :status"),
            {"status": "CANCELLED" if cancelled else "OPEN"},
        )
    action = "auction-recruitment-message/ack" if auction else "recruitment-message/ack"
    response = await post(runtime_client, action, lease(row))
    assert response.status_code == 200, response.text
    following = (await oracle("claim", {"instanceId": "bot:2"}))["notifications"]
    assert len(following) == 1
    assert following[0]["type"] == ("AUCTION_" if auction else "") + "RECRUITMENT_MESSAGE_" + (
        "DELETE" if cancelled else "UPDATE"
    )
    snapshot = following[0]["payload"]["snapshot"]["recruitment"]
    assert snapshot["revision"] == 4 and snapshot["messageId"] == "987"
    assert following[0]["payload"]["schemaVersion"] == (2 if auction else 1)
    assert (await oracle("ack", lease(following[0], "bot:2")))["acknowledged"]


async def test_ack_type_payload_and_binding_failures_rollback(owner, runtime_client):
    await seed(owner, runtime_client)
    row = (await claim(runtime_client))[0]
    assert_failure(
        await post(runtime_client, "ack", lease(row)),
        422,
        "recruitment_message_create_requires_typed_ack",
        "모집 메시지 생성 알림은 전용 확인 경로로 처리해야 합니다.",
    )
    assert_failure(
        await post(runtime_client, "auction-recruitment-message/ack", lease(row)),
        422,
        "notification_type_mismatch",
        "이 알림은 모집 메시지 확인으로 처리할 수 없습니다.",
    )
    async with owner.begin() as c:
        original = await c.scalar(text("SELECT payload FROM bot_notification_outbox"))
        await c.execute(
            text(
                "UPDATE bot_notification_outbox "
                "SET payload = payload || jsonb_build_object('extra', 1)"
            )
        )
    assert_failure(
        await post(runtime_client, "recruitment-message/ack", lease(row)),
        500,
        "notification_outbox_invariant_failed",
        "저장된 알림 전송 상태를 처리할 수 없습니다.",
    )
    async with owner.begin() as c:
        await c.execute(
            text("UPDATE bot_notification_outbox SET payload = CAST(:p AS jsonb)"),
            {"p": json.dumps(original)},
        )
        await c.execute(text("UPDATE recruitments SET channel_id = '555', message_id = '666'"))
    assert_failure(
        await post(runtime_client, "recruitment-message/ack", lease(row)),
        409,
        "recruitment_message_bind_conflict",
        "모집 메시지를 현재 모집 상태에 연결할 수 없습니다.",
    )
    async with owner.connect() as c:
        assert await c.scalar(text("SELECT delivered_at FROM bot_notification_outbox")) is None
        assert await c.scalar(text("SELECT lease_token FROM bot_notification_outbox")) == uuid.UUID(
            row["leaseToken"]
        )


async def test_claim_serializes_same_recruitment_and_skips_locked_rows(owner, runtime_client):
    path, _ = await seed(owner, runtime_client)
    async with owner.begin() as c:
        await notification(
            c,
            {"guild": GUILD_A, "now": datetime.now().astimezone()},
            "RECRUITMENT_MESSAGE_UPDATE",
            "789",
            "update-1",
            {"recruitmentId": path.rsplit("/", 1)[1]},
        )
        await notification(
            c,
            {"guild": GUILD_A, "now": datetime.now().astimezone()},
            "SERIES_RESULT_CREATE",
            "789",
            "independent",
            {},
        )
    responses = await asyncio.gather(claim(runtime_client, "a"), claim(runtime_client, "b"))
    rows = [row for batch in responses for row in batch]
    assert len(rows) == 2 and len({row["id"] for row in rows}) == 2
    assert {row["type"] for row in rows} == {"RECRUITMENT_MESSAGE_CREATE", "SERIES_RESULT_CREATE"}
    assert await claim(runtime_client, "c") == []
    # A pending row lock must not block another worker from claiming an independent notification.
    async with owner.begin() as c:
        await c.execute(
            text(
                "UPDATE bot_notification_outbox "
                "SET lease_owner=NULL, lease_token=NULL, lease_until=NULL"
            )
        )
    async with owner.begin() as c:
        await c.execute(
            text(
                "SELECT id FROM bot_notification_outbox "
                "WHERE notification_type = 'RECRUITMENT_MESSAGE_CREATE' FOR UPDATE"
            )
        )
        rows = await asyncio.wait_for(claim(runtime_client, "d"), 2)
        assert len(rows) == 1 and rows[0]["type"] == "SERIES_RESULT_CREATE"


async def test_expired_lease_reassignment_and_exhaustion(owner, runtime_client):
    await seed(owner, runtime_client, generic=True)
    first = (await claim(runtime_client))[0]
    async with owner.begin() as c:
        await c.execute(
            text("UPDATE bot_notification_outbox SET lease_until=now()-interval '1 second'")
        )
    second = (await oracle("claim", {"instanceId": "bot:2"}))["notifications"][0]
    assert second["id"] == first["id"] and second["attempts"] == 2
    assert (
        second["discordNonce"] == first["discordNonce"]
        and second["leaseToken"] != first["leaseToken"]
    )
    assert (await post(runtime_client, "ack", lease(first))).status_code == 409
    async with owner.begin() as c:
        await c.execute(
            text(
                "UPDATE bot_notification_outbox "
                "SET attempts=8, lease_until=now()-interval '1 second'"
            )
        )
    assert await claim(runtime_client) == []
    async with owner.connect() as c:
        assert (
            await c.scalar(text("SELECT failure_code FROM bot_notification_outbox"))
            == "MAX_ATTEMPTS"
        )


@pytest.mark.parametrize(
    "failure,attempts,code",
    [
        ({"kind": "HTTP", "status": 403}, 1, "DISCORD_HTTP_4XX"),
        ({"kind": "HTTP", "status": 429, "retryAfterMs": 2000}, 1, None),
        ({"kind": "HTTP", "status": 503}, 1, None),
        ({"kind": "TRANSPORT"}, 1, None),
        ({"kind": "TRANSPORT"}, 8, "MAX_ATTEMPTS"),
        ({"kind": "DELIVERY_UNKNOWN"}, 8, "DELIVERY_UNKNOWN"),
    ],
)
async def test_nack_dispositions_and_rust_reclaim(owner, runtime_client, failure, attempts, code):
    await seed(owner, runtime_client, generic=True)
    row = (await oracle("claim", {"instanceId": "bot:1"}))["notifications"][0]
    async with owner.begin() as c:
        await c.execute(text("UPDATE bot_notification_outbox SET attempts=:n"), {"n": attempts})
    body = {**lease(row, message=False), "failure": {**failure, "message": "safe test failure"}}
    response = await post(runtime_client, "nack", body)
    assert response.status_code == 200, response.text
    data = response.json()["data"]
    assert data["attempts"] == attempts
    if code:
        assert data == {"state": "FAILED", "failureCode": code, "attempts": attempts}
        assert await claim(runtime_client) == []
    else:
        assert data["state"] == "RETRY_SCHEDULED"
        assert await claim(runtime_client) == []
        async with owner.begin() as c:
            await c.execute(text("UPDATE bot_notification_outbox SET available_at=now()"))
        reclaimed = (await oracle("claim", {"instanceId": "bot:2"}))["notifications"][0]
        assert reclaimed["attempts"] == attempts + 1
        assert (await post(runtime_client, "ack", lease(reclaimed, "bot:2"))).status_code == 200


@pytest.mark.parametrize(
    "failure",
    [
        {"kind": "HTTP"},
        {"kind": "HTTP", "status": 399},
        {"kind": "HTTP", "status": 503, "retryAfterMs": 1},
        {"kind": "HTTP", "status": 429, "retryAfterMs": 0},
        {"kind": "HTTP", "status": 429, "retryAfterMs": 3600001},
        {"kind": "TRANSPORT", "status": 500},
        {"kind": "DELIVERY_UNKNOWN", "retryAfterMs": 1},
    ],
)
async def test_invalid_failure_preserves_lease(owner, runtime_client, failure):
    await seed(owner, runtime_client, generic=True)
    row = (await claim(runtime_client))[0]
    assert_failure(
        await post(
            runtime_client,
            "nack",
            {**lease(row, message=False), "failure": {**failure, "message": "failure"}},
        ),
        400,
        "invalid_notification_request",
        "Bot 알림 전송 요청 값이 올바르지 않습니다.",
    )
    assert (await oracle("ack", lease(row)))["acknowledged"]


async def test_heartbeat_upsert_and_auth_input_boundaries(owner, runtime_client):
    path = "/internal/heartbeat"
    body = {"instanceId": "bot-heartbeat-test", "version": "1.2.3", "commitSha": "abc123"}
    response = await runtime_client.post(path, json=body)
    assert response.status_code == 401
    response = await runtime_client.post(path, json=body, headers=BOT_HEADERS)
    assert response.status_code == 200, response.text
    first = response.json()["data"]["lastHeartbeatAt"]
    response = await runtime_client.post(
        path, json={**body, "version": "1.2.4", "commitSha": None}, headers=BOT_HEADERS
    )
    assert response.json()["data"]["accepted"]
    async with owner.connect() as c:
        row = (
            (
                await c.execute(
                    text("SELECT * FROM bot_instances WHERE instance_id='bot-heartbeat-test'")
                )
            )
            .mappings()
            .one()
        )
        assert row["version"] == "1.2.4" and row["commit_sha"] is None
        assert row["started_at"] == datetime.fromisoformat(first.replace("Z", "+00:00"))
    for invalid in ({"instanceId": "-bot"}, {"version": "has space"}, {"commitSha": "bad:sha"}):
        assert_failure(
            await runtime_client.post(path, json={**body, **invalid}, headers=BOT_HEADERS),
            422,
            "invalid_heartbeat",
            "Bot heartbeat 요청 값이 올바르지 않습니다.",
        )
    for invalid in ({"instanceId": ""}, {"instanceId": "bad/name"}, {"limit": 0}, {"limit": 51}):
        assert_failure(
            await post(runtime_client, "claim", {"instanceId": "bot", **invalid}),
            400,
            "invalid_notification_request",
            "Bot 알림 전송 요청 값이 올바르지 않습니다.",
        )
    for invalid in ({"limit": -1}, {"limit": True}, {"extra": 1}):
        assert (
            await post(runtime_client, "claim", {"instanceId": "bot", **invalid})
        ).status_code == 422


@pytest.mark.parametrize("late_failure", ["revision", "dedup"])
async def test_ack_late_failure_rolls_back_delivery_and_binding(
    owner, runtime_client, late_failure
):
    path, _ = await seed(owner, runtime_client)
    row = (await claim(runtime_client))[0]
    async with owner.begin() as c:
        if late_failure == "revision":
            await c.execute(
                text("""
                UPDATE bot_notification_outbox SET payload = jsonb_set(payload,
                    '{snapshot,recruitment,revision}', 'null'::jsonb)
            """)
            )
        else:
            await c.execute(text("UPDATE recruitments SET revision=3, status='CANCELLED'"))
            await notification(
                c,
                {"guild": GUILD_A, "now": datetime.now().astimezone()},
                "RECRUITMENT_MESSAGE_DELETE",
                "789",
                f"recruitment:{path.rsplit('/', 1)[1]}:message:delete:4",
                {},
            )
    assert_failure(
        await post(runtime_client, "recruitment-message/ack", lease(row)),
        500,
        "notification_outbox_invariant_failed",
        "저장된 알림 전송 상태를 처리할 수 없습니다.",
    )
    async with owner.connect() as c:
        assert await c.scalar(text("SELECT message_id FROM recruitments")) is None
        assert await c.scalar(text("SELECT revision FROM recruitments")) == (
            3 if late_failure == "dedup" else 0
        )
        assert (
            await c.scalar(
                text("SELECT delivered_at FROM bot_notification_outbox WHERE id=:id"),
                {"id": uuid.UUID(row["id"])},
            )
            is None
        )


async def test_retry_delay_matches_rust_for_same_notification(owner, runtime_client):
    await seed(owner, runtime_client, generic=True)
    row = (await claim(runtime_client))[0]
    body = {**lease(row, message=False), "failure": {"kind": "TRANSPORT", "message": "timeout"}}
    async with owner.connect() as c:
        before_python = await c.scalar(text("SELECT clock_timestamp()"))
    response = await post(runtime_client, "nack", body)
    assert response.status_code == 200
    python_available = datetime.fromisoformat(
        response.json()["data"]["availableAt"].replace("Z", "+00:00")
    )
    async with owner.begin() as c:
        after_python = await c.scalar(text("SELECT clock_timestamp()"))
        await c.execute(
            text("""
            UPDATE bot_notification_outbox SET lease_owner='bot:1', lease_token=:token,
                lease_until=clock_timestamp()+interval '30 seconds'
        """),
            {"token": uuid.UUID(row["leaseToken"])},
        )
    async with owner.connect() as c:
        before_rust = await c.scalar(text("SELECT clock_timestamp()"))
    rust_result = await oracle("nack", body)
    rust_available = datetime.fromisoformat(rust_result["availableAt"].replace("Z", "+00:00"))
    async with owner.connect() as c:
        after_rust = await c.scalar(text("SELECT clock_timestamp()"))
    # Both calls must allow the same delay interval despite process/network latency.
    assert python_available - after_python <= rust_available - before_rust
    assert rust_available - after_rust <= python_available - before_python
