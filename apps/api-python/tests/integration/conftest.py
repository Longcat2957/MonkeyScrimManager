from collections.abc import AsyncIterator

import httpx
import pytest
from pydantic import SecretStr
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncEngine

from monkeybot_api.app import create_app
from monkeybot_api.core.config import Settings
from monkeybot_api.db.database import create_engine
from tests.support.database import isolated_database_url


@pytest.fixture
async def owner(settings: Settings) -> AsyncIterator[AsyncEngine]:
    settings.database_url = SecretStr(isolated_database_url("FASTAPI_TEST_OWNER_URL"))
    engine = create_engine(settings)
    try:
        async with engine.begin() as connection:
            await connection.execute(text("TRUNCATE users, guilds, oauth_login_attempts CASCADE"))
        yield engine
    finally:
        await engine.dispose()


@pytest.fixture
async def runtime_client(owner: AsyncEngine, discord_stub) -> AsyncIterator[httpx.AsyncClient]:
    settings = Settings(
        env="test",
        public_origin="http://localhost:5173",
        database_url=SecretStr(isolated_database_url("FASTAPI_TEST_RUNTIME_URL")),
        discord_application_id="123456789012345678",
        discord_client_secret=SecretStr("integration-test-only"),
        oauth_encryption_key=SecretStr("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
        service_admin_ids="328516947210438656",
        bot_service_token="isolated-bot-service-token-32-characters",
    )
    app = create_app(settings, discord_transport=httpx.MockTransport(discord_stub))
    async with app.router.lifespan_context(app):
        async with httpx.AsyncClient(
            transport=httpx.ASGITransport(app=app), base_url="http://localhost"
        ) as client:
            yield client
