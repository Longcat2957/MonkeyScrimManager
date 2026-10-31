"""Application assembly and lifecycle for the FastAPI migration candidate."""

import asyncio
from collections.abc import AsyncIterator
from contextlib import asynccontextmanager

import httpx
from fastapi import FastAPI
from fastapi.exceptions import RequestValidationError
from sqlalchemy.exc import SQLAlchemyError
from starlette.exceptions import HTTPException

from monkeybot_api.api.middleware import RequestPipeline
from monkeybot_api.api.router import router
from monkeybot_api.api.routers import dev
from monkeybot_api.core.config import Settings
from monkeybot_api.core.errors import (
    ApiError,
    api_error_handler,
    database_error_handler,
    http_error_handler,
    validation_error_handler,
)
from monkeybot_api.core.security import AuthCrypto, OAuthStartLimiter
from monkeybot_api.db.database import Database
from monkeybot_api.integrations.discord import DiscordClient
from monkeybot_api.services.guilds.transfer_retention import run_transfer_retention
from monkeybot_api.services.realtime import RealtimeRuntime


def create_app(
    settings: Settings | None = None, *, discord_transport: httpx.AsyncBaseTransport | None = None
) -> FastAPI:
    settings = settings or Settings()
    database = Database(settings)
    discord = DiscordClient(settings, discord_transport)

    @asynccontextmanager
    async def lifespan(app: FastAPI) -> AsyncIterator[None]:
        realtime: RealtimeRuntime | None = None
        retention: asyncio.Task[None] | None = None
        try:
            await database.start()
            realtime = RealtimeRuntime(database, settings)
            await realtime.start()
            app.state.realtime = realtime
            retention = asyncio.create_task(
                run_transfer_retention(database), name="guild-transfer-retention"
            )
            yield
        finally:
            async with asyncio.timeout(settings.shutdown_timeout_ms / 1000):
                try:
                    if retention is not None:
                        retention.cancel()
                        await asyncio.gather(retention, return_exceptions=True)
                finally:
                    try:
                        if realtime is not None:
                            await realtime.close()
                    finally:
                        try:
                            await discord.http.aclose()
                        finally:
                            await database.close()

    app = FastAPI(
        title="MonkeyBot API migration candidate",
        lifespan=lifespan,
        docs_url=None,
        redoc_url=None,
        openapi_url=None,
        redirect_slashes=False,
        exception_handlers={
            ApiError: api_error_handler,
            HTTPException: http_error_handler,
            RequestValidationError: validation_error_handler,
            SQLAlchemyError: database_error_handler,
        },
    )
    app.state.settings = settings
    app.state.database = database
    app.state.discord = discord
    app.state.oauth_crypto = (
        AuthCrypto(settings.oauth_encryption_key.get_secret_value())
        if settings.oauth_encryption_key
        else None
    )
    app.state.oauth_start_limiter = OAuthStartLimiter()
    app.add_middleware(RequestPipeline, settings=settings)
    app.include_router(router)
    if settings.dummy_users_enabled:
        app.include_router(dev.router)
    return app
