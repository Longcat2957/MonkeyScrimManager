"""Small identity/cache builders for autocomplete contract tests."""

import json
import uuid
from datetime import datetime
from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncEngine

from monkeybot_api.core.riot_identity import normalize_riot_id
from tests.support.guilds.context import GUILD_A

PUBLIC_PATH = "/api/riot/records/suggestions"
GUILD_PATH = f"/api/guilds/{GUILD_A}/records/suggestions"


async def member(
    owner: AsyncEngine,
    number: int,
    name: str,
    *,
    guild: str = GUILD_A,
    active: bool = True,
    deleted: bool = False,
    show_riot: bool | None = None,
    game_name: str | None = None,
    platform: str = "KR",
    main: bool = True,
    origin: str = "RIOT",
) -> str:
    identity_kind = "DUMMY" if origin == "DUMMY" else "DISCORD"
    identifier = f"dmy_{uuid.uuid7()}" if origin == "DUMMY" else str(100000000000000000 + number)
    async with owner.begin() as connection:
        await connection.execute(
            text("""
            INSERT INTO users (
                user_id, identity_kind, display_name, created_at, updated_at, deleted_at
            ) VALUES (:user, :kind, :name, now(), now(), CASE WHEN :deleted THEN now() END)
            """),
            {"user": identifier, "kind": identity_kind, "name": name, "deleted": deleted},
        )
        await connection.execute(
            text("""
            INSERT INTO guild_members (guild_id, user_id, display_name, status, first_seen_at)
            VALUES (:guild, :user, :name, :status, now())
            """),
            {
                "guild": guild,
                "user": identifier,
                "name": name,
                "status": "ACTIVE" if active else "INACTIVE",
            },
        )
        if show_riot is not None:
            await connection.execute(
                text("""
                INSERT INTO user_privacy_settings (
                    user_id, public_profile_enabled, show_riot_ids,
                    allow_external_record_search, updated_at
                ) VALUES (:user, false, :show, false, now())
                """),
                {"user": identifier, "show": show_riot},
            )
        if game_name is not None:
            await connection.execute(
                text("""
                INSERT INTO riot_accounts (
                    puuid, user_id, platform, routing_region, game_name, tag_line,
                    normalized_game_name, normalized_tag_line, profile_icon_id,
                    is_main, created_at, updated_at, origin
                ) VALUES (
                    :puuid, :user, :platform, 'ASIA', :name, 'KR1', :normalized, 'kr1',
                    :icon, :main, now(), '2026-09-01T00:00:00Z', :origin
                )
                """),
                {
                    "puuid": ("dummy:" if origin == "DUMMY" else "account:") + identifier,
                    "user": identifier,
                    "platform": platform,
                    "name": game_name,
                    "normalized": normalize_riot_id(game_name),
                    "icon": number,
                    "main": main,
                    "origin": origin,
                },
            )
    return identifier


async def snapshot(
    owner: AsyncEngine,
    identifier: str,
    name: str,
    *,
    tag: str = "KR1",
    icon: Any = None,
    known_at: str = "2026-09-02T00:00:00Z",
) -> None:
    async with owner.begin() as connection:
        await connection.execute(
            text("""
            INSERT INTO riot_records_snapshots (
                platform, puuid, routing_region, game_name, tag_line,
                normalized_game_name, normalized_tag_line, schema_version, snapshot,
                refreshed_at, created_at, updated_at
            ) VALUES (
                'KR', :id, 'ASIA', :name, :tag, :normalized, :normalized_tag, 1,
                CAST(:snapshot AS jsonb), now(), now(), CAST(:known_at AS timestamptz)
            )
            """),
            {
                "id": identifier,
                "name": name,
                "tag": tag,
                "normalized": normalize_riot_id(name),
                "normalized_tag": normalize_riot_id(tag),
                "snapshot": json.dumps({"profileIconId": icon}),
                "known_at": datetime.fromisoformat(known_at),
            },
        )
