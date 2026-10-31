"""Persistence for service status and the guarded guild record reset."""

import hashlib
import json
import uuid
from datetime import datetime, timedelta
from typing import Any, cast

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError
from monkeybot_api.schemas.service_admin import ResetResult

RESET_ROUTE = "/api/admin/guilds/{guild_id}/record-reset"
RESET_TABLES = (
    "admin_audit_log",
    "revert_checkpoints",
    "bot_notification_outbox",
    "outbox_events",
    "guild_data_imports",
    "guild_full_data_imports",
    "idempotency_records",
    "mmr_ledger_entries",
    "game_participant_stats",
    "game_bans",
    "game_participants",
    "games",
    "game_input_bans",
    "game_input_picks",
    "game_inputs",
    "auction_match_lineups",
    "auction_matches",
    "auction_bracket_slots",
    "auction_point_ledger",
    "auction_team_members",
    "auction_actions",
    "auction_lots",
    "auction_bid_candidates",
    "auction_teams",
    "auction_tournaments",
    "auction_setup_teams",
    "auction_setup_drafts",
    "auction_recruitment_participants",
    "auction_recruitments",
    "series_participants",
    "series",
    "recruitment_entry_assignments",
    "recruitment_preferred_lanes",
    "recruitment_entry_drafts",
    "recruitment_participants",
    "recruitments",
    "user_lane_mmr",
    "guild_counters",
    "seasons",
)
RESET_TOTAL_ROWS_SQL = (
    "SELECT COALESCE(sum(row_count), 0)::bigint FROM ("
    + " UNION ALL ".join(
        f"SELECT count(*)::bigint AS row_count FROM {table} WHERE guild_id = :guild"
        for table in RESET_TABLES
    )
    + ") AS reset_rows"
)


def reset_internal_error() -> ApiError:
    return ApiError(
        500,
        "record_reset_internal_error",
        "테스트 기록 초기화 상태를 저장하는 중 내부 오류가 발생했습니다.",
    )


async def latest_heartbeat(c: AsyncConnection) -> datetime | None:
    return cast(
        datetime | None,
        await c.scalar(
            text("""
            SELECT last_heartbeat_at FROM bot_instances
            ORDER BY last_heartbeat_at DESC, updated_at DESC, instance_id ASC LIMIT 1
        """)
        ),
    )


async def preview(c: AsyncConnection, guild: str, *, lock: bool) -> dict[str, Any]:
    target = (
        (
            await c.execute(
                text(
                    """
                SELECT COALESCE(config.display_name, guild.discord_name) AS display_name,
                       guild.status
                FROM guilds AS guild
                LEFT JOIN guild_config AS config ON config.guild_id = guild.guild_id
                WHERE guild.guild_id = :guild
            """
                    + (" FOR UPDATE OF guild" if lock else "")
                ),
                {"guild": guild},
            )
        )
        .mappings()
        .one_or_none()
    )
    if target is None:
        raise ApiError(404, "guild_not_found", "초기화할 서버를 찾을 수 없습니다.")
    if target["status"] != "ACTIVE":
        raise ApiError(
            409, "guild_not_active", "활성 상태인 서버만 테스트 기록을 초기화할 수 있습니다."
        )
    season = (
        (
            await c.execute(
                text("""
                SELECT name, created_by FROM seasons
                WHERE guild_id = :guild AND status = 'ACTIVE'
            """),
                {"guild": guild},
            )
        )
        .mappings()
        .one_or_none()
    )
    if season is None:
        raise ApiError(
            409, "active_season_missing", "활성 시즌이 없어 테스트 기록을 초기화할 수 없습니다."
        )
    counts = (
        (
            await c.execute(
                text("""
                SELECT
                    (SELECT count(*) FROM seasons WHERE guild_id = :guild) AS seasons,
                    (SELECT count(*) FROM recruitments WHERE guild_id = :guild) AS recruitments,
                    (SELECT count(*) FROM series WHERE guild_id = :guild) AS series,
                    (SELECT count(*) FROM games WHERE guild_id = :guild) AS games,
                    (SELECT count(*) FROM auction_recruitments WHERE guild_id = :guild)
                        AS auction_recruitments,
                    (SELECT count(*) FROM auction_tournaments WHERE guild_id = :guild)
                        AS auction_tournaments,
                    ((SELECT count(*) FROM user_lane_mmr WHERE guild_id = :guild)
                     + (SELECT count(*) FROM mmr_ledger_entries WHERE guild_id = :guild))
                        AS mmr_rows,
                    ((SELECT count(*) FROM guild_data_imports WHERE guild_id = :guild)
                     + (SELECT count(*) FROM guild_full_data_imports WHERE guild_id = :guild))
                        AS import_jobs
            """),
                {"guild": guild},
            )
        )
        .mappings()
        .one()
    )
    total_rows = await c.scalar(text(RESET_TOTAL_ROWS_SQL), {"guild": guild})
    member_count = await c.scalar(
        text("SELECT count(*) FROM guild_members WHERE guild_id=:guild AND status='ACTIVE'"),
        {"guild": guild},
    )
    return {
        "guildId": guild,
        "displayName": target["display_name"],
        "memberCount": member_count,
        "activeSeasonName": season["name"],
        "confirmation": f"RESET {guild}",
        "counts": {
            "totalRows": total_rows,
            "seasons": counts["seasons"],
            "recruitments": counts["recruitments"],
            "series": counts["series"],
            "games": counts["games"],
            "auctionRecruitments": counts["auction_recruitments"],
            "auctionTournaments": counts["auction_tournaments"],
            "mmrRows": counts["mmr_rows"],
            "importJobs": counts["import_jobs"],
        },
        "creator": season["created_by"],
    }


async def acquire_reset(
    c: AsyncConnection, guild: str, actor: str, key: str, now: datetime
) -> tuple[uuid.UUID | None, dict[str, Any] | None]:
    payload = json.dumps([RESET_ROUTE, guild], separators=(",", ":"))
    request_hash = hashlib.sha256(payload.encode()).digest()
    params = {
        "actor": actor,
        "key": key,
        "now": now,
        "expires": now + timedelta(hours=24),
        "hash": request_hash,
        "record": uuid.uuid7(),
    }
    locked = await c.scalar(
        text("""
        SELECT pg_try_advisory_xact_lock(hashtextextended(jsonb_build_array(
            'monkeybot:idempotency:v1', 'GLOBAL', 'USER', CAST(:actor AS text),
            'POST', CAST(:route AS text), CAST(:key AS text))::text, 0))
    """),
        {**params, "route": RESET_ROUTE},
    )
    if not locked:
        raise ApiError(409, "idempotency_in_progress", "같은 초기화 요청이 이미 처리 중입니다.")
    await c.execute(
        text("""
        DELETE FROM idempotency_records
        WHERE scope_key='GLOBAL' AND guild_id IS NULL AND principal_type='USER'
          AND principal_id=:actor AND method='POST' AND route_template=:route
          AND idempotency_key=:key AND expires_at <= :now
    """),
        {**params, "route": RESET_ROUTE},
    )
    inserted = await c.scalar(
        text("""
        INSERT INTO idempotency_records
            (id, guild_id, scope_key, principal_type, principal_id, method,
             route_template, idempotency_key, request_hash, state, created_at, expires_at)
        VALUES (:record, NULL, 'GLOBAL', 'USER', :actor, 'POST', :route,
                :key, :hash, 'IN_PROGRESS', :now, :expires)
        ON CONFLICT ON CONSTRAINT idempotency_records_command_key DO NOTHING RETURNING id
    """),
        {**params, "route": RESET_ROUTE},
    )
    if inserted is not None:
        return cast(uuid.UUID, inserted), None
    existing = (
        (
            await c.execute(
                text("""
                SELECT request_hash, state, status_code, response_body
                FROM idempotency_records
                WHERE scope_key='GLOBAL' AND guild_id IS NULL AND principal_type='USER'
                  AND principal_id=:actor AND method='POST' AND route_template=:route
                  AND idempotency_key=:key FOR UPDATE
            """),
                {**params, "route": RESET_ROUTE},
            )
        )
        .mappings()
        .one_or_none()
    )
    if existing is None:
        raise reset_internal_error()
    if bytes(existing["request_hash"]) != request_hash:
        raise ApiError(
            409, "idempotency_conflict", "같은 Idempotency-Key가 다른 초기화 요청에 사용되었습니다."
        )
    if existing["state"] == "IN_PROGRESS":
        raise ApiError(409, "idempotency_in_progress", "같은 초기화 요청이 이미 처리 중입니다.")
    if existing["state"] != "COMPLETED" or existing["status_code"] != 200:
        raise reset_internal_error()
    try:
        return None, ResetResult.model_validate(existing["response_body"]).model_dump()
    except ValueError:
        raise reset_internal_error() from None


async def purge_and_reseed(
    c: AsyncConnection, guild: str, name: str, creator: str | None, now: datetime
) -> tuple[int, uuid.UUID]:
    deleted = await c.scalar(
        text("SELECT public.service_admin_reset_guild_records(:guild)"), {"guild": guild}
    )
    await c.execute(
        text("""
        INSERT INTO guild_admin_state (guild_id, scope, revision, updated_by, updated_at)
        SELECT :guild, scope, 1, NULL, :now
        FROM unnest(ARRAY['MEMBERS', 'OPERATIONS']) AS scope
        ON CONFLICT (guild_id, scope) DO UPDATE
        SET revision=guild_admin_state.revision+1,
            updated_by=NULL, updated_at=EXCLUDED.updated_at
    """),
        {"guild": guild, "now": now},
    )
    season_id = uuid.uuid7()
    await c.execute(
        text("""
        INSERT INTO seasons
            (guild_id, id, name, status, started_at, ended_at, created_by, created_at)
        VALUES (:guild, :season, :name, 'ACTIVE', :now, NULL, :creator, :now)
    """),
        {"guild": guild, "season": season_id, "name": name, "creator": creator, "now": now},
    )
    for counter in ("RECRUITMENT", "SERIES", "AUCTION_RECRUITMENT", "AUCTION_TOURNAMENT"):
        await c.execute(
            text("""
            INSERT INTO guild_counters (guild_id, counter_key, next_value)
            VALUES (:guild, :counter, 1)
        """),
            {"guild": guild, "counter": counter},
        )
    return cast(int, deleted), season_id


async def complete_reset(
    c: AsyncConnection, record: uuid.UUID, result: dict[str, Any], now: datetime
) -> None:
    completed = await c.scalar(
        text("""
        UPDATE idempotency_records
        SET state='COMPLETED', status_code=200, response_body=CAST(:body AS jsonb),
            completed_at=:now
        WHERE id=:record AND guild_id IS NULL AND scope_key='GLOBAL'
          AND state='IN_PROGRESS' RETURNING id
    """),
        {"record": record, "body": json.dumps(result, ensure_ascii=False), "now": now},
    )
    if completed is None:
        raise reset_internal_error()
