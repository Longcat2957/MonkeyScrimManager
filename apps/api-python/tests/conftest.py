import os
from collections.abc import AsyncIterator

import httpx
import pytest
from pydantic import SecretStr
from sqlalchemy import text
from sqlalchemy.engine import make_url
from sqlalchemy.ext.asyncio import AsyncEngine

from monkeybot_api.app import create_app
from monkeybot_api.config import Settings
from monkeybot_api.database import create_engine


@pytest.fixture(autouse=True)
def isolate_settings(monkeypatch: pytest.MonkeyPatch) -> None:
    for name in os.environ:
        if name.startswith("MONKEYBOT_"):
            monkeypatch.delenv(name)


@pytest.fixture
def settings() -> Settings:
    return Settings(env="test", public_origin="http://localhost:5173")


@pytest.fixture
async def client(settings: Settings) -> AsyncIterator[httpx.AsyncClient]:
    app = create_app(settings)
    async with app.router.lifespan_context(app):
        async with httpx.AsyncClient(
            transport=httpx.ASGITransport(app=app), base_url="http://localhost"
        ) as client:
            yield client


def test_database_url(name: str) -> str:
    value = os.environ.get(name)
    if not value:
        pytest.skip("Run scripts/test-fastapi.sh for isolated PostgreSQL integration tests")
    url = make_url(value)
    # Tests delete fixture data. Never accept a development/production database here.
    if url.database != "monkeybot_fastapi_test" or url.host != "127.0.0.1":
        pytest.fail("FastAPI tests require the isolated loopback monkeybot_fastapi_test database")
    return value


@pytest.fixture
async def owner(settings: Settings) -> AsyncIterator[AsyncEngine]:
    settings.database_url = SecretStr(test_database_url("FASTAPI_TEST_OWNER_URL"))
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
        database_url=SecretStr(test_database_url("FASTAPI_TEST_RUNTIME_URL")),
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


class DiscordStub:
    def __init__(self) -> None:
        self.requests: list[httpx.Request] = []
        self.token_status = 200
        self.user_status = 200
        self.token = {
            "access_token": "test-provider-token",
            "token_type": "Bearer",
            "scope": "identify",
        }
        self.user = {
            "id": "328516947210438656",
            "username": "test-user",
            "global_name": "OAuth 사용자",
            "avatar": "a_0123abcdef",
        }

    def __call__(self, request: httpx.Request) -> httpx.Response:
        self.requests.append(request)
        if request.url.path == "/api/v10/oauth2/token":
            return httpx.Response(self.token_status, json=self.token)
        assert request.url.path == "/api/v10/users/@me"
        assert request.headers["authorization"] == "Bearer test-provider-token"
        return httpx.Response(self.user_status, json=self.user)


@pytest.fixture
def discord_stub() -> DiscordStub:
    return DiscordStub()
