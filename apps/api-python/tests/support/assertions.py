"""Reusable fixtures and request builders; no test module imports."""

import uuid

import httpx


def assert_failure(response: httpx.Response, status: int, code: str, message: str) -> None:
    assert response.status_code == status
    error = response.json()["error"]
    assert error["code"] == code
    assert error["message"] == message
    assert error["requestId"] == response.headers["x-request-id"]
    assert uuid.UUID(error["requestId"]).version == 7
