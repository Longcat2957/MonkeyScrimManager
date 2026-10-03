"""Authenticated /ws invalidate subscriptions."""

import asyncio
import json
import logging
import uuid
from collections.abc import Mapping, MutableMapping
from typing import Any

from fastapi import APIRouter, WebSocket
from sqlalchemy.exc import SQLAlchemyError
from starlette.responses import JSONResponse
from starlette.websockets import WebSocketDisconnect

from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.json import reject_json_constant, unique_json_fields
from monkeybot_api.repositories import auth as auth_repository
from monkeybot_api.services import realtime as service
from monkeybot_api.services.auth import session as auth_service

logger = logging.getLogger(__name__)
router = APIRouter()
MAX_FRAME_BYTES = 8 * 1024
RECHECK_SECONDS = 60.0
SEND_TIMEOUT_SECONDS = 5.0

ERRORS = {
    "invalid_message": "실시간 요청 형식이 올바르지 않습니다.",
    "invalid_topic": "실시간 topic 형식이 올바르지 않습니다.",
    "unsupported_topic": "이 화면에서는 해당 실시간 topic을 사용할 수 없습니다.",
    "subscription_forbidden": "해당 길드의 실시간 정보를 볼 권한이 없습니다.",
    "subscription_not_found": "실시간으로 볼 수 있는 대상을 찾을 수 없습니다.",
    "subscription_unavailable": "실시간 구독 권한을 확인할 수 없습니다.",
}


def session_cookie(websocket: WebSocket) -> str | None:
    values: list[str] = []
    for header in websocket.headers.getlist("cookie"):
        for pair in header.split(";"):
            name, separator, value = pair.strip().partition("=")
            if separator and name.strip() == "monkeybot_session":
                values.append(value.strip())
    return values[0] if len(values) == 1 else None


async def deny(websocket: WebSocket, error: ApiError, request_id: str) -> None:
    payload: dict[str, Any] = {
        "code": error.code,
        "message": error.message,
        "requestId": request_id,
    }
    if error.details is not None:
        payload["details"] = error.details
    response = JSONResponse({"error": payload}, status_code=error.status)
    response.headers["x-request-id"] = request_id
    response.headers["cache-control"] = "no-store"
    if "websocket.http.response" in websocket.scope.get("extensions", {}):
        await websocket.send_denial_response(response)
    else:
        # ASGI servers without the denial extension cannot send an HTTP JSON handshake error.
        await websocket.close(code=1008)


async def send(websocket: WebSocket, frame: dict[str, Any]) -> bool:
    try:
        async with asyncio.timeout(SEND_TIMEOUT_SECONDS):
            await websocket.send_json(frame)
        return True
    except RuntimeError, OSError, TimeoutError, WebSocketDisconnect:
        return False


async def send_error(websocket: WebSocket, code: str) -> bool:
    return await send(websocket, {"type": "error", "code": code, "message": ERRORS[code]})


async def update_subscriptions(
    websocket: WebSocket,
    runtime: service.RealtimeRuntime,
    user: str,
    subscriptions: set[str],
    frame: dict[str, Any],
) -> bool:
    try:
        topics = service.parse_topics(frame["topics"])
    except service.TopicError as error:
        return await send_error(websocket, error.code)
    if frame["type"] == "subscribe":
        if len(subscriptions | {topic.raw for topic in topics}) > 16:
            return await send_error(websocket, "invalid_topic")
        for topic in topics:
            outcome = await service.authorize_topic(runtime.database, user, topic)
            if outcome != "allowed":
                return await send_error(websocket, outcome)
        subscriptions.update(topic.raw for topic in topics)
    else:
        subscriptions.difference_update(topic.raw for topic in topics)
    return await send(websocket, {"type": "subscribed", "topics": sorted(subscriptions)})


async def handle_message(
    websocket: WebSocket,
    runtime: service.RealtimeRuntime,
    user: str,
    subscriptions: set[str],
    message: Mapping[str, Any],
) -> bool:
    if message["type"] == "websocket.disconnect":
        return False
    raw = message.get("text")
    if raw is None:
        data = message.get("bytes")
        if isinstance(data, bytes) and len(data) > MAX_FRAME_BYTES:
            await websocket.close(code=1009)
            return False
        return await send_error(websocket, "invalid_message")
    if not isinstance(raw, str):
        return await send_error(websocket, "invalid_message")
    if len(raw.encode("utf-8")) > MAX_FRAME_BYTES:
        await websocket.close(code=1009)
        return False
    try:
        frame = json.loads(
            raw, object_pairs_hook=unique_json_fields, parse_constant=reject_json_constant
        )
    except ValueError, UnicodeDecodeError:
        return await send_error(websocket, "invalid_message")
    if (
        not isinstance(frame, dict)
        or set(frame) != {"type", "topics"}
        or not isinstance(frame["type"], str)
        or frame["type"] not in {"subscribe", "unsubscribe"}
    ):
        return await send_error(websocket, "invalid_message")
    if not isinstance(frame["topics"], list) or any(
        not isinstance(value, str) for value in frame["topics"]
    ):
        return await send_error(websocket, "invalid_message")
    return await update_subscriptions(websocket, runtime, user, subscriptions, frame)


async def recheck(
    websocket: WebSocket,
    runtime: service.RealtimeRuntime,
    user: str,
    token_hash: bytes,
    subscriptions: set[str],
) -> bool:
    if not subscriptions:
        return True
    try:
        principal = await auth_repository.find_session(runtime.database, token_hash)
    except Exception as error:
        logger.warning(
            "realtime_session_recheck_failed", extra={"failure_type": type(error).__name__}
        )
        return False
    if principal is None or principal["user_id"] != user:
        return False
    for raw in tuple(subscriptions):
        try:
            topic = service.parse_topic(raw)
        except service.TopicError:
            subscriptions.discard(raw)
            continue
        outcome = await service.authorize_topic(runtime.database, user, topic)
        if outcome == "subscription_unavailable":
            return False
        if outcome != "allowed":
            subscriptions.discard(raw)
    return await send(websocket, {"type": "subscribed", "topics": sorted(subscriptions)})


async def serve(
    websocket: WebSocket,
    runtime: service.RealtimeRuntime,
    user: str,
    token_hash: bytes,
) -> None:
    await websocket.accept()
    queue = runtime.subscribe()
    subscriptions: set[str] = set()
    receiving: asyncio.Task[MutableMapping[str, Any]] | None = None
    eventing: asyncio.Task[dict[str, Any] | None] | None = None
    checking: asyncio.Task[None] | None = None
    try:
        receiving = asyncio.create_task(websocket.receive())
        eventing = asyncio.create_task(queue.get())
        checking = asyncio.create_task(asyncio.sleep(RECHECK_SECONDS))
        while True:
            finished, _ = await asyncio.wait(
                {receiving, eventing, checking}, return_when=asyncio.FIRST_COMPLETED
            )
            if receiving in finished:
                message = receiving.result()
                receiving = asyncio.create_task(websocket.receive())
                if not await handle_message(websocket, runtime, user, subscriptions, message):
                    return
            if eventing in finished:
                event = eventing.result()
                eventing = asyncio.create_task(queue.get())
                if event is None:
                    frame: dict[str, Any] = {"type": "resync_required"}
                    if subscriptions:
                        frame["topics"] = sorted(subscriptions)
                    if not await send(websocket, frame):
                        return
                elif event["topic"] in subscriptions and not await send(websocket, event):
                    return
            if checking in finished:
                checking.result()
                checking = asyncio.create_task(asyncio.sleep(RECHECK_SECONDS))
                if not await recheck(websocket, runtime, user, token_hash, subscriptions):
                    return
    finally:
        runtime.unsubscribe(queue)
        for task in (receiving, eventing, checking):
            if task is not None:
                task.cancel()
        await asyncio.gather(
            *(task for task in (receiving, eventing, checking) if task is not None),
            return_exceptions=True,
        )
        try:
            await websocket.close()
        except RuntimeError, OSError, WebSocketDisconnect:
            pass


@router.websocket("/ws")
async def websocket_endpoint(websocket: WebSocket) -> None:
    request_id = str(uuid.uuid7())
    database = websocket.app.state.database
    settings = websocket.app.state.settings
    try:
        principal = await auth_service.authenticate_session(
            database, settings, session_cookie(websocket), request_id
        )
        if websocket.headers.getlist("origin") != [settings.public_origin]:
            raise ApiError(403, "origin_validation_failed", "요청 출처를 확인할 수 없습니다.")
        runtime: service.RealtimeRuntime | None = getattr(websocket.app.state, "realtime", None)
        if runtime is None or not runtime.available:
            raise ApiError(
                503, "realtime_unavailable", "실시간 편집 서비스를 일시적으로 사용할 수 없습니다."
            )
    except ApiError as error:
        await deny(websocket, error, request_id)
        return
    except SQLAlchemyError as error:
        logger.warning(
            "realtime_handshake_database_failed",
            extra={"request_id": request_id, "failure_type": type(error).__name__},
        )
        await deny(
            websocket,
            ApiError(503, "database_unavailable", "데이터베이스를 일시적으로 사용할 수 없습니다."),
            request_id,
        )
        return
    assert runtime is not None
    logger.info("realtime_connected", extra={"request_id": request_id})
    try:
        await serve(websocket, runtime, str(principal["user_id"]), principal["token_hash"])
    finally:
        logger.info("realtime_disconnected", extra={"request_id": request_id})
