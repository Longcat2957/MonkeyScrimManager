import asyncio
import uuid

import httpx
import pytest
from fastapi import Request
from pydantic import BaseModel, ConfigDict, ValidationError

from monkeybot_api.app import create_app
from monkeybot_api.config import Settings


def assert_failure(response: httpx.Response, status: int, code: str, message: str) -> None:
    assert response.status_code == status
    error = response.json()["error"]
    assert error["code"] == code
    assert error["message"] == message
    assert error["requestId"] == response.headers["x-request-id"]
    assert uuid.UUID(error["requestId"]).version == 7


async def test_liveness_has_no_dependency_requirement(client: httpx.AsyncClient) -> None:
    response = await client.get("/healthz", headers={"X-Request-ID": "untrusted"})
    assert response.status_code == 200
    assert response.json()["data"] == {"status": "ok"}
    request_id = response.headers["x-request-id"]
    assert uuid.UUID(request_id).version == 7
    assert response.json()["meta"]["requestId"] == request_id
    assert response.headers["cache-control"] == "no-store"
    assert response.headers["x-content-type-options"] == "nosniff"
    assert response.headers["x-frame-options"] == "DENY"
    assert response.headers["referrer-policy"] == "no-referrer"
    head = await client.head("/healthz")
    assert head.status_code == 200 and head.content == b""
    assert head.headers["x-request-id"] != request_id


async def test_unconfigured_readiness_is_not_liveness(client: httpx.AsyncClient) -> None:
    response = await client.get("/readyz")
    assert_failure(
        response, 503, "service_not_ready", "서비스가 요청을 받을 준비가 되지 않았습니다."
    )
    assert response.json()["error"]["details"] == {
        "status": "not_ready",
        "acceptingRequests": True,
        "dependencies": dict.fromkeys(["postgres", "migration", "valkey"], "not_configured"),
    }


async def test_unconfigured_auth_returns_concrete_reason(client: httpx.AsyncClient) -> None:
    assert_failure(
        await client.get("/api/me"),
        503,
        "authentication_unavailable",
        "인증 서비스를 일시적으로 사용할 수 없습니다.",
    )


@pytest.mark.parametrize("path", ["/missing", "/api/me/", "/docs", "/openapi.json"])
async def test_no_implicit_redirect_or_new_public_routes(
    client: httpx.AsyncClient, path: str
) -> None:
    assert_failure(await client.get(path), 404, "not_found", "요청한 API 경로를 찾을 수 없습니다.")


async def test_method_error_preserves_allow_header(client: httpx.AsyncClient) -> None:
    response = await client.post("/api/me")
    assert_failure(
        response, 405, "method_not_allowed", "이 API 경로에서 지원하지 않는 HTTP 메서드입니다."
    )
    assert {method.strip() for method in response.headers["allow"].split(",")} == {"GET", "HEAD"}


class Input(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)
    count: int


async def test_body_validation_timeout_and_exception_boundaries(settings: Settings) -> None:
    settings.max_body_bytes = 32
    settings.request_timeout_ms = 50
    app = create_app(settings)

    @app.post("/input")
    async def accept(payload: Input) -> dict[str, int]:
        return {"count": payload.count}

    @app.post("/stream")
    async def stream(request: Request) -> dict[str, int]:
        return {"size": len(await request.body())}

    @app.get("/slow")
    async def slow() -> None:
        await asyncio.sleep(1)

    @app.get("/crash")
    async def crash() -> None:
        raise RuntimeError("private diagnostic not for clients")

    async with app.router.lifespan_context(app):
        async with httpx.AsyncClient(
            transport=httpx.ASGITransport(app=app), base_url="http://localhost"
        ) as client:
            malformed = await client.post(
                "/input", content="{", headers={"content-type": "application/json"}
            )
            assert_failure(malformed, 400, "bad_request", "요청 형식이 올바르지 않습니다.")
            invalid = await client.post("/input", json={"count": "1"})
            assert_failure(
                invalid,
                422,
                "unprocessable_entity",
                "요청 JSON의 필드 또는 값이 API 계약과 일치하지 않습니다.",
            )
            oversized = await client.post("/input", content="x" * 33)
            assert_failure(
                oversized, 413, "payload_too_large", "요청 본문이 허용된 크기를 초과했습니다."
            )
            assert oversized.json()["error"]["details"] == {"maxBodyBytes": 32}

            async def chunks():
                yield b"x" * 20
                yield b"y" * 20

            chunked = await client.post("/stream", content=chunks())
            assert_failure(
                chunked, 413, "payload_too_large", "요청 본문이 허용된 크기를 초과했습니다."
            )
            chunked_json = await client.post(
                "/input", content=chunks(), headers={"content-type": "application/json"}
            )
            assert_failure(
                chunked_json, 413, "payload_too_large", "요청 본문이 허용된 크기를 초과했습니다."
            )
            assert_failure(
                await client.get("/slow"),
                503,
                "request_timeout",
                "요청 처리 시간이 초과되었습니다.",
            )
            failed = await client.get("/crash")
            assert_failure(
                failed, 500, "internal_error", "요청을 처리하는 중 내부 오류가 발생했습니다."
            )
            assert "private diagnostic" not in failed.text
            assert (await client.get("/healthz")).status_code == 200
    assert app.state.database.accepting_requests is False


def test_configuration_rejects_insecure_production_and_partial_auth() -> None:
    with pytest.raises(ValidationError, match="HTTPS"):
        Settings(env="production", public_origin="http://example.com")
    with pytest.raises(ValidationError, match="required"):
        Settings(env="test", public_origin="http://localhost", discord_application_id="123")


def test_invalid_configuration_does_not_stringify_credentials() -> None:
    with pytest.raises(ValidationError) as caught:
        Settings(
            env="test",
            public_origin="http://localhost",
            database_url="https://user:private-password@localhost/db",
        )
    assert "private-password" not in str(caught.value)
