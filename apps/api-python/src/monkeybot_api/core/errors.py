import logging
from typing import Any

from fastapi import Request
from fastapi.exceptions import RequestValidationError
from sqlalchemy.exc import DBAPIError, SQLAlchemyError
from sqlalchemy.exc import TimeoutError as PoolTimeout
from starlette.exceptions import HTTPException
from starlette.responses import JSONResponse

logger = logging.getLogger(__name__)


def hidden_guild() -> ApiError:
    return ApiError(404, "not_found", "요청한 길드를 찾을 수 없습니다.")


MESSAGES = {
    400: ("bad_request", "요청 형식이 올바르지 않습니다."),
    401: ("unauthorized", "인증이 필요합니다."),
    403: ("forbidden", "요청을 수행할 권한이 없습니다."),
    404: ("not_found", "요청한 API 경로를 찾을 수 없습니다."),
    405: ("method_not_allowed", "이 API 경로에서 지원하지 않는 HTTP 메서드입니다."),
    415: ("unsupported_media_type", "지원하지 않는 요청 본문 형식입니다."),
    422: ("unprocessable_entity", "요청 JSON의 필드 또는 값이 API 계약과 일치하지 않습니다."),
    500: ("internal_error", "요청을 처리하는 중 내부 오류가 발생했습니다."),
    503: ("service_unavailable", "서비스를 일시적으로 사용할 수 없습니다."),
}


class ApiError(HTTPException):
    def __init__(
        self, status: int, code: str, message: str, details: dict[str, Any] | None = None
    ) -> None:
        # Preserve the error when FastAPI catches exceptions while reading a JSON body.
        super().__init__(status_code=status, detail=code)
        self.status, self.code, self.message, self.details = status, code, message, details

    @classmethod
    def from_status(cls, status: int) -> ApiError:
        code, message = MESSAGES.get(status, MESSAGES[500])
        return cls(status, code, message)

    def response(self, request: Request) -> JSONResponse:
        request.state.error_code = self.code
        body: dict[str, Any] = {
            "code": self.code,
            "message": self.message,
            "requestId": request.state.request_id,
        }
        if self.details is not None:
            body["details"] = self.details
        return JSONResponse({"error": body}, status_code=self.status)


def success(request: Request, data: dict[str, Any]) -> JSONResponse:
    return JSONResponse({"data": data, "meta": {"requestId": request.state.request_id}})


async def api_error_handler(request: Request, error: Exception) -> JSONResponse:
    assert isinstance(error, ApiError)
    return error.response(request)


async def http_error_handler(request: Request, error: Exception) -> JSONResponse:
    assert isinstance(error, HTTPException)
    response = ApiError.from_status(error.status_code).response(request)
    for name, value in (error.headers or {}).items():
        if name.lower() in {"allow", "www-authenticate", "retry-after"}:
            response.headers[name] = value
    return response


async def validation_error_handler(request: Request, error: Exception) -> JSONResponse:
    assert isinstance(error, RequestValidationError)
    status = 400 if any(item["type"] == "json_invalid" for item in error.errors()) else 422
    return ApiError.from_status(status).response(request)


async def database_error_handler(request: Request, error: Exception) -> JSONResponse:
    assert isinstance(error, SQLAlchemyError)
    code = getattr(error.orig, "sqlstate", "") if isinstance(error, DBAPIError) else ""
    transient = (
        isinstance(error, PoolTimeout)
        or (isinstance(error, DBAPIError) and error.connection_invalidated)
        or (code or "").startswith(("08", "53"))
        or code in {"40001", "40P01", "55P03", "57014", "57P01", "57P02", "57P03"}
    )
    logger.error(
        "database_request_failed",
        extra={
            "request_id": request.state.request_id,
            "failure_type": type(error).__name__,
            "sqlstate": code,
        },
    )
    if transient:
        return ApiError(
            503, "database_unavailable", "데이터베이스를 일시적으로 사용할 수 없습니다."
        ).response(request)
    return ApiError.from_status(500).response(request)
