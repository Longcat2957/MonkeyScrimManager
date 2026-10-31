"""Member administration queries, sharing the service's transaction."""

import json
import uuid
from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection


async def metadata(c: AsyncConnection, guild: str) -> dict[str, Any] | None:
    row = (
        (
            await c.execute(
                text("""
        SELECT COALESCE(admin_state.revision, 0) AS revision,
               COALESCE(admin_state.updated_at, config.updated_at, guild.updated_at) AS updated_at,
               COALESCE(admin_state.updated_by, config.updated_by) AS updated_by
        FROM guilds AS guild
        JOIN guild_config AS config ON config.guild_id = guild.guild_id
        LEFT JOIN guild_admin_state AS admin_state
          ON admin_state.guild_id = guild.guild_id AND admin_state.scope = 'MEMBERS'
        WHERE guild.guild_id = :guild AND guild.status = 'ACTIVE'
    """),
                {"guild": guild},
            )
        )
        .mappings()
        .one_or_none()
    )
    return dict(row) if row is not None else None


async def member_lanes(c: AsyncConnection, guild: str) -> list[dict[str, Any]]:
    rows = (
        (
            await c.execute(
                text("""
        WITH canonical_lanes (lane, lane_order) AS (
            VALUES ('TOP'::text, 1), ('JUNGLE', 2), ('MID', 3), ('BOTTOM', 4), ('SUPPORT', 5)
        ), active_season AS (
            SELECT id FROM seasons WHERE guild_id = :guild AND status = 'ACTIVE'
        )
        SELECT member.user_id, app_user.identity_kind, member.display_name, riot.riot_id,
               EXISTS (
                   SELECT 1 FROM guild_member_discord_roles AS member_role
                   WHERE member_role.guild_id = member.guild_id
                     AND member_role.user_id = member.user_id
                     AND member_role.role_id = config.operator_role_id
               ) AS is_operator,
               member.main_lane, member.registered_at IS NOT NULL AS registered,
               member.status, member.last_active_at, canonical_lanes.lane AS mmr_lane,
               COALESCE(lane_mmr.mmr, 1500::double precision) AS mmr
        FROM guild_members AS member
        JOIN users AS app_user ON app_user.user_id = member.user_id
        JOIN guild_config AS config ON config.guild_id = member.guild_id
        CROSS JOIN canonical_lanes
        LEFT JOIN active_season ON true
        LEFT JOIN user_lane_mmr AS lane_mmr
          ON lane_mmr.guild_id = member.guild_id AND lane_mmr.season_id = active_season.id
         AND lane_mmr.user_id = member.user_id AND lane_mmr.lane = canonical_lanes.lane
        LEFT JOIN LATERAL (
            SELECT account.game_name || '#' || account.tag_line AS riot_id
            FROM riot_accounts AS account WHERE account.user_id = member.user_id AND account.is_main
            LIMIT 1
        ) AS riot ON true
        WHERE member.guild_id = :guild
        ORDER BY CASE member.status WHEN 'ACTIVE' THEN 0 ELSE 1 END,
                 lower(member.display_name), member.user_id, canonical_lanes.lane_order
    """),
                {"guild": guild},
            )
        )
        .mappings()
        .all()
    )
    return [dict(row) for row in rows]


async def lock_state(c: AsyncConnection, p: dict[str, Any]) -> dict[str, Any]:
    await c.execute(
        text("""
        INSERT INTO guild_admin_state (guild_id, scope, revision, updated_by, updated_at)
        VALUES (:guild, 'MEMBERS', 0, :actor_id, :now)
        ON CONFLICT (guild_id, scope) DO NOTHING
    """),
        p,
    )
    row = (
        (
            await c.execute(
                text("""
        SELECT revision, updated_at, updated_by FROM guild_admin_state
        WHERE guild_id = :guild AND scope = 'MEMBERS' FOR UPDATE
    """),
                p,
            )
        )
        .mappings()
        .one()
    )
    return dict(row)


async def lock_member_status(c: AsyncConnection, p: dict[str, Any]) -> str | None:
    status: str | None = await c.scalar(
        text("""
        SELECT status FROM guild_members WHERE guild_id = :guild AND user_id = :member FOR UPDATE
    """),
        p,
    )
    return status


async def lock_active_season(c: AsyncConnection, guild: str) -> uuid.UUID | None:
    season: uuid.UUID | None = await c.scalar(
        text("""
        SELECT id FROM seasons WHERE guild_id = :guild AND status = 'ACTIVE' FOR UPDATE
    """),
        {"guild": guild},
    )
    return season


async def increment_revision(c: AsyncConnection, p: dict[str, Any], current: int) -> int:
    result = await c.execute(
        text("""
        UPDATE guild_admin_state SET revision = :next, updated_by = :actor_id, updated_at = :now
        WHERE guild_id = :guild AND scope = 'MEMBERS' AND revision = :current
    """),
        {**p, "current": current, "next": current + 1},
    )
    return result.rowcount


async def set_status(c: AsyncConnection, p: dict[str, Any], status: str) -> int:
    result = await c.execute(
        text("""
        UPDATE guild_members SET status = :status,
            inactivated_at = CASE WHEN :status = 'INACTIVE'
                                  THEN CAST(:now AS timestamptz) ELSE NULL END
        WHERE guild_id = :guild AND user_id = :member
    """),
        {**p, "status": status},
    )
    return result.rowcount


async def append_adjustment(c: AsyncConnection, p: dict[str, Any]) -> None:
    await c.execute(
        text("""
        INSERT INTO mmr_ledger_entries (
            guild_id, id, season_id, user_id, lane, lane_revision, source_type, source_id,
            game_id, opponent_user_id, mmr_before, mmr_after, delta, games_played_delta,
            wins_delta, reverses_entry_id, reversed_source_type, actor_type, actor_id, note,
            request_id, created_at
        ) VALUES (
            :guild, :ledger_id, :season, :member, :lane, :lane_revision,
            'MANUAL_ADJUSTMENT', :source_id, NULL, NULL, :before, :after, :delta,
            0, 0, NULL, NULL, 'USER', :actor_id, :note, :request_id, :now
        )
    """),
        p,
    )


async def update_mmr(c: AsyncConnection, p: dict[str, Any]) -> int:
    result = await c.execute(
        text("""
        UPDATE user_lane_mmr SET mmr = :after, revision = :lane_revision, updated_at = :now
        WHERE guild_id = :guild AND season_id = :season AND user_id = :member AND lane = :lane
    """),
        p,
    )
    return result.rowcount


async def append_audit(
    c: AsyncConnection,
    p: dict[str, Any],
    action: str,
    target_type: str,
    before: dict[str, Any],
    after: dict[str, Any],
    note: str | None = None,
) -> None:
    await c.execute(
        text("""
        INSERT INTO admin_audit_log (
            guild_id, id, principal_type, principal_id, action, target_type, target_id,
            checkpoint_id, request_id, before_summary, after_summary, result, note, created_at
        ) VALUES (
            :guild, :audit_id, 'USER', :actor_id, :action, :target_type, :member,
            NULL, :request_id, CAST(:before AS jsonb), CAST(:after AS jsonb), 'APPLIED', :note, :now
        )
    """),
        {
            **p,
            "action": action,
            "target_type": target_type,
            "before": json.dumps(before),
            "after": json.dumps(after),
            "note": note,
        },
    )
