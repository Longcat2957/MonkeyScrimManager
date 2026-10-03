"""Bot notification delivery HTTP boundaries."""

from fastapi import APIRouter, Request
from starlette.responses import JSONResponse

from monkeybot_api.api.authentication import require_bot
from monkeybot_api.api.body import read_json
from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.core.errors import success
from monkeybot_api.schemas.notifications import Ack, Claim, Heartbeat, Nack
from monkeybot_api.services.notifications import delivery

router = APIRouter()


@router.post("/internal/heartbeat")
async def record_heartbeat(
    request: Request, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    require_bot(request, settings)
    body = await read_json(request, Heartbeat)
    async with database.connection(transaction=True) as c:
        result = await delivery.heartbeat(c, body)
    return success(request, result)


@router.post("/internal/notifications/claim")
async def claim_notifications(
    request: Request, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    require_bot(request, settings)
    body = await read_json(request, Claim)
    async with database.connection(transaction=True) as c:
        result = await delivery.claim(c, body)
    return success(request, result)


@router.post("/internal/notifications/nack")
async def nack_notification(
    request: Request, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    require_bot(request, settings)
    body = await read_json(request, Nack)
    async with database.connection(transaction=True) as c:
        result = await delivery.nack(c, body)
    return success(request, result)


@router.post("/internal/notifications/ack")
async def acknowledge(
    request: Request, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    require_bot(request, settings)
    body = await read_json(request, Ack)
    async with database.connection(transaction=True) as c:
        result = await delivery.ack(c, body, None)
    return success(request, result)


@router.post("/internal/notifications/recruitment-message/ack")
async def acknowledge_recruitment(
    request: Request, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    require_bot(request, settings)
    body = await read_json(request, Ack)
    async with database.connection(transaction=True) as c:
        result = await delivery.ack(c, body, "RECRUITMENT")
    return success(request, result)


@router.post("/internal/notifications/auction-recruitment-message/ack")
async def acknowledge_auction(
    request: Request, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    require_bot(request, settings)
    body = await read_json(request, Ack)
    async with database.connection(transaction=True) as c:
        result = await delivery.ack(c, body, "AUCTION_RECRUITMENT")
    return success(request, result)
