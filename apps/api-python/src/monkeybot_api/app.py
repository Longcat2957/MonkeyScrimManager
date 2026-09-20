import asyncio
from collections.abc import AsyncIterator
from contextlib import asynccontextmanager

import httpx
from fastapi import FastAPI, Request
from fastapi.exceptions import RequestValidationError
from sqlalchemy.exc import SQLAlchemyError
from starlette.exceptions import HTTPException
from starlette.responses import JSONResponse

from monkeybot_api import (
    auction_routes,
    context,
    guild_admin,
    guild_seasons,
    notification_delivery,
    oauth,
    profile,
    recruitment_create,
    recruitment_delivery,
    recruitment_lifecycle,
    recruitment_members,
    series_routes,
    tournament_routes,
    workflow,
)
from monkeybot_api.config import Settings
from monkeybot_api.database import Database
from monkeybot_api.discord import DiscordClient
from monkeybot_api.errors import (
    ApiError,
    api_error_handler,
    database_error_handler,
    http_error_handler,
    success,
    validation_error_handler,
)
from monkeybot_api.middleware import RequestPipeline
from monkeybot_api.session import current_user


def create_app(
    settings: Settings | None = None, *, discord_transport: httpx.AsyncBaseTransport | None = None
) -> FastAPI:
    settings = settings or Settings()
    database = Database(settings)
    discord = DiscordClient(settings, discord_transport)

    @asynccontextmanager
    async def lifespan(app: FastAPI) -> AsyncIterator[None]:
        try:
            await database.start()
            yield
        finally:
            async with asyncio.timeout(settings.shutdown_timeout_ms / 1000):
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
    app.state.database = database
    app.state.discord = discord
    app.add_middleware(RequestPipeline, settings=settings)

    # Axum's GET router also supports HEAD. Preserve this deliberately.
    @app.api_route("/healthz", methods=["GET", "HEAD"])
    async def healthz(request: Request) -> JSONResponse:
        return success(request, {"status": "ok"})

    @app.api_route("/readyz", methods=["GET", "HEAD"])
    async def readyz(request: Request) -> JSONResponse:
        report = await database.readiness()
        if report["status"] == "not_ready":
            raise ApiError(
                503, "service_not_ready", "서비스가 요청을 받을 준비가 되지 않았습니다.", report
            )
        return success(request, report)

    @app.api_route("/api/me", methods=["GET", "HEAD"])
    async def me(request: Request) -> JSONResponse:
        return success(request, await current_user(request, database, settings))

    app.include_router(oauth.router(settings, database, discord))
    app.include_router(context.router(settings, database))
    app.include_router(profile.router(settings, database))
    app.include_router(workflow.router(settings, database))
    app.include_router(recruitment_create.router(settings, database))
    app.include_router(recruitment_members.router(settings, database))
    app.include_router(recruitment_lifecycle.router(settings, database))
    app.include_router(recruitment_delivery.router(settings, database))
    app.include_router(series_routes.router(settings, database))
    app.include_router(notification_delivery.router(settings, database))
    app.include_router(guild_admin.router(settings, database))
    app.include_router(guild_seasons.router(settings, database))
    app.include_router(auction_routes.router(settings, database))
    app.include_router(tournament_routes.router(settings, database))
    return app
