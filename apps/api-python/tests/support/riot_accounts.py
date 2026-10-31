"""Session and canonical account builders for self-service Riot account tests."""

from datetime import UTC, datetime

import httpx
from sqlalchemy.ext.asyncio import AsyncEngine

from monkeybot_api.schemas.internal_guilds import CanonicalRiotAccount
from tests.support.auth.oauth import cookies, finish_login, start_login
from tests.support.users.profile import csrf

PATH = "/api/me/riot-accounts"


async def authenticate(owner: AsyncEngine, client: httpx.AsyncClient) -> dict[str, str]:
    del owner  # The owner fixture clears the isolated database before OAuth login.
    state, _ = await start_login(client)
    response = await finish_login(client, state)
    assert response.status_code == 303, response.text
    return csrf(cookies(response))


def account(
    puuid: str = "puuid-one", name: str = "Canonical One", icon: int = 12
) -> CanonicalRiotAccount:
    return CanonicalRiotAccount(
        puuid=puuid,
        platform="KR",
        routing_region="ASIA",
        game_name=name,
        tag_line="KR1",
        profile_icon_id=icon,
        last_synced_at=datetime(2026, 1, 1, tzinfo=UTC),
    )
