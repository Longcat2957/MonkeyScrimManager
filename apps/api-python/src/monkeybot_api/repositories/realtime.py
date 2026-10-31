"""Read-only topic permissions and leased realtime outbox operations."""

import uuid
from datetime import UTC, datetime
from typing import Any

from sqlalchemy import text

from monkeybot_api.db.database import Database

ACTIVE_MEMBER = text("""
    SELECT EXISTS (
      SELECT 1 FROM guild_members AS member
      JOIN guilds AS guild ON guild.guild_id=member.guild_id AND guild.status='ACTIVE'
      WHERE member.guild_id=:guild AND member.user_id=:user AND member.status='ACTIVE'
    )
""")

VISIBLE_RECRUITMENT = text("""
    SELECT EXISTS (
      SELECT 1 FROM recruitments
      WHERE guild_id=:guild AND id=:resource AND status IN ('OPEN', 'CLOSED')
    )
""")

OPERATOR_TOPIC = {
    "series": ("series", "resource.status='IN_PROGRESS' AND resource.deleted_at IS NULL"),
    "auction-recruitment": (
        "auction_recruitments",
        "resource.status IN ('OPEN', 'CLOSED')",
    ),
    "auction-tournament": (
        "auction_tournaments",
        "resource.status='BIDDING' AND resource.deleted_at IS NULL",
    ),
}


async def authorize_topic(
    database: Database, user: str, guild: str, kind: str, resource: uuid.UUID
) -> str:
    """Return allowed, forbidden, or not_found. The caller maps failures safely."""
    values = {"user": user, "guild": guild, "resource": resource}
    async with database.connection() as c:
        if kind in {"recruitment", "recruitment-entry-draft"}:
            if not await c.scalar(ACTIVE_MEMBER, values):
                return "forbidden"
            if not await c.scalar(VISIBLE_RECRUITMENT, values):
                return "not_found"
            return "allowed"
        table, state = OPERATOR_TOPIC[kind]
        # The table and predicate are fixed constants above, never client input.
        query = text(f"""
            SELECT EXISTS (
              SELECT 1 FROM {table} AS resource
              JOIN guilds AS guild ON guild.guild_id=resource.guild_id
                AND guild.status='ACTIVE'
              JOIN guild_members AS member ON member.guild_id=resource.guild_id
                AND member.user_id=:user AND member.status='ACTIVE'
              JOIN guild_config AS config ON config.guild_id=resource.guild_id
              JOIN guild_member_discord_roles AS role
                ON role.guild_id=member.guild_id AND role.user_id=member.user_id
                AND role.role_id=config.operator_role_id
              WHERE resource.guild_id=:guild AND resource.id=:resource AND {state}
            )
        """)
        return "allowed" if await c.scalar(query, values) else "forbidden"


async def claim_any(database: Database, owner: str, limit: int = 64) -> list[dict[str, Any]]:
    """Commit claims before any external publish; each claim gets a fresh token."""
    async with database.connection(transaction=True) as c:
        rows = (
            (
                await c.execute(
                    text("""
                    WITH claim_clock AS MATERIALIZED (SELECT clock_timestamp() AS current_time)
                    SELECT id, guild_id, topic, aggregate_id, aggregate_revision, occurred_at
                    FROM outbox_events CROSS JOIN claim_clock
                    WHERE published_at IS NULL AND available_at<=claim_clock.current_time
                      AND (lease_until IS NULL OR lease_until<claim_clock.current_time)
                    ORDER BY available_at, id LIMIT :limit
                    FOR UPDATE OF outbox_events SKIP LOCKED
                    """),
                    {"limit": limit},
                )
            )
            .mappings()
            .all()
        )
        claimed: list[dict[str, Any]] = []
        for row in rows:
            token = uuid.uuid7()
            lease_until = await c.scalar(
                text("""
                UPDATE outbox_events
                SET lease_owner=:owner, lease_token=:token,
                    lease_until=clock_timestamp()+interval '30 seconds'
                WHERE guild_id=:guild AND id=:id AND published_at IS NULL
                RETURNING lease_until
                """),
                {"owner": owner, "token": token, "guild": row["guild_id"], "id": row["id"]},
            )
            if lease_until is None:
                raise RuntimeError("claimed realtime outbox row disappeared")
            claimed.append({**dict(row), "lease_owner": owner, "lease_token": token})
        return claimed


async def ack(database: Database, event: dict[str, Any]) -> bool:
    async with database.connection(transaction=True) as c:
        result = await c.execute(
            text("""
            UPDATE outbox_events
            SET published_at=:now, lease_owner=NULL, lease_token=NULL, lease_until=NULL
            WHERE guild_id=:guild AND id=:id AND lease_owner=:owner
              AND lease_token=:token AND published_at IS NULL
            """),
            {
                "now": datetime.now(UTC),
                "guild": event["guild_id"],
                "id": event["id"],
                "owner": event["lease_owner"],
                "token": event["lease_token"],
            },
        )
        return result.rowcount == 1


async def nack(database: Database, event: dict[str, Any], reason: str) -> bool:
    async with database.connection(transaction=True) as c:
        result = await c.execute(
            text("""
            UPDATE outbox_events
            SET attempts=attempts+1, available_at=clock_timestamp()+interval '5 seconds',
                last_error=:reason, lease_owner=NULL, lease_token=NULL, lease_until=NULL
            WHERE guild_id=:guild AND id=:id AND lease_owner=:owner
              AND lease_token=:token AND published_at IS NULL
            """),
            {
                "guild": event["guild_id"],
                "id": event["id"],
                "owner": event["lease_owner"],
                "token": event["lease_token"],
                "reason": reason,
            },
        )
        return result.rowcount == 1
