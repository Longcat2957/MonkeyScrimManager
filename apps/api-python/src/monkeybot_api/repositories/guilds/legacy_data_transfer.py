"""Queries for the legacy member and season MMR seed transfer."""

import json
import uuid
from typing import Any, cast

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection


async def season(c: AsyncConnection, guild: str, *, lock: bool = False) -> dict[str, Any] | None:
    sql = "SELECT id, name FROM seasons WHERE guild_id=:guild AND status='ACTIVE'"
    if lock:
        sql += " FOR UPDATE"
    row = (await c.execute(text(sql), {"guild": guild})).mappings().one_or_none()
    return dict(row) if row is not None else None


async def guild_name(c: AsyncConnection, guild: str) -> str | None:
    value = await c.scalar(
        text("SELECT display_name FROM guild_config WHERE guild_id=:guild"), {"guild": guild}
    )
    return value if isinstance(value, str) else None


async def export_rows(c: AsyncConnection, guild: str, season_id: uuid.UUID) -> list[dict[str, Any]]:
    result = await c.execute(
        text("""
        WITH canonical_lanes (lane, lane_order) AS (
            VALUES ('TOP'::text, 1), ('JUNGLE', 2), ('MID', 3), ('BOTTOM', 4), ('SUPPORT', 5)
        )
        SELECT member.user_id, member.display_name, canonical_lanes.lane,
               COALESCE(lane_mmr.mmr, 1500::double precision) AS mmr
        FROM guild_members AS member
        JOIN users AS app_user ON app_user.user_id=member.user_id
                              AND app_user.identity_kind='DISCORD'
        CROSS JOIN canonical_lanes
        LEFT JOIN user_lane_mmr AS lane_mmr
          ON lane_mmr.guild_id=member.guild_id AND lane_mmr.season_id=:season
         AND lane_mmr.user_id=member.user_id AND lane_mmr.lane=canonical_lanes.lane
        WHERE member.guild_id=:guild AND member.status='ACTIVE'
        ORDER BY member.user_id, canonical_lanes.lane_order
    """),
        {"guild": guild, "season": season_id},
    )
    return [dict(row) for row in result.mappings()]


async def members_revision(c: AsyncConnection, guild: str) -> int | None:
    return cast(
        int | None,
        await c.scalar(
            text("""
        SELECT COALESCE(state.revision, 0)
        FROM guilds AS guild
        LEFT JOIN guild_admin_state AS state
          ON state.guild_id=guild.guild_id AND state.scope='MEMBERS'
        WHERE guild.guild_id=:guild AND guild.status='ACTIVE'
    """),
            {"guild": guild},
        ),
    )


async def active_members(c: AsyncConnection, guild: str, ids: list[str]) -> set[str]:
    result = await c.execute(
        text("""
        SELECT member.user_id
        FROM guild_members AS member
        JOIN users AS app_user ON app_user.user_id=member.user_id
                              AND app_user.identity_kind='DISCORD'
        WHERE member.guild_id=:guild AND member.status='ACTIVE'
          AND member.user_id=ANY(:ids)
    """),
        {"guild": guild, "ids": ids},
    )
    return set(result.scalars())


async def current_mmr(
    c: AsyncConnection, guild: str, season_id: uuid.UUID, ids: list[str]
) -> dict[tuple[str, str], float]:
    result = await c.execute(
        text("""
        SELECT user_id, lane, mmr FROM user_lane_mmr
        WHERE guild_id=:guild AND season_id=:season AND user_id=ANY(:ids)
    """),
        {"guild": guild, "season": season_id, "ids": ids},
    )
    return {(row.user_id, row.lane): row.mmr for row in result}


async def create_preview(c: AsyncConnection, p: dict[str, Any]) -> None:
    await c.execute(
        text("""
        INSERT INTO guild_data_imports (
          guild_id, id, actor_id, status, archive_digest, archive_payload, source_guild_id,
          source_season_id, target_season_id, target_members_revision, target_mmr_digest,
          summary, conflict_count, request_id, created_at, expires_at, applied_at
        ) VALUES (
          :guild, :import_id, :actor_id, 'PREVIEWED', :digest,
          CAST(:archive AS jsonb), :source_guild, :source_season, :season, :revision,
          :mmr_digest, CAST(:summary AS jsonb), :conflict_count, :request_id,
          :now, :expires, NULL
        )
    """),
        p,
    )


async def lock_import(
    c: AsyncConnection, guild: str, import_id: uuid.UUID
) -> dict[str, Any] | None:
    row = (
        (
            await c.execute(
                text("""
            SELECT actor_id, status, archive_digest, archive_payload::text AS archive_payload,
                   target_season_id,
                   target_members_revision, target_mmr_digest, summary, conflict_count, expires_at
            FROM guild_data_imports WHERE guild_id=:guild AND id=:import_id FOR UPDATE
        """),
                {"guild": guild, "import_id": import_id},
            )
        )
        .mappings()
        .one_or_none()
    )
    return dict(row) if row is not None else None


async def lock_members_state(c: AsyncConnection, p: dict[str, Any]) -> int:
    await c.execute(
        text("""
        INSERT INTO guild_admin_state (guild_id, scope, revision, updated_by, updated_at)
        VALUES (:guild, 'MEMBERS', 0, :actor_id, :now)
        ON CONFLICT (guild_id, scope) DO NOTHING
    """),
        p,
    )
    revision = await c.scalar(
        text("""
        SELECT revision FROM guild_admin_state
        WHERE guild_id=:guild AND scope='MEMBERS' FOR UPDATE
    """),
        p,
    )
    if revision is None:
        raise RuntimeError("member revision row missing after insert")
    return cast(int, revision)


async def has_active_workflow(c: AsyncConnection, guild: str) -> bool:
    value = await c.scalar(
        text("""
        SELECT EXISTS (
          SELECT 1 FROM recruitments WHERE guild_id=:guild AND status IN ('OPEN', 'CLOSED')
          UNION ALL
          SELECT 1 FROM series WHERE guild_id=:guild AND status='IN_PROGRESS' AND deleted_at IS NULL
          UNION ALL
          SELECT 1 FROM auction_recruitments
           WHERE guild_id=:guild AND status IN ('OPEN', 'CLOSED')
          UNION ALL
          SELECT 1 FROM auction_tournaments WHERE guild_id=:guild
           AND status IN ('BIDDING', 'BRACKET_SETUP', 'IN_GAME') AND deleted_at IS NULL
        )
    """),
        {"guild": guild},
    )
    return bool(value)


async def lock_mmr_rows(
    c: AsyncConnection, p: dict[str, Any], keys: list[tuple[str, str]]
) -> list[dict[str, Any]]:
    for user, lane in sorted(keys):
        await c.execute(
            text("""
            INSERT INTO user_lane_mmr
              (guild_id, season_id, user_id, lane, mmr, games_played, wins, revision, updated_at)
            VALUES (:guild, :season, :user, :lane, 1500, 0, 0, 0, :now)
            ON CONFLICT (guild_id, season_id, user_id, lane) DO NOTHING
        """),
            {**p, "user": user, "lane": lane},
        )
    result = await c.execute(
        text("""
        SELECT user_id, lane, mmr, revision FROM user_lane_mmr
        WHERE guild_id=:guild AND season_id=:season AND user_id=ANY(:ids)
          AND lane=ANY(:lanes)
        ORDER BY user_id, lane FOR UPDATE
    """),
        {**p, "ids": list({user for user, _ in keys}), "lanes": list({lane for _, lane in keys})},
    )
    return [dict(row) for row in result.mappings()]


async def persist_mmr_change(c: AsyncConnection, p: dict[str, Any]) -> None:
    await c.execute(
        text("""
        INSERT INTO mmr_ledger_entries (
          guild_id, id, season_id, user_id, lane, lane_revision, source_type, source_id,
          game_id, opponent_user_id, mmr_before, mmr_after, delta, games_played_delta,
          wins_delta, reverses_entry_id, reversed_source_type, actor_type, actor_id, note,
          request_id, created_at
        ) VALUES (
          :guild, :ledger_id, :season, :user, :lane, :lane_revision, 'DATA_IMPORT_SEED',
          :import_id, NULL, NULL, :before, :after, :delta, 0, 0, NULL, NULL, 'USER',
          :actor_id, NULL, :request_id, :now
        )
    """),
        p,
    )
    updated = await c.execute(
        text("""
        UPDATE user_lane_mmr SET mmr=:after, revision=:lane_revision, updated_at=:now
        WHERE guild_id=:guild AND season_id=:season AND user_id=:user AND lane=:lane
    """),
        p,
    )
    if updated.rowcount != 1:
        raise RuntimeError("MMR row missing after seed ledger insert")


async def increment_members_revision(c: AsyncConnection, p: dict[str, Any]) -> None:
    updated = await c.execute(
        text("""
        UPDATE guild_admin_state
        SET revision=:next_revision, updated_by=:actor_id, updated_at=:now
        WHERE guild_id=:guild AND scope='MEMBERS' AND revision=:revision
    """),
        p,
    )
    if updated.rowcount != 1:
        raise RuntimeError("member revision update lost")


async def mark_applied(c: AsyncConnection, p: dict[str, Any]) -> None:
    updated = await c.execute(
        text("""
        UPDATE guild_data_imports
        SET status='APPLIED', archive_payload=NULL, applied_at=:now
        WHERE guild_id=:guild AND id=:import_id AND status='PREVIEWED'
    """),
        p,
    )
    if updated.rowcount != 1:
        raise RuntimeError("preview state update lost")


async def audit(
    c: AsyncConnection,
    p: dict[str, Any],
    action: str,
    target_type: str,
    target: str,
    before: dict[str, Any],
    after: dict[str, Any],
) -> None:
    await c.execute(
        text("""
        INSERT INTO admin_audit_log (
          guild_id, id, principal_type, principal_id, action, target_type, target_id,
          checkpoint_id, request_id, before_summary, after_summary, result, note, created_at
        ) VALUES (
          :guild, :audit_id, 'USER', :actor_id, :action, :target_type, :target,
          NULL, :request_id, CAST(:before AS jsonb), CAST(:after AS jsonb), 'APPLIED', NULL, :now
        )
    """),
        {
            **p,
            "action": action,
            "target_type": target_type,
            "target": target,
            "before": json.dumps(before),
            "after": json.dumps(after),
        },
    )
