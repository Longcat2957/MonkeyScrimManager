"""Member administration fixtures with one operator and one target member."""

import httpx
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncEngine

from tests.support.guilds.context import GUILD_A, ROLE_A, USER_ID, authenticated_setup
from tests.support.users.profile import csrf

MEMBER_ID = "100000000000000002"
MEMBERS = f"/api/guilds/{GUILD_A}/members"
MMR = f"{MEMBERS}/{MEMBER_ID}/mmr"
STATUS = f"{MEMBERS}/{MEMBER_ID}/status"
MMR_ROUTE = "/api/guilds/{guildId}/members/{memberId}/mmr"
DEFAULT_MMR = dict.fromkeys(("TOP", "JUNGLE", "MID", "BOTTOM", "SUPPORT"), 1500.0)


async def setup(owner: AsyncEngine, client: httpx.AsyncClient) -> dict[str, str]:
    material = await authenticated_setup(owner, client)
    async with owner.begin() as c:
        await c.execute(
            text("UPDATE guild_members SET display_name='Operator' WHERE user_id=:user"),
            {"user": USER_ID},
        )
        await c.execute(
            text("""
                INSERT INTO guild_member_discord_roles
                    (guild_id, user_id, role_id, last_synced_at)
                VALUES (:guild, :user, :role, now())
            """),
            {"guild": GUILD_A, "user": USER_ID, "role": ROLE_A},
        )
        await c.execute(
            text("""
                INSERT INTO users (user_id, identity_kind, display_name, created_at, updated_at)
                VALUES (:member, 'DISCORD', 'Global target', now(), now())
            """),
            {"member": MEMBER_ID},
        )
        await c.execute(
            text("""
                INSERT INTO guild_members
                    (guild_id, user_id, display_name, status, first_seen_at, registered_at,
                     last_active_at)
                VALUES (:guild, :member, 'Alpha', 'ACTIVE', now(),
                        '2026-01-02T03:04:05Z', '2026-02-03T04:05:06Z')
            """),
            {"guild": GUILD_A, "member": MEMBER_ID},
        )
    return csrf(material)


def adjustment(revision: int = 0, **changes) -> dict:
    return {
        "lane": "MID",
        "delta": 25.5,
        "note": "  수동 조정  ",
        "expectedRevision": revision,
        **changes,
    }


async def adjust(client, headers, body=None, *, key="member-mmr", member=MEMBER_ID):
    return await client.patch(
        f"{MEMBERS}/{member}/mmr",
        json=adjustment() if body is None else body,
        headers={**headers, "idempotency-key": key},
    )


async def snapshot(client, headers) -> dict:
    response = await client.get(MEMBERS, headers=headers)
    assert response.status_code == 200, response.text
    return response.json()["data"]


def member(snapshot: dict, member_id: str = MEMBER_ID) -> dict:
    return next(row for row in snapshot["members"] if row["id"] == member_id)


async def effects(owner: AsyncEngine) -> dict:
    """Small persisted-state summary for rollback and replay assertions."""
    async with owner.connect() as c:
        row = (
            (
                await c.execute(
                    text("""
                    SELECT
                        (SELECT count(*) FROM guild_admin_state WHERE scope='MEMBERS') AS state,
                        (SELECT count(*) FROM user_lane_mmr) AS lanes,
                        (SELECT count(*) FROM mmr_ledger_entries) AS ledger,
                        (SELECT count(*) FROM admin_audit_log) AS audit,
                        (SELECT count(*) FROM idempotency_records) AS idempotency,
                        (SELECT count(*) FROM outbox_events) AS outbox
                """)
                )
            )
            .mappings()
            .one()
        )
    return dict(row)
