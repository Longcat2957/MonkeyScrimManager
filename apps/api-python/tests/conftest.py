import os
from collections.abc import AsyncIterator

import httpx
import pytest

from monkeybot_api.app import create_app
from monkeybot_api.core.config import Settings
from tests.support.auth.discord import DiscordStub


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


@pytest.fixture
def discord_stub() -> DiscordStub:
    return DiscordStub()
