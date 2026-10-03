"""Identity-only fixtures shared by guild record profile contract tests."""

import httpx
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncEngine

from tests.support.guilds.context import GUILD_A, authenticated_setup

PLAYER = "100000000000000003"
OTHER = "100000000000000004"
DUMMY = "dmy_01900000-0000-7000-8000-000000000005"
PATH = f"/api/guilds/{GUILD_A}/records/profile"


async def setup(owner: AsyncEngine, client: httpx.AsyncClient) -> None:
    await authenticated_setup(owner, client)
    async with owner.begin() as c:
        for user, name in ((PLAYER, "Longcat"), (OTHER, "Other"), (DUMMY, "Dummy")):
            params = {
                "user": user,
                "name": name,
                "guild": GUILD_A,
                "kind": "DUMMY" if user == DUMMY else "DISCORD",
            }
            await c.execute(
                text("""
                INSERT INTO users
                    (user_id, identity_kind, display_name, avatar_hash, created_at, updated_at)
                VALUES (:user, :kind, :name, 'a_0123abcdef', now(), now())
            """),
                params,
            )
            await c.execute(
                text("""
                INSERT INTO guild_members
                    (guild_id, user_id, display_name, status, main_lane, first_seen_at)
                VALUES (:guild, :user, :name, 'ACTIVE', 'MID', now())
            """),
                params,
            )
        await c.execute(
            text("""
            INSERT INTO user_privacy_settings
                (user_id, public_profile_enabled, show_riot_ids,
                 allow_external_record_search, updated_at)
            VALUES (:user, true, true, false, now())
        """),
            {"user": PLAYER},
        )
        for user, name, tag, main, icon in (
            (PLAYER, "Longcat", "KR1", True, 7),
            (PLAYER, "LongAlt", "KR2", False, 8),
            (OTHER, "SecretLink", "KR3", True, 9),
        ):
            await c.execute(
                text("""
                INSERT INTO riot_accounts
                    (puuid, user_id, platform, routing_region, game_name, tag_line,
                     normalized_game_name, normalized_tag_line, profile_icon_id,
                     is_main, last_synced_at, created_at, updated_at)
                VALUES (:puuid, :user, 'KR', 'ASIA', :name, :tag, lower(:name), lower(:tag),
                        :icon, :main, now(), now(), now())
            """),
                {
                    "puuid": "profile-" + name,
                    "user": user,
                    "name": name,
                    "tag": tag,
                    "icon": icon,
                    "main": main,
                },
            )
        await c.execute(
            text("""
            INSERT INTO user_champion_preferences (user_id, lane, position, champion_id)
            VALUES (:user, 'TOP', 1, 'Gnar'), (:user, 'MID', 1, 'Ahri'),
                   (:user, 'MID', 2, 'Orianna')
        """),
            {"user": PLAYER},
        )
