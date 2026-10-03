"""Isolated, flag-enabled API client for development-only routes."""

from collections.abc import AsyncIterator

import httpx
import pytest
from pydantic import SecretStr
from sqlalchemy.ext.asyncio import AsyncEngine

from monkeybot_api.app import create_app
from monkeybot_api.core.config import Settings
from tests.support.database import isolated_database_url
from tests.support.guilds.context import USER_ID


@pytest.fixture
async def dummy_enabled_client(
    owner: AsyncEngine, discord_stub
) -> AsyncIterator[httpx.AsyncClient]:
    settings = Settings(
        env="test",
        public_origin="http://localhost:5173",
        database_url=SecretStr(isolated_database_url("FASTAPI_TEST_RUNTIME_URL")),
        discord_application_id="123456789012345678",
        discord_client_secret=SecretStr("integration-test-only"),
        oauth_encryption_key=SecretStr("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
        service_admin_ids=USER_ID,
        dummy_users_enabled=True,
    )
    app = create_app(settings, discord_transport=httpx.MockTransport(discord_stub))
    async with app.router.lifespan_context(app):
        async with httpx.AsyncClient(
            transport=httpx.ASGITransport(app=app), base_url="http://localhost"
        ) as client:
            yield client
