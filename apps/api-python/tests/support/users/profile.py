"""Reusable fixtures and request builders; no test module imports."""

from tests.support.auth.oauth import ORIGIN
from tests.support.guilds.context import (
    USER_ID,
)

PROFILE = f"/api/users/{USER_ID}/profile"


PRIVACY = "/api/me/privacy"


CHAMPIONS = "/api/me/champion-preferences"


DEFAULTS = {"publicProfile": True, "showRiotIds": True, "allowExternalRecordSearch": False}


def csrf(material: dict[str, str]) -> dict[str, str]:
    return {"origin": ORIGIN, "x-csrf-token": material["monkeybot_csrf"]}
