"""Standalone FastAPI surface preserving the internal Riot service contract."""

import hmac
import json
import logging
from collections.abc import AsyncIterator
from contextlib import asynccontextmanager
from dataclasses import dataclass
from typing import Any

import httpx
from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse
from starlette.exceptions import HTTPException as StarletteHTTPException

from .config import Settings
from .domain import ErrorKind, Platform, RiotError, RiotId
from .provider import HttpRiotProvider, RiotProvider
from .service import RiotService

MAX_BODY_BYTES = 16 * 1024
LOGGER = logging.getLogger(__name__)

_ERRORS: dict[ErrorKind, tuple[int, str]] = {
    ErrorKind.INVALID_REQUEST: (400, "Riot lookup parameters are invalid."),
    ErrorKind.UNAUTHORIZED: (401, "Internal service authentication failed."),
    ErrorKind.NOT_FOUND: (404, "The Riot account was not found."),
    ErrorKind.RATE_LIMITED: (429, "The Riot API rate limit was exceeded."),
    ErrorKind.TIMEOUT: (503, "The Riot API request timed out."),
    ErrorKind.UNAVAILABLE: (503, "The Riot API is temporarily unavailable."),
    ErrorKind.CREDENTIALS_REJECTED: (503, "The Riot API credentials were rejected."),
    ErrorKind.BAD_GATEWAY: (502, "The Riot API returned an unusable response."),
}


@dataclass(slots=True)
class ApiError(Exception):
    status: int
    code: str
    message: str


def _error_response(
    status: int, code: str, message: str, *, retry: int | None = None
) -> JSONResponse:
    headers = {"Retry-After": str(retry)} if retry is not None else None
    return JSONResponse(
        status_code=status,
        content={"error": {"code": code, "message": message}},
        headers=headers,
    )


def create_app(settings: Settings | None = None, provider: RiotProvider | None = None) -> FastAPI:
    settings = settings or Settings()
    http_client: httpx.AsyncClient | None = None
    if provider is None:
        http_client = httpx.AsyncClient(
            timeout=httpx.Timeout(settings.request_timeout_ms / 1000, connect=2.0),
            headers={"User-Agent": "MonkeyBot-Riot/0.1 (+https://monkeybot.lol)"},
            follow_redirects=True,
        )
        provider = HttpRiotProvider(
            http_client, settings.api_key.get_secret_value(), settings.request_timeout_ms
        )
    service = RiotService(provider, settings.match_count, settings.season_start_unix_seconds)

    @asynccontextmanager
    async def lifespan(_: FastAPI) -> AsyncIterator[None]:
        try:
            yield
        finally:
            if http_client is not None:
                await http_client.aclose()

    app = FastAPI(
        lifespan=lifespan,
        docs_url=None,
        redoc_url=None,
        openapi_url=None,
        redirect_slashes=False,
    )

    @app.exception_handler(RiotError)
    async def riot_error_handler(_: Request, error: RiotError) -> JSONResponse:
        status, message = _ERRORS[error.kind]
        return _error_response(status, error.kind.value, message, retry=error.retry_after_seconds)

    @app.exception_handler(ApiError)
    async def api_error_handler(_: Request, error: ApiError) -> JSONResponse:
        return _error_response(error.status, error.code, error.message)

    @app.exception_handler(StarletteHTTPException)
    async def missing_route(_: Request, error: StarletteHTTPException) -> JSONResponse:
        if error.status_code == 404:
            return _error_response(404, "not_found", "Internal Riot endpoint was not found.")
        return _error_response(
            error.status_code, "invalid_request", "Riot lookup parameters are invalid."
        )

    @app.exception_handler(Exception)
    async def unexpected_error(request: Request, error: Exception) -> JSONResponse:
        LOGGER.exception(
            "Riot service request failed", extra={"path": request.url.path}, exc_info=error
        )
        return _error_response(503, "riot_unavailable", "The Riot API is temporarily unavailable.")

    @app.middleware("http")
    async def reject_large_body(request: Request, call_next: Any) -> Any:
        content_length = request.headers.get("content-length")
        if content_length and content_length.isascii() and content_length.isdecimal():
            if int(content_length) > MAX_BODY_BYTES:
                return _error_response(413, "payload_too_large", "Request body is too large.")
        return await call_next(request)

    @app.api_route("/healthz", methods=["GET", "HEAD"])
    async def healthz() -> dict[str, str]:
        return {"status": "ok"}

    @app.post("/internal/riot/accounts/resolve")
    async def resolve_account(request: Request) -> dict[str, Any]:
        payload = await _json_object(request)
        if set(payload) != {"platform", "lookup"} or not isinstance(payload["platform"], str):
            raise RiotError(ErrorKind.INVALID_REQUEST)
        lookup = payload["lookup"]
        if not isinstance(lookup, dict):
            raise RiotError(ErrorKind.INVALID_REQUEST)
        kind = lookup.get("kind")
        if lookup.get("kind") == "riotId" and set(lookup) == {"kind", "gameName", "tagLine"}:
            game_name, tag_line = lookup["gameName"], lookup["tagLine"]
            if not isinstance(game_name, str) or not isinstance(tag_line, str):
                raise RiotError(ErrorKind.INVALID_REQUEST)
        elif kind == "puuid" and set(lookup) == {"kind", "puuid"}:
            puuid = lookup["puuid"]
            if not isinstance(puuid, str):
                raise RiotError(ErrorKind.INVALID_REQUEST)
        else:
            raise RiotError(ErrorKind.INVALID_REQUEST)
        _authorize(request, settings)
        platform = Platform.parse(payload["platform"])
        if kind == "riotId":
            return await service.resolve_by_riot_id(
                RiotId.parse(game_name, tag_line, platform.code)
            )
        return await service.resolve_by_puuid(platform, puuid)

    @app.api_route("/internal/riot/records", methods=["GET", "HEAD"])
    async def records(request: Request) -> dict[str, Any]:
        game_name, tag_line, platform = _riot_id_query(request)
        _authorize(request, settings)
        return await service.records(RiotId.parse(game_name, tag_line, platform))

    @app.api_route("/internal/riot/records/matches", methods=["GET", "HEAD"])
    async def recent_matches(request: Request) -> dict[str, Any]:
        game_name, tag_line, platform = _riot_id_query(request, paginated=True)
        offset = _unsigned_query(request, "start", 0)
        count = _unsigned_query(request, "count", 10)
        _authorize(request, settings)
        return await service.recent_matches_page(
            RiotId.parse(game_name, tag_line, platform), offset, count
        )

    @app.post("/internal/riot/records/refresh")
    async def refresh_records(request: Request) -> dict[str, Any]:
        payload = await _json_object(request)
        game_name, tag_line, platform = _riot_id_body_fields(payload)
        _authorize(request, settings)
        return await service.records(
            RiotId.parse(game_name, tag_line, platform), force_refresh=True
        )

    return app


def _authorize(request: Request, settings: Settings) -> None:
    values = request.headers.getlist("authorization")
    supplied = (
        values[0].removeprefix("Bearer ") if values and values[0].startswith("Bearer ") else ""
    )
    try:
        supplied_bytes = supplied.encode("ascii")
    except UnicodeEncodeError as exc:
        raise RiotError(ErrorKind.UNAUTHORIZED) from exc
    message = b"monkeybot-riot-internal-auth"
    supplied_tag = hmac.digest(supplied_bytes, message, "sha256")
    expected_tag = hmac.digest(
        settings.internal_token.get_secret_value().encode(), message, "sha256"
    )
    if not hmac.compare_digest(supplied_tag, expected_tag):
        raise RiotError(ErrorKind.UNAUTHORIZED)


async def _json_object(request: Request) -> dict[str, Any]:
    content_type = request.headers.get("content-type", "").split(";", 1)[0].lower()
    if content_type != "application/json":
        raise ApiError(415, "unsupported_media_type", "Content type must be application/json.")
    body = bytearray()
    async for chunk in request.stream():
        body.extend(chunk)
        if len(body) > MAX_BODY_BYTES:
            raise ApiError(413, "payload_too_large", "Request body is too large.")
    try:
        payload = json.loads(body)
    except (ValueError, UnicodeDecodeError) as exc:
        raise RiotError(ErrorKind.INVALID_REQUEST) from exc
    if not isinstance(payload, dict):
        raise RiotError(ErrorKind.INVALID_REQUEST)
    return payload


def _riot_id_body_fields(payload: dict[str, Any]) -> tuple[str, str, str]:
    if set(payload) != {"gameName", "tagLine", "platform"}:
        raise RiotError(ErrorKind.INVALID_REQUEST)
    game_name, tag_line, platform = payload["gameName"], payload["tagLine"], payload["platform"]
    if not all(isinstance(value, str) for value in (game_name, tag_line, platform)):
        raise RiotError(ErrorKind.INVALID_REQUEST)
    return game_name, tag_line, platform


def _riot_id_query(request: Request, *, paginated: bool = False) -> tuple[str, str, str]:
    allowed = {"gameName", "tagLine", "platform"}
    if paginated:
        allowed.update({"start", "count"})
    params = request.query_params
    if set(params) - allowed or any(len(params.getlist(key)) != 1 for key in params):
        raise RiotError(ErrorKind.INVALID_REQUEST)
    try:
        return params["gameName"], params["tagLine"], params["platform"]
    except KeyError as exc:
        raise RiotError(ErrorKind.INVALID_REQUEST) from exc


def _unsigned_query(request: Request, key: str, default: int) -> int:
    value = request.query_params.get(key)
    if value is None:
        return default
    if not value.isascii() or not value.isdecimal():
        raise RiotError(ErrorKind.INVALID_REQUEST)
    result = int(value)
    if result > 2**64 - 1:
        raise RiotError(ErrorKind.INVALID_REQUEST)
    return result
