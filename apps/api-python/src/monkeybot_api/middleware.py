import asyncio
import logging
import time
import uuid

from starlette.requests import Request
from starlette.types import ASGIApp, Message, Receive, Scope, Send

from monkeybot_api.config import Settings
from monkeybot_api.errors import ApiError

logger = logging.getLogger(__name__)


class RequestPipeline:
    def __init__(self, app: ASGIApp, settings: Settings) -> None:
        self.app, self.settings = app, settings

    async def __call__(self, scope: Scope, receive: Receive, send: Send) -> None:
        if scope["type"] != "http":
            await self.app(scope, receive, send)
            return
        started = time.monotonic()
        request = Request(scope)
        request.state.request_id = str(uuid.uuid7())
        request.state.error_code = ""
        status, response_started, received = 500, False, 0

        def too_large() -> ApiError:
            return ApiError(
                413,
                "payload_too_large",
                "요청 본문이 허용된 크기를 초과했습니다.",
                {"maxBodyBytes": self.settings.max_body_bytes},
            )

        async def limited_receive() -> Message:
            nonlocal received
            message = await receive()
            if message["type"] == "http.request":
                received += len(message.get("body", b""))
                if received > self.settings.max_body_bytes:
                    raise too_large()
            return message

        async def response_send(message: Message) -> None:
            nonlocal status, response_started
            if message["type"] == "http.response.start":
                status, response_started = message["status"], True
                security = {
                    b"x-request-id": request.state.request_id.encode(),
                    b"cache-control": b"no-store",
                    b"x-content-type-options": b"nosniff",
                    b"x-frame-options": b"DENY",
                    b"referrer-policy": b"no-referrer",
                }
                message["headers"] = [
                    (name, value)
                    for name, value in message.get("headers", [])
                    if name.lower() not in security
                ] + list(security.items())
            await send(message)

        try:
            async with asyncio.timeout(self.settings.request_timeout_ms / 1000):
                content_length = request.headers.get("content-length")
                if content_length is not None:
                    if not content_length.isascii() or not content_length.isdecimal():
                        raise ApiError.from_status(400)
                    if int(content_length) > self.settings.max_body_bytes:
                        raise too_large()
                await self.app(scope, limited_receive, response_send)
        except Exception as error:
            if response_started:
                raise
            if isinstance(error, ApiError):
                failure = error
            elif isinstance(error, TimeoutError):
                failure = ApiError(503, "request_timeout", "요청 처리 시간이 초과되었습니다.")
            else:
                logger.error(
                    "request_failed",
                    extra={
                        "request_id": request.state.request_id,
                        "failure_type": type(error).__name__,
                    },
                    exc_info=True,
                )
                failure = ApiError.from_status(500)
            await failure.response(request)(scope, receive, response_send)
        finally:
            route = scope.get("route")
            logger.info(
                "request_completed",
                extra={
                    "request_id": request.state.request_id,
                    "trace_id": request.state.request_id,
                    "method": scope["method"],
                    "route": getattr(route, "path", "<unmatched>"),
                    "status": status,
                    "latency_ms": round((time.monotonic() - started) * 1000, 2),
                    "error_code": request.state.error_code,
                },
            )
