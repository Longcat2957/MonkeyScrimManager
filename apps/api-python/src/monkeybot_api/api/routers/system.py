"""Liveness and dependency readiness endpoints."""

from fastapi import APIRouter, Request
from starlette.responses import JSONResponse

from monkeybot_api.api.dependencies import DatabaseDep
from monkeybot_api.core.errors import ApiError, success

router = APIRouter()


# Axum's GET router also supports HEAD. Preserve this deliberately.
@router.api_route("/healthz", methods=["GET", "HEAD"])
async def healthz(request: Request) -> JSONResponse:
    return success(request, {"status": "ok"})


@router.api_route("/readyz", methods=["GET", "HEAD"])
async def readyz(request: Request, database: DatabaseDep) -> JSONResponse:
    report = await database.readiness()
    if report["status"] == "not_ready":
        raise ApiError(
            503, "service_not_ready", "서비스가 요청을 받을 준비가 되지 않았습니다.", report
        )
    return success(request, report)
