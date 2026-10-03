"""Two API instances sharing PostgreSQL and a disposable Valkey."""

import asyncio
import json
import socket
import subprocess
import uuid
from collections.abc import AsyncIterator, Awaitable, Callable, Iterator
from contextlib import asynccontextmanager
from typing import Any

import pytest
import uvicorn
import websockets
from fastapi import FastAPI
from pydantic import SecretStr
from redis.exceptions import RedisError
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncEngine

from monkeybot_api.api.routers import realtime as realtime_router
from monkeybot_api.app import create_app
from monkeybot_api.core.config import Settings
from monkeybot_api.db.database import Database
from monkeybot_api.repositories import realtime as repository
from monkeybot_api.services.realtime import HUB_CAPACITY
from tests.support.auth.session import TOKEN, USER_ID, seed_session
from tests.support.database import isolated_database_url
from tests.support.guilds.context import GUILD_A, ROLE_A, guild_fixture

pytestmark = pytest.mark.postgres


@pytest.fixture
def valkey_server() -> Iterator[tuple[str, str]]:
    name = f"monkeybot-fastapi-valkey-{uuid.uuid4().hex[:12]}"
    port_reservation = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    port_reservation.bind(("127.0.0.1", 0))
    port = port_reservation.getsockname()[1]
    port_reservation.close()
    try:
        subprocess.run(
            [
                "docker",
                "run",
                "--detach",
                "--name",
                name,
                "--publish",
                f"127.0.0.1:{port}:6379",
                "valkey/valkey:8-alpine",
                "valkey-server",
                "--save",
                "",
                "--appendonly",
                "no",
            ],
            check=True,
            capture_output=True,
            text=True,
            timeout=60,
        )
        yield name, f"redis://127.0.0.1:{port}/0"
    finally:
        subprocess.run(
            ["docker", "rm", "--force", name],
            check=False,
            capture_output=True,
            text=True,
            timeout=20,
        )


def settings(valkey_url: str | None = None) -> Settings:
    return Settings(
        env="test",
        public_origin="http://localhost:5173",
        database_url=SecretStr(isolated_database_url("FASTAPI_TEST_RUNTIME_URL")),
        valkey_url=SecretStr(valkey_url) if valkey_url is not None else None,
        discord_application_id="123456789012345678",
        discord_client_secret=SecretStr("integration-test-only"),
        oauth_encryption_key=SecretStr("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
    )


@asynccontextmanager
async def live_instance(config: Settings) -> AsyncIterator[tuple[str, FastAPI]]:
    app = create_app(config)
    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listener.bind(("127.0.0.1", 0))
    listener.listen(128)
    port = listener.getsockname()[1]
    server = uvicorn.Server(
        uvicorn.Config(
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
    )
    task = asyncio.create_task(server.serve(sockets=[listener]))
    try:
        await wait_until(lambda: asyncio.sleep(0, result=server.started or task.done()))
        if task.done() or not server.started:
            raise RuntimeError("Uvicorn stopped before readiness")
        yield f"ws://127.0.0.1:{port}/ws", app
    finally:
        server.should_exit = True
        await asyncio.wait_for(task, 5)
        listener.close()


async def wait_until(check: Callable[[], Awaitable[Any]], *, wait_seconds: float = 10.0) -> Any:
    async with asyncio.timeout(wait_seconds):
        while True:
            result = await check()
            if result:
                return result
            await asyncio.sleep(0.05)


async def docker_control(*args: str) -> None:
    process = await asyncio.create_subprocess_exec(
        "docker", *args, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE
    )
    async with asyncio.timeout(20):
        _, error = await process.communicate()
    if process.returncode != 0:
        raise RuntimeError(f"Disposable Valkey control failed: {error.decode().strip()}")


async def seed_recruitment(owner: AsyncEngine) -> uuid.UUID:
    await seed_session(owner)
    season = await guild_fixture(owner, GUILD_A, "Alpha", ROLE_A)
    resource = uuid.uuid7()
    async with owner.begin() as c:
        await c.execute(
            text("""
            INSERT INTO recruitments
              (guild_id, id, season_id, display_number, title, target_count,
               status, revision, created_by, created_at, updated_by, updated_at)
            VALUES (:guild, :id, :season, 1, 'Multi-instance fixture', 10,
                    'OPEN', 0, :user, now(), :user, now())
            """),
            {"guild": GUILD_A, "id": resource, "season": season, "user": USER_ID},
        )
    return resource


async def commit_event(owner: AsyncEngine, resource: uuid.UUID, revision: int) -> uuid.UUID:
    event_id = uuid.uuid7()
    topic = f"guild:{GUILD_A}:recruitment:{resource}"
    async with owner.begin() as c:
        await c.execute(
            text("""
            UPDATE recruitments SET revision=:revision, updated_at=now()
            WHERE guild_id=:guild AND id=:resource
            """),
            {"revision": revision, "guild": GUILD_A, "resource": resource},
        )
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


async def outbox_state(owner: AsyncEngine, event_id: uuid.UUID) -> Any:
    async with owner.connect() as c:
        return (
            await c.execute(
                text("""
                SELECT attempts, published_at, lease_owner, lease_token, last_error
                FROM outbox_events WHERE id=:id
                """),
                {"id": event_id},
            )
        ).one()


def connect(url: str):
    return websockets.connect(
        url,
        origin="http://localhost:5173",
        additional_headers={"Cookie": f"monkeybot_session={TOKEN}"},
        proxy=None,
    )


async def received(ws: Any, *, wait_seconds: float = 5.0) -> dict[str, Any]:
    return json.loads(await asyncio.wait_for(ws.recv(), wait_seconds))


async def receive_event(
    ws: Any, event_id: uuid.UUID, *, wait_seconds: float = 5.0
) -> dict[str, Any]:
    async with asyncio.timeout(wait_seconds):
        while True:
            frame = await received(ws, wait_seconds=wait_seconds)
            if frame.get("eventId") == str(event_id):
                return frame


async def test_two_instances_relay_recover_and_resync(
    owner: AsyncEngine,
    valkey_server: tuple[str, str],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    container, valkey_url = valkey_server
    resource = await seed_recruitment(owner)
    topic = f"guild:{GUILD_A}:recruitment:{resource}"

    async with live_instance(settings(valkey_url)) as (url_a, app_a):
        async with live_instance(settings(valkey_url)) as (url_b, app_b):
            database_a: Database = app_a.state.database
            database_b: Database = app_b.state.database
            assert database_a is not database_b
            assert app_a.state.realtime is not app_b.state.realtime
            valkey = database_a.valkey
            assert valkey is not None

            async def valkey_healthy() -> bool:
                try:
                    return bool(await valkey.ping())
                except RedisError, OSError, TimeoutError:
                    return False

            async def subscribers_ready() -> bool:
                try:
                    clients = await valkey.client_list()
                except RedisError, OSError, TimeoutError:
                    return False
                return sum(int(client["psub"]) > 0 for client in clients) >= 2

            async def valkey_unavailable() -> bool:
                return await database_a.valkey_status() == "unavailable"

            async def published(event_id: uuid.UUID) -> Any:
                state = await outbox_state(owner, event_id)
                return state if state.published_at is not None else None

            async def failed(event_id: uuid.UUID) -> Any:
                state = await outbox_state(owner, event_id)
                return state if state.attempts >= 1 else None

            await wait_until(valkey_healthy)
            await wait_until(subscribers_ready)

            async with connect(url_a) as ws_a, connect(url_b) as ws_b:
                for ws in (ws_a, ws_b):
                    await ws.send(json.dumps({"type": "subscribe", "topics": [topic]}))
                    assert await received(ws) == {"type": "subscribed", "topics": [topic]}

                first = await commit_event(owner, resource, 1)
                for ws in (ws_a, ws_b):
                    frame = await receive_event(ws, first)
                    assert frame["topic"] == topic and frame["revision"] == 1
                assert (await wait_until(lambda: published(first))).published_at

                await docker_control("stop", "--time", "0", container)
                await wait_until(valkey_unavailable)

                second = await commit_event(owner, resource, 2)
                failure = await wait_until(lambda: failed(second), wait_seconds=8.0)
                assert failure.published_at is None
                assert failure.last_error == "realtime_publish_failed"
                async with owner.connect() as c:
                    assert (
                        await c.scalar(
                            text("SELECT revision FROM recruitments WHERE id=:id"), {"id": resource}
                        )
                        == 2
                    )
                readiness = await database_a.readiness()
                assert readiness["status"] == "degraded"
                assert readiness["dependencies"]["postgres"] == "available"

                # Hold the retry until both subscribers have reattached after Valkey restarts.
                async with owner.begin() as c:
                    await c.execute(
                        text(
                            "UPDATE outbox_events "
                            "SET available_at=now()+interval '60 seconds' WHERE id=:id"
                        ),
                        {"id": second},
                    )
                await docker_control("start", container)
                await wait_until(valkey_healthy)
                await wait_until(subscribers_ready)
                assert (await database_a.readiness())["status"] == "ready"
                async with owner.begin() as c:
                    await c.execute(
                        text("UPDATE outbox_events SET available_at=now() WHERE id=:id"),
                        {"id": second},
                    )
                for ws in (ws_a, ws_b):
                    frame = await receive_event(ws, second, wait_seconds=10.0)
                    assert frame["topic"] == topic and frame["revision"] == 2
                recovered = await wait_until(lambda: published(second))
                assert recovered.attempts >= 1

            # Synchronously overflow the receiving instance's bounded queue before its
            # websocket task can consume it. The client must be told to refetch a snapshot.
            async with connect(url_b) as slow:
                await slow.send(json.dumps({"type": "subscribe", "topics": [topic]}))
                assert await received(slow) == {"type": "subscribed", "topics": [topic]}
                for number in range(HUB_CAPACITY + 1):
                    app_b.state.realtime.fan_out(
                        {
                            "type": "invalidate",
                            "eventId": str(uuid.uuid7()),
                            "topic": topic,
                            "resourceId": str(resource),
                            "revision": number,
                            "occurredAt": "2026-01-01T00:00:00Z",
                        }
                    )
                assert await received(slow) == {"type": "resync_required", "topics": [topic]}

            monkeypatch.setattr(realtime_router, "RECHECK_SECONDS", 0.05)
            async with connect(url_b) as ws:
                await ws.send(json.dumps({"type": "subscribe", "topics": [topic]}))
                assert await received(ws) == {"type": "subscribed", "topics": [topic]}
                async with owner.begin() as c:
                    await c.execute(
                        text(
                            "UPDATE guild_members SET status='INACTIVE' "
                            "WHERE guild_id=:guild AND user_id=:user"
                        ),
                        {"guild": GUILD_A, "user": USER_ID},
                    )
                assert await received(ws) == {"type": "subscribed", "topics": []}


async def test_expired_lease_handoff_fences_previous_instance(owner: AsyncEngine) -> None:
    resource = await seed_recruitment(owner)
    event_id = await commit_event(owner, resource, 1)
    database_a = Database(settings())
    database_b = Database(settings())
    try:
        await database_a.start()
        await database_b.start()
        first = await repository.claim_any(database_a, "instance-a")
        assert len(first) == 1 and first[0]["id"] == event_id
        async with owner.begin() as c:
            await c.execute(
                text("UPDATE outbox_events SET lease_until=now()-interval '1 second' WHERE id=:id"),
                {"id": event_id},
            )
        second = await repository.claim_any(database_b, "instance-b")
        assert len(second) == 1 and second[0]["id"] == event_id
        assert second[0]["lease_token"] != first[0]["lease_token"]
        assert not await repository.ack(database_a, first[0])
        assert not await repository.nack(database_a, first[0], "late_failure")
        assert await repository.ack(database_b, second[0])
        state = await outbox_state(owner, event_id)
        assert state.published_at is not None and state.attempts == 0
    finally:
        await database_a.close()
        await database_b.close()
