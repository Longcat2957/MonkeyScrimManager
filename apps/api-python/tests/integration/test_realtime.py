"""Actual Uvicorn /ws handshake, topic rules, fanout, and outbox leases."""

import asyncio
import json
import socket
import uuid
from collections.abc import AsyncIterator
from typing import Any

import pytest
import uvicorn
import websockets
from fastapi import FastAPI
from pydantic import SecretStr
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncEngine
from websockets.exceptions import InvalidStatus

from monkeybot_api.api.routers import realtime as realtime_router
from monkeybot_api.app import create_app
from monkeybot_api.core.config import Settings
from monkeybot_api.db.database import Database
from monkeybot_api.repositories import realtime as repository
from monkeybot_api.services.realtime import HUB_CAPACITY, RealtimeRuntime
from tests.support.auth.session import TOKEN, USER_ID, seed_session
from tests.support.database import isolated_database_url
from tests.support.guilds.context import GUILD_A, ROLE_A, guild_fixture

pytestmark = pytest.mark.postgres


@pytest.fixture
async def live_ws(owner: AsyncEngine, discord_stub) -> AsyncIterator[tuple[str, FastAPI]]:
    settings = Settings(
        env="test",
        public_origin="http://localhost:5173",
        database_url=SecretStr(isolated_database_url("FASTAPI_TEST_RUNTIME_URL")),
        discord_application_id="123456789012345678",
        discord_client_secret=SecretStr("integration-test-only"),
        oauth_encryption_key=SecretStr("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
    )
    app = create_app(settings)
    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listener.bind(("127.0.0.1", 0))
    listener.listen(128)
    port = listener.getsockname()[1]
    config = uvicorn.Config(
        app,
        host="127.0.0.1",
        port=port,
        lifespan="on",
        ws="websockets",
        ws_max_size=8 * 1024,
        ws_ping_interval=25,
        ws_ping_timeout=75,
        log_level="error",
    )
    server = uvicorn.Server(config)
    task = asyncio.create_task(server.serve(sockets=[listener]))
    try:
        for _ in range(100):
            if server.started:
                break
            if task.done():
                raise RuntimeError("Uvicorn stopped before readiness")
            await asyncio.sleep(0.01)
        assert server.started
        yield f"ws://127.0.0.1:{port}/ws", app
    finally:
        server.should_exit = True
        await asyncio.wait_for(task, 5)


async def seed_recruitment(owner: AsyncEngine) -> uuid.UUID:
    await seed_session(owner)
    season = await guild_fixture(owner, GUILD_A, "Alpha", ROLE_A)
    recruitment = uuid.uuid7()
    async with owner.begin() as c:
        await c.execute(
            text("""
            INSERT INTO recruitments
              (guild_id, id, season_id, display_number, title, target_count,
               status, revision, created_by, created_at, updated_by, updated_at)
            VALUES (:guild, :id, :season, 1, 'WebSocket fixture', 10,
                    'OPEN', 0, :user, now(), :user, now())
            """),
            {"guild": GUILD_A, "id": recruitment, "season": season, "user": USER_ID},
        )
    return recruitment


def connect(url: str, *, token: str | None = TOKEN, origin: str | None = "http://localhost:5173"):
    headers = {"Cookie": f"monkeybot_session={token}"} if token is not None else None
    return websockets.connect(url, origin=origin, additional_headers=headers, proxy=None)


async def received(ws: Any) -> dict[str, Any]:
    return json.loads(await asyncio.wait_for(ws.recv(), 3))


async def enqueue(
    owner: AsyncEngine,
    resource: uuid.UUID,
    *,
    topic: str,
    revision: int = 1,
) -> uuid.UUID:
    event_id = uuid.uuid7()
    async with owner.begin() as c:
        await c.execute(
            text("""
            INSERT INTO outbox_events
              (id, guild_id, topic, aggregate_type, aggregate_id, aggregate_revision,
               event_type, payload, occurred_at, available_at)
            VALUES (:id, :guild, :topic, 'RECRUITMENT', :resource, :revision,
                    'INVALIDATE', '{}'::jsonb, now(), now())
            """),
            {
                "id": event_id,
                "guild": GUILD_A,
                "topic": topic,
                "resource": str(resource),
                "revision": revision,
            },
        )
    return event_id


async def test_handshake_denials_and_subscription_fanout(
    owner: AsyncEngine, live_ws: tuple[str, FastAPI]
) -> None:
    resource = await seed_recruitment(owner)
    url, _ = live_ws
    topic = f"guild:{GUILD_A}:recruitment:{resource}"
    draft = f"guild:{GUILD_A}:recruitment-entry-draft:{resource}"
    with pytest.raises(InvalidStatus) as unauthenticated:
        async with connect(url, token=None):
            pass
    assert unauthenticated.value.response.status_code == 401
    assert json.loads(unauthenticated.value.response.body)["error"]["code"] == "unauthorized"
    with pytest.raises(InvalidStatus) as wrong_origin:
        async with connect(url, origin="http://wrong.example"):
            pass
    assert wrong_origin.value.response.status_code == 403
    assert (
        json.loads(wrong_origin.value.response.body)["error"]["code"] == "origin_validation_failed"
    )

    async with connect(url) as ws:
        await ws.send(json.dumps({"type": "subscribe", "topics": [topic, draft]}))
        assert await received(ws) == {"type": "subscribed", "topics": sorted([topic, draft])}
        await ws.send(json.dumps({"type": "subscribe", "topics": [topic, topic]}))
        assert await received(ws) == {
            "type": "error",
            "code": "invalid_topic",
            "message": "실시간 topic 형식이 올바르지 않습니다.",
        }
        await ws.send(
            json.dumps(
                {
                    "type": "subscribe",
                    "topics": [topic, f"guild:{GUILD_A}:recruitment:{uuid.uuid7()}"],
                }
            )
        )
        assert (await received(ws))["code"] == "subscription_not_found"
        await ws.send(
            json.dumps(
                {"type": "subscribe", "topics": [f"guild:{GUILD_A}:auction-match:{resource}"]}
            )
        )
        assert (await received(ws))["code"] == "unsupported_topic"
        await ws.send(json.dumps({"type": "subscribe", "topics": [topic], "unknown": True}))
        assert (await received(ws))["code"] == "invalid_message"
        await ws.send(json.dumps({"type": [], "topics": [topic]}))
        assert (await received(ws))["code"] == "invalid_message"
        await ws.send(b"invalid-binary-frame")
        assert (await received(ws))["code"] == "invalid_message"
        event_id = await enqueue(owner, resource, topic=topic)
        event = await received(ws)
        assert event["type"] == "invalidate"
        assert event["eventId"] == str(event_id)
        assert event["topic"] == topic
        assert event["resourceId"] == str(resource)
        assert event["revision"] == 1
        await ws.send(json.dumps({"type": "unsubscribe", "topics": [topic, draft]}))
        assert await received(ws) == {"type": "subscribed", "topics": []}
    for _ in range(100):
        async with owner.connect() as c:
            published = await c.scalar(
                text("SELECT published_at IS NOT NULL FROM outbox_events WHERE id=:id"),
                {"id": event_id},
            )
        if published:
            break
        await asyncio.sleep(0.02)
    assert published


async def test_recheck_revokes_membership_and_invalid_event_is_nacked(
    owner: AsyncEngine, live_ws: tuple[str, FastAPI], monkeypatch: pytest.MonkeyPatch
) -> None:
    resource = await seed_recruitment(owner)
    url, _ = live_ws
    topic = f"guild:{GUILD_A}:recruitment:{resource}"
    monkeypatch.setattr(realtime_router, "RECHECK_SECONDS", 0.1)
    async with connect(url) as ws:
        await ws.send(json.dumps({"type": "subscribe", "topics": [topic]}))
        assert await received(ws) == {"type": "subscribed", "topics": [topic]}
        async with owner.begin() as c:
            await c.execute(
                text("""
                UPDATE guild_members SET status='INACTIVE'
                WHERE guild_id=:guild AND user_id=:user
            """),
                {"guild": GUILD_A, "user": USER_ID},
            )
        assert await received(ws) == {"type": "subscribed", "topics": []}
    invalid = await enqueue(owner, resource, topic=f"guild:{GUILD_A}:invalid:{resource}")
    for _ in range(100):
        async with owner.connect() as c:
            row = (
                await c.execute(
                    text("""
                SELECT attempts, last_error, published_at FROM outbox_events WHERE id=:id
            """),
                    {"id": invalid},
                )
            ).one()
        if row.attempts:
            break
        await asyncio.sleep(0.02)
    assert row.attempts == 1
    assert row.last_error == "invalid_realtime_event"
    assert row.published_at is None


def test_local_hub_marks_lag_and_remains_bounded(settings: Settings) -> None:
    runtime = RealtimeRuntime(Database(settings), settings)
    queue = runtime.subscribe()
    for number in range(HUB_CAPACITY + 1):
        runtime.fan_out({"topic": "example", "revision": number})
    assert queue.qsize() <= HUB_CAPACITY
    assert queue.get_nowait() is None
    assert queue.get_nowait()["revision"] == HUB_CAPACITY
    runtime.unsubscribe(queue)


async def test_old_outbox_lease_cannot_ack_or_nack_new_claim(owner: AsyncEngine) -> None:
    resource = await seed_recruitment(owner)
    topic = f"guild:{GUILD_A}:recruitment:{resource}"
    event_id = await enqueue(owner, resource, topic=topic)
    settings = Settings(
        env="test",
        public_origin="http://localhost:5173",
        database_url=SecretStr(isolated_database_url("FASTAPI_TEST_RUNTIME_URL")),
    )
    database = Database(settings)
    try:
        await database.start()
        claimed = await repository.claim_any(database, "owner-before")
        assert len(claimed) == 1 and claimed[0]["id"] == event_id
        new_token = uuid.uuid7()
        async with owner.begin() as c:
            await c.execute(
                text("""
                UPDATE outbox_events SET lease_owner='owner-after', lease_token=:token
                WHERE id=:id
            """),
                {"token": new_token, "id": event_id},
            )
        assert not await repository.ack(database, claimed[0])
        assert not await repository.nack(database, claimed[0], "late_failure")
        async with owner.connect() as c:
            row = (
                await c.execute(
                    text("""
                SELECT lease_owner, lease_token, attempts, published_at
                FROM outbox_events WHERE id=:id
            """),
                    {"id": event_id},
                )
            ).one()
        assert row.lease_owner == "owner-after"
        assert row.lease_token == new_token
        assert row.attempts == 0 and row.published_at is None
    finally:
        await database.close()
