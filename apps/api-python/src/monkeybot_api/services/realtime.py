"""Bounded websocket fanout and lease-based realtime outbox relay."""

import asyncio
import json
import logging
import uuid
from dataclasses import dataclass
from datetime import datetime
from typing import Any

from monkeybot_api.core.config import Settings, is_snowflake
from monkeybot_api.core.json import reject_json_constant, unique_json_fields
from monkeybot_api.core.time import timestamp
from monkeybot_api.db.database import Database
from monkeybot_api.repositories import realtime as repository

logger = logging.getLogger(__name__)
HUB_CAPACITY = 256
OUTBOX_IDLE_POLL = 0.25
VALKEY_RECONNECT = 1.0
VALKEY_TIMEOUT = 3.0


@dataclass(frozen=True, slots=True)
class Topic:
    raw: str
    guild: str
    kind: str
    resource: uuid.UUID


class TopicError(ValueError):
    def __init__(self, code: str) -> None:
        super().__init__(code)
        self.code = code


def parse_topic(raw: str) -> Topic:
    parts = raw.split(":")
    if len(parts) != 4 or parts[0] != "guild" or not is_snowflake(parts[1]):
        raise TopicError("invalid_topic")
    kind = parts[2]
    if kind not in {
        "recruitment",
        "recruitment-entry-draft",
        "series",
        "auction-recruitment",
        "auction-tournament",
        "auction-match",
    }:
        raise TopicError("unsupported_topic")
    try:
        resource = uuid.UUID(parts[3])
    except ValueError:
        raise TopicError("invalid_topic") from None
    if raw != f"guild:{parts[1]}:{kind}:{resource}":
        raise TopicError("invalid_topic")
    return Topic(raw, parts[1], kind, resource)


def parse_topics(value: object) -> list[Topic]:
    if not isinstance(value, list) or not 1 <= len(value) <= 16:
        raise TopicError("invalid_topic")
    seen: set[str] = set()
    parsed: list[Topic] = []
    for raw in value:
        if not isinstance(raw, str):
            raise TopicError("invalid_topic")
        topic = parse_topic(raw)
        if topic.raw in seen:
            raise TopicError("invalid_topic")
        seen.add(topic.raw)
        parsed.append(topic)
    return parsed


async def authorize_topic(database: Database, user: str, topic: Topic) -> str:
    if topic.kind == "auction-match":
        return "unsupported_topic"
    try:
        outcome = await repository.authorize_topic(
            database, user, topic.guild, topic.kind, topic.resource
        )
    except Exception as error:
        logger.warning(
            "realtime_topic_authorization_failed",
            extra={"failure_type": type(error).__name__},
        )
        return "subscription_unavailable"
    return {
        "allowed": "allowed",
        "forbidden": "subscription_forbidden",
        "not_found": "subscription_not_found",
    }[outcome]


def event_from_claim(row: dict[str, Any]) -> dict[str, Any]:
    topic = parse_topic(row["topic"])
    if topic.guild != row["guild_id"] or str(topic.resource) != str(row["aggregate_id"]):
        raise ValueError("outbox topic identity mismatch")
    revision = row["aggregate_revision"]
    if not isinstance(revision, int) or revision < 0:
        raise ValueError("invalid outbox revision")
    occurred_at = row["occurred_at"]
    if not isinstance(occurred_at, datetime) or occurred_at.tzinfo is None:
        raise ValueError("invalid outbox occurrence timestamp")
    return {
        "type": "invalidate",
        "eventId": str(row["id"]),
        "topic": topic.raw,
        "resourceId": str(row["aggregate_id"]),
        "revision": revision,
        "occurredAt": timestamp(occurred_at),
    }


def event_from_valkey(payload: bytes | str) -> dict[str, Any]:
    value = json.loads(
        payload, object_pairs_hook=unique_json_fields, parse_constant=reject_json_constant
    )
    if (
        not isinstance(value, dict)
        or set(value)
        != {
            "type",
            "eventId",
            "topic",
            "resourceId",
            "revision",
            "occurredAt",
        }
        or value["type"] != "invalidate"
    ):
        raise ValueError("invalid realtime event")
    if not isinstance(value["topic"], str):
        raise ValueError("invalid realtime event topic")
    topic = parse_topic(value["topic"])
    if value["resourceId"] != str(topic.resource):
        raise ValueError("invalid realtime event resource")
    if (
        not isinstance(value["revision"], int)
        or isinstance(value["revision"], bool)
        or value["revision"] < 0
    ):
        raise ValueError("invalid realtime event revision")
    if not isinstance(value["eventId"], str) or not isinstance(value["occurredAt"], str):
        raise ValueError("invalid realtime event metadata")
    uuid.UUID(value["eventId"])
    occurred = datetime.fromisoformat(value["occurredAt"].replace("Z", "+00:00"))
    if occurred.tzinfo is None:
        raise ValueError("invalid realtime event timestamp")
    return value


class RealtimeRuntime:
    def __init__(self, database: Database, settings: Settings) -> None:
        self.database = database
        self.settings = settings
        self.available = database.engine is not None and (
            database.valkey is not None or settings.env in {"local", "test"}
        )
        self._queues: set[asyncio.Queue[dict[str, Any] | None]] = set()
        self._tasks: list[asyncio.Task[None]] = []
        self._stop = asyncio.Event()
        self._owner = f"realtime-{uuid.uuid7()}"
        self._namespace = f"monkeybot:{settings.env}:pubsub"

    async def start(self) -> None:
        if not self.available or self._tasks:
            return
        self._stop.clear()
        self._tasks.append(asyncio.create_task(self._relay(), name="realtime-outbox-relay"))
        if self.database.valkey is not None:
            self._tasks.append(
                asyncio.create_task(self._subscriber(), name="realtime-valkey-subscriber")
            )

    async def close(self) -> None:
        self._stop.set()
        for task in self._tasks:
            task.cancel()
        if self._tasks:
            await asyncio.gather(*self._tasks, return_exceptions=True)
        self._tasks.clear()
        for queue in tuple(self._queues):
            self.unsubscribe(queue)

    def subscribe(self) -> asyncio.Queue[dict[str, Any] | None]:
        queue: asyncio.Queue[dict[str, Any] | None] = asyncio.Queue(maxsize=HUB_CAPACITY)
        self._queues.add(queue)
        return queue

    def unsubscribe(self, queue: asyncio.Queue[dict[str, Any] | None]) -> None:
        self._queues.discard(queue)

    def fan_out(self, event: dict[str, Any]) -> None:
        for queue in tuple(self._queues):
            if queue.full():
                while not queue.empty():
                    queue.get_nowait()
                queue.put_nowait(None)
            if not queue.full():
                queue.put_nowait(event)

    async def publish(self, event: dict[str, Any]) -> None:
        valkey = self.database.valkey
        if valkey is not None:
            channel = f"{self._namespace}:{event['topic']}"
            async with asyncio.timeout(VALKEY_TIMEOUT):
                await valkey.publish(channel, json.dumps(event, ensure_ascii=False))
        self.fan_out(event)

    async def _relay(self) -> None:
        while not self._stop.is_set():
            try:
                claimed = await repository.claim_any(self.database, self._owner)
            except Exception as error:
                logger.warning(
                    "realtime_outbox_claim_failed", extra={"failure_type": type(error).__name__}
                )
                await self._wait(OUTBOX_IDLE_POLL)
                continue
            if not claimed:
                await self._wait(OUTBOX_IDLE_POLL)
                continue
            for row in claimed:
                try:
                    event = event_from_claim(row)
                except (TopicError, ValueError, TypeError, KeyError) as error:
                    logger.error(
                        "realtime_outbox_event_invalid",
                        extra={"event_id": str(row["id"]), "failure_type": type(error).__name__},
                    )
                    await self._nack(row, "invalid_realtime_event")
                    continue
                try:
                    await self.publish(event)
                except Exception as error:
                    logger.warning(
                        "realtime_outbox_publish_failed",
                        extra={"event_id": str(row["id"]), "failure_type": type(error).__name__},
                    )
                    await self._nack(row, "realtime_publish_failed")
                    continue
                try:
                    await repository.ack(self.database, row)
                except Exception as error:
                    logger.warning(
                        "realtime_outbox_ack_failed",
                        extra={"event_id": str(row["id"]), "failure_type": type(error).__name__},
                    )

    async def _nack(self, row: dict[str, Any], reason: str) -> None:
        try:
            await repository.nack(self.database, row, reason)
        except Exception as error:
            logger.warning(
                "realtime_outbox_nack_failed",
                extra={"event_id": str(row["id"]), "failure_type": type(error).__name__},
            )

    async def _subscriber(self) -> None:
        valkey = self.database.valkey
        assert valkey is not None
        while not self._stop.is_set():
            try:
                async with valkey.pubsub() as pubsub:
                    await pubsub.psubscribe(f"{self._namespace}:*")
                    while not self._stop.is_set():
                        message = await pubsub.get_message(
                            ignore_subscribe_messages=True, timeout=1.0
                        )
                        if message is None:
                            continue
                        try:
                            event = event_from_valkey(message["data"])
                        except (ValueError, TypeError, KeyError, TopicError) as error:
                            logger.warning(
                                "realtime_valkey_event_invalid",
                                extra={"failure_type": type(error).__name__},
                            )
                            continue
                        self.fan_out(event)
            except Exception as error:
                logger.warning(
                    "realtime_valkey_subscriber_failed",
                    extra={"failure_type": type(error).__name__},
                )
                await self._wait(VALKEY_RECONNECT)

    async def _wait(self, seconds: float) -> None:
        try:
            await asyncio.wait_for(self._stop.wait(), timeout=seconds)
        except TimeoutError:
            pass
