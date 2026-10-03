"""Request-scoped access to resources owned by the application lifespan."""

from typing import Annotated, cast

from fastapi import Depends, Request

from monkeybot_api.core.config import Settings
from monkeybot_api.db.database import Database
from monkeybot_api.integrations.discord import DiscordClient


def get_settings(request: Request) -> Settings:
    return cast(Settings, request.app.state.settings)


def get_database(request: Request) -> Database:
    return cast(Database, request.app.state.database)


def get_discord(request: Request) -> DiscordClient:
    return cast(DiscordClient, request.app.state.discord)


SettingsDep = Annotated[Settings, Depends(get_settings)]
DatabaseDep = Annotated[Database, Depends(get_database)]
DiscordDep = Annotated[DiscordClient, Depends(get_discord)]
