"""Reusable full data transfer fixtures and request helpers."""

import json
import uuid

import httpx
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncEngine

from tests.support.guilds.context import GUILD_A, GUILD_B, ROLE_A, USER_ID, authenticated_setup
from tests.support.users.profile import csrf

EXPORT = f"/api/guilds/{GUILD_A}/full-data-export"


async def setup(owner: AsyncEngine, client: httpx.AsyncClient) -> dict[str, str]:
    material = await authenticated_setup(owner, client)
    async with owner.begin() as c:
        await c.execute(
            text("UPDATE guild_config SET display_name='Export guild' WHERE guild_id=:guild"),
            {"guild": GUILD_A},
        )
        await c.execute(
            text("""
                INSERT INTO guild_member_discord_roles (guild_id, user_id, role_id, last_synced_at)
                VALUES (:guild, :user, :role, now())
            """),
            {"guild": GUILD_A, "user": USER_ID, "role": ROLE_A},
        )
    return csrf(material)


async def export(client: httpx.AsyncClient, headers: dict[str, str]) -> httpx.Response:
    return await client.post(EXPORT, headers=headers)


async def seed_rows(owner: AsyncEngine) -> dict[str, str]:
    dummy = f"dmy_{uuid.uuid7()}"
    foreign = "200000000000000001"
    creator = "200000000000000002"
    updater = "200000000000000003"
    recruitment = uuid.uuid7()
    series = uuid.uuid7()
    audit = uuid.uuid7()
    checkpoint = uuid.uuid7()
    async with owner.begin() as c:
        await c.execute(
            text("""
                INSERT INTO users (user_id, identity_kind, display_name, created_at, updated_at)
                VALUES (:dummy, 'DUMMY', 'Dummy source', now(), now()),
                       (:foreign, 'DISCORD', 'Foreign user', now(), now()),
                       (:creator, 'DISCORD', 'Dummy creator', now(), now()),
                       (:updater, 'DISCORD', 'Dummy updater', now(), now())
            """),
            {"dummy": dummy, "foreign": foreign, "creator": creator, "updater": updater},
        )
        nonmembers = [{"user": user} for user in (foreign, creator, updater)]
        await c.execute(
            text("""
                INSERT INTO user_discord_identities (user_id, discord_id, created_at, updated_at)
                VALUES (:user, :user, now(), now())
            """),
            nonmembers,
        )
        await c.execute(
            text("""
                INSERT INTO user_privacy_settings (
                    user_id, public_profile_enabled, show_riot_ids,
                    allow_external_record_search, updated_at
                ) VALUES (:user, false, false, false, now())
            """),
            nonmembers,
        )
        await c.execute(
            text("""
                INSERT INTO user_champion_preferences (user_id, lane, position, champion_id)
                VALUES (:user, 'MID', 1, 'Ahri')
            """),
            [{"user": user} for user in (USER_ID, foreign, creator, updater)],
        )
        await c.execute(
            text("""
                INSERT INTO riot_accounts (
                    puuid, user_id, platform, routing_region, game_name, tag_line,
                    normalized_game_name, normalized_tag_line, is_main,
                    created_at, updated_at, origin
                ) VALUES (
                    :puuid, :user, 'KR', 'ASIA', :user, 'KR1', :user, 'kr1', true,
                    now(), now(), :origin
                )
            """),
            [
                {
                    "user": user,
                    "puuid": f"dummy:{user}" if user == dummy else f"export:{user}",
                    "origin": "DUMMY" if user == dummy else "RIOT",
                }
                for user in (USER_ID, dummy, foreign, creator, updater)
            ],
        )
        await c.execute(
            text("""
                INSERT INTO dummy_user_profiles (
                    user_id, stable_key, state, revision,
                    created_by, updated_by, created_at, updated_at
                ) VALUES (
                    :dummy, 'export_dummy', 'ACTIVE', 0, :creator, :updater, now(), now()
                )
            """),
            {"dummy": dummy, "creator": creator, "updater": updater},
        )
        await c.execute(
            text("""
                INSERT INTO guilds (guild_id, discord_name, status, installed_at, updated_at)
                VALUES (:foreign_guild, 'Foreign', 'ACTIVE', now(), now())
            """),
            {"foreign_guild": GUILD_B},
        )
        await c.execute(
            text("""
                INSERT INTO guild_config (
                    guild_id, display_name, operator_role_id,
                    is_externally_public, revision, updated_at
                )
                VALUES (:foreign_guild, 'Foreign guild', :role, false, 0, now())
            """),
            {"foreign_guild": GUILD_B, "role": ROLE_A},
        )
        await c.execute(
            text("""
                INSERT INTO guild_members (guild_id, user_id, display_name, status, first_seen_at)
                VALUES (:guild, :dummy, 'Dummy member', 'ACTIVE', now()),
                       (:foreign_guild, :dummy, 'Dummy member elsewhere', 'ACTIVE', now()),
                       (:foreign_guild, :foreign, 'Foreign member', 'ACTIVE', now())
            """),
            {"guild": GUILD_A, "dummy": dummy, "foreign_guild": GUILD_B, "foreign": foreign},
        )
        await c.execute(
            text("""
                INSERT INTO user_guild_preferences (user_id, default_guild_id, updated_at)
                VALUES (:actor, :guild, now()), (:foreign, :foreign_guild, now()),
                       (:dummy, :foreign_guild, now())
                ON CONFLICT (user_id) DO UPDATE SET default_guild_id = EXCLUDED.default_guild_id
            """),
            {
                "actor": USER_ID,
                "guild": GUILD_A,
                "foreign": foreign,
                "foreign_guild": GUILD_B,
                "dummy": dummy,
            },
        )
        await c.execute(
            text("""
                INSERT INTO revert_checkpoints (
                    guild_id, id, target_type, target_id, target_revision, action,
                    schema_version, restore_payload, status, created_by_type,
                    created_by, created_at
                ) VALUES (
                    :guild, :checkpoint, 'TEST', :recruitment, 0, 'TEST_SEED',
                    1, '{}', 'ACTIVE', 'USER', :actor, now()
                )
            """),
            {
                "guild": GUILD_A,
                "checkpoint": checkpoint,
                "recruitment": str(recruitment),
                "actor": USER_ID,
            },
        )
        await c.execute(
            text("""
                INSERT INTO guild_counters (guild_id, counter_key, next_value)
                VALUES (:guild, 'RECRUITMENT', 2)
            """),
            {"guild": GUILD_A},
        )
        season = await c.scalar(
            text("SELECT id FROM seasons WHERE guild_id=:guild AND status='ACTIVE'"),
            {"guild": GUILD_A},
        )
        await c.execute(
            text("""
                INSERT INTO recruitments (
                    guild_id, id, season_id, display_number, title, target_count, status,
                    best_of, hard_fearless, revision, created_by,
                    created_at, updated_at
                ) VALUES (
                    :guild, :recruitment, :season, 1, '일반 내전 모집 #1', 10, 'OPEN',
                    3, true, 0, :actor, now(), now()
                )
            """),
            {"guild": GUILD_A, "recruitment": recruitment, "season": season, "actor": USER_ID},
        )
        await c.execute(
            text("""
                INSERT INTO series (
                    guild_id, id, season_id, display_number, title, source_recruitment_id,
                    best_of, hard_fearless, status, revision, started_at, updated_at
                ) VALUES (
                    :guild, :series, :season, 1, '일반 내전 #1', :recruitment,
                    3, true, 'IN_PROGRESS', 0, now(), now()
                )
            """),
            {"guild": GUILD_A, "series": series, "season": season, "recruitment": recruitment},
        )
        await c.execute(
            text("""
                INSERT INTO admin_audit_log (
                    guild_id, id, principal_type, principal_id, action, target_type, target_id,
                    checkpoint_id, request_id, before_summary, after_summary, result, note,
                    created_at
                ) VALUES (
                    :guild, :audit, 'USER', :actor, 'TEST_SEED', 'TEST', :recruitment,
                    :checkpoint, 'seed-request', CAST(:before AS jsonb), CAST(:after AS jsonb),
                    'APPLIED', NULL, now()
                )
            """),
            {
                "guild": GUILD_A,
                "audit": audit,
                "checkpoint": checkpoint,
                "actor": USER_ID,
                "recruitment": str(recruitment),
                "before": json.dumps({"seed": True}),
                "after": json.dumps({"series": str(series)}),
            },
        )
    return {
        "dummy": dummy,
        "foreign": foreign,
        "creator": creator,
        "updater": updater,
        "recruitment": str(recruitment),
        "series": str(series),
    }


async def audit_actions(owner: AsyncEngine) -> list[str]:
    async with owner.connect() as c:
        rows = (
            (
                await c.execute(
                    text("""
                    SELECT action FROM admin_audit_log
                    WHERE guild_id=:guild ORDER BY created_at, id
                """),
                    {"guild": GUILD_A},
                )
            )
            .mappings()
            .all()
        )
    return [row["action"] for row in rows]
