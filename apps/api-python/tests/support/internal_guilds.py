"""Small request builders for Bot guild setup and registration tests."""

from datetime import UTC, datetime

from monkeybot_api.schemas.internal_guilds import CanonicalRiotAccount
from tests.support.guilds.context import GUILD_A, GUILD_B, ROLE_A, USER_ID
from tests.support.recruitments.creation import BOT

SETUP_A = f"/internal/guilds/{GUILD_A}/setup"
REGISTER_A = f"/internal/guilds/{GUILD_A}/registrations"
REGISTER_B = f"/internal/guilds/{GUILD_B}/registrations"


def actor(identifier: str = USER_ID, name: str = "Discord user") -> dict:
    return {
        "id": identifier,
        "globalDisplayName": name,
        "displayName": name,
        "avatarHash": None,
        "roleIds": [ROLE_A],
    }


def setup_body(identifier: str = USER_ID, name: str = "Guild A") -> dict:
    return {
        "actor": actor(identifier),
        "guildName": name,
        "guildIconHash": None,
        "operatorRoleId": ROLE_A,
        "recruitmentChannelId": "8001",
        "entryChannelId": None,
        "resultChannelId": None,
        "logChannelId": None,
        "seasonName": "첫 시즌",
    }


def registration_body(identifier: str = USER_ID) -> dict:
    return {
        "actor": actor(identifier),
        "gameName": " Hide on bush ",
        "tagLine": " KR1 ",
        "platform": "kr",
    }


def bot_headers(key: str = "123456789012345679") -> dict[str, str]:
    return {"authorization": f"Bearer {BOT}", "idempotency-key": key}


def resolved(puuid: str = "riot-puuid-1") -> CanonicalRiotAccount:
    return CanonicalRiotAccount(
        puuid=puuid,
        platform="KR",
        routing_region="ASIA",
        game_name="Hide on bush",
        tag_line="KR1",
        profile_icon_id=10,
        last_synced_at=datetime(2026, 1, 1, tzinfo=UTC),
    )
