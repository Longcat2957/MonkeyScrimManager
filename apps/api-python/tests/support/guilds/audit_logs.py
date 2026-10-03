"""Small audit fixtures built on the shared authenticated guild setup."""

import json
import uuid
from datetime import UTC, datetime, timedelta

from sqlalchemy import text

from tests.support.guilds.context import (
    GUILD_A,
    GUILD_B,
    ROLE_A,
    ROLE_B,
    USER_ID,
    authenticated_setup,
    guild_fixture,
)

PATH = f"/api/guilds/{GUILD_A}/audit-logs"
CREATED_AT = datetime(2026, 1, 2, 3, 4, 5, 123456, tzinfo=UTC)
FALLBACK_USER = "900000000000000001"
IDS = [uuid.UUID(int=100 - index) for index in range(8)]
CHECKPOINT = uuid.UUID(int=200)
RECRUITMENT = uuid.UUID(int=201)


async def setup(owner, client):
    await authenticated_setup(owner, client)
    async with owner.begin() as connection:
        await connection.execute(
            text("""
            INSERT INTO guild_member_discord_roles (guild_id, user_id, role_id, last_synced_at)
            VALUES (:guild, :user, :role, now())
            """),
            {"guild": GUILD_A, "user": USER_ID, "role": ROLE_A},
        )
        await connection.execute(
            text("UPDATE guild_config SET display_name='Audit Guild' WHERE guild_id=:guild"),
            {"guild": GUILD_A},
        )


async def insert_audit(owner, record_id, *, guild=GUILD_A, created_at=CREATED_AT, **changes):
    values = {
        "guild": guild,
        "id": record_id,
        "principal_type": "USER",
        "principal_id": USER_ID,
        "action": "UNRECOGNIZED_ACTION",
        "target_type": None,
        "target_id": None,
        "checkpoint": None,
        "request_id": None,
        "before": None,
        "after": None,
        "result": "APPLIED",
        "note": None,
        "created_at": created_at,
        **changes,
    }
    for field in ("before", "after"):
        if values[field] is not None:
            values[field] = json.dumps(values[field], ensure_ascii=False)
    async with owner.begin() as connection:
        await connection.execute(
            text("""
            INSERT INTO admin_audit_log
              (guild_id, id, principal_type, principal_id, action, target_type, target_id,
               checkpoint_id, request_id, before_summary, after_summary, result, note, created_at)
            VALUES (:guild, :id, :principal_type, :principal_id, :action, :target_type, :target_id,
                    :checkpoint, :request_id, CAST(:before AS jsonb), CAST(:after AS jsonb),
                    :result, :note, :created_at)
            """),
            values,
        )


async def projected_entries(owner, client):
    await setup(owner, client)
    other_season = await guild_fixture(owner, GUILD_B, "Other Guild", ROLE_B)
    async with owner.begin() as connection:
        await connection.execute(
            text("""
            INSERT INTO users (user_id, display_name, created_at, updated_at)
            VALUES (:user, 'Global Fallback', now(), now())
            """),
            {"user": FALLBACK_USER},
        )
        await connection.execute(
            text("""
            INSERT INTO guild_members (guild_id, user_id, display_name, status, first_seen_at)
            VALUES (:guild, :user, 'Other Tenant Name', 'ACTIVE', now())
            """),
            {"guild": GUILD_B, "user": FALLBACK_USER},
        )
        season = await connection.scalar(
            text("SELECT id FROM seasons WHERE guild_id=:guild"), {"guild": GUILD_A}
        )
        await connection.execute(
            text("""
            INSERT INTO recruitments
              (guild_id, id, season_id, display_number, title, status, revision,
               created_by, created_at, updated_at)
            VALUES (:guild, :id, :season, 1, '금요일 내전 모집', 'OPEN', 0, :actor, now(), now())
            """),
            {"guild": GUILD_A, "id": RECRUITMENT, "season": season, "actor": USER_ID},
        )
        await connection.execute(
            text("""
            INSERT INTO revert_checkpoints
              (guild_id, id, target_type, target_id, target_revision, action, schema_version,
               restore_payload, status, created_by_type, created_by, created_at,
               reverted_by_type, reverted_by, reverted_at)
            VALUES (:guild, :id, 'SEASON', :season, 1, 'TEST_REVERT', 1, '{}', 'REVERTED',
                    'SYSTEM', 'system', now(), 'SYSTEM', 'system', now())
            """),
            {"guild": GUILD_A, "id": CHECKPOINT, "season": str(season)},
        )
    entries = [
        {
            "target_type": "GUILD_MEMBER_MMR",
            "target_id": USER_ID,
            "action": "MEMBER_MMR_ADJUSTED",
            "before": {"mmr": 1500, "nested": [None, True, {"name": "한글"}]},
            "after": [1520, "unmodified"],
            "request_id": "request-mmr",
            "note": "수동 조정",
        },
        {"principal_type": "BOT", "target_type": "GUILD_CONFIG", "target_id": GUILD_A},
        {
            "principal_type": "SYSTEM",
            "target_type": "SEASON",
            "target_id": str(season),
            "result": "REVERTED",
            "checkpoint": CHECKPOINT,
        },
        {
            "principal_type": "MIGRATION",
            "target_type": "RECRUITMENT",
            "target_id": str(RECRUITMENT),
        },
        {
            "principal_id": FALLBACK_USER,
            "target_type": "GUILD_MEMBER",
            "target_id": FALLBACK_USER,
        },
        {"target_type": "GUILD", "target_id": GUILD_A},
        {"target_type": "SEASON", "target_id": str(other_season)},
        {"principal_id": "999", "target_type": "UNKNOWN", "target_id": "999"},
    ]
    for index, changes in enumerate(entries):
        await insert_audit(
            owner,
            IDS[index],
            created_at=CREATED_AT - timedelta(seconds=max(0, index - 1)),
            **changes,
        )
    await insert_audit(owner, uuid.UUID(int=1000), guild=GUILD_B)
