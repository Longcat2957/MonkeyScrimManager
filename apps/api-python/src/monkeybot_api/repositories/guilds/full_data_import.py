"""Persistence and archive restoration for complete guild imports."""

import json
import uuid
from dataclasses import dataclass
from datetime import datetime
from typing import Any

from sqlalchemy import text
from sqlalchemy.exc import DBAPIError
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError
from monkeybot_api.repositories.guilds.full_data_transfer import ARCHIVE_TABLE_NAMES

DOMAIN_TABLES = (
    "seasons",
    "recruitments",
    "recruitment_participants",
    "recruitment_preferred_lanes",
    "recruitment_entry_drafts",
    "recruitment_entry_assignments",
    "series",
    "series_participants",
    "auction_recruitments",
    "auction_recruitment_participants",
    "auction_setup_drafts",
    "auction_setup_teams",
    "auction_tournaments",
    "auction_teams",
    "auction_bid_candidates",
    "auction_lots",
    "auction_actions",
    "auction_team_members",
    "auction_point_ledger",
    "auction_bracket_slots",
    "auction_matches",
    "auction_match_lineups",
    "game_inputs",
    "game_input_picks",
    "game_input_bans",
    "games",
    "game_participants",
    "game_participant_stats",
    "game_bans",
    "user_lane_mmr",
    "mmr_ledger_entries",
)
ACTIVE_SEASON_TABLES = (
    "recruitments",
    "series",
    "auction_recruitments",
    "auction_tournaments",
    "auction_matches",
    "games",
    "user_lane_mmr",
    "mmr_ledger_entries",
)
BEFORE_TOURNAMENT_TEAMS = (
    "users",
    "user_discord_identities",
    "user_privacy_settings",
    "user_champion_preferences",
    "riot_accounts",
    "dummy_user_profiles",
    "guild_members",
    "user_guild_preferences",
    "guild_counters",
    "seasons",
    "recruitments",
    "recruitment_participants",
    "recruitment_preferred_lanes",
    "recruitment_entry_drafts",
    "recruitment_entry_assignments",
    "series",
    "series_participants",
    "auction_recruitments",
    "auction_recruitment_participants",
    "auction_setup_drafts",
    "auction_setup_teams",
)
AFTER_TOURNAMENT_TEAMS = (
    "auction_bid_candidates",
    "auction_lots",
    "auction_actions",
    "auction_team_members",
    "auction_point_ledger",
    "auction_bracket_slots",
    "auction_matches",
    "auction_match_lineups",
    "game_inputs",
    "game_input_picks",
    "game_input_bans",
    "games",
    "game_participants",
    "game_participant_stats",
    "game_bans",
    "user_lane_mmr",
    "mmr_ledger_entries",
    "admin_audit_log",
)
COUNTER_NAMESPACES = (
    ("recruitments", "RECRUITMENT"),
    ("series", "SERIES"),
    ("auction_recruitments", "AUCTION_RECRUITMENT"),
    ("auction_tournaments", "AUCTION_TOURNAMENT"),
)


@dataclass(frozen=True)
class TargetState:
    domain_rows: int
    active_domain_rows: int
    active_season_id: uuid.UUID | None
    display_number_bases: dict[str, int]
    members: list[dict[str, Any]]
    counters: list[dict[str, Any]]
    admin_state: list[dict[str, Any]]
    seasons: list[dict[str, Any]]

    @property
    def importable(self) -> bool:
        return self.domain_rows == 0 or (
            self.active_season_id is not None and self.active_domain_rows == 0
        )


async def lock_guild(c: AsyncConnection, guild: str) -> None:
    found = await c.scalar(
        text("SELECT true FROM guilds WHERE guild_id=:guild AND status='ACTIVE' FOR UPDATE"),
        {"guild": guild},
    )
    if found is not True:
        raise ApiError(404, "not_found", "요청한 길드를 찾을 수 없습니다.")


async def _count_tables(
    c: AsyncConnection, guild: str, tables: tuple[str, ...], season_id: uuid.UUID | None = None
) -> int:
    # Table names are a closed, source-controlled allowlist.
    where = "guild_id=:guild" + (" AND season_id=:season" if season_id else "")
    union = " UNION ALL ".join(
        f"SELECT count(*)::bigint AS row_count FROM {table} WHERE {where}" for table in tables
    )
    value = await c.scalar(
        text(f"SELECT COALESCE(sum(row_count), 0)::bigint FROM ({union}) AS counts"),
        {"guild": guild, "season": season_id},
    )
    return int(value)


async def _json_rows(c: AsyncConnection, query: str, guild: str) -> list[dict[str, Any]]:
    value = await c.scalar(text(query), {"guild": guild})
    try:
        rows = json.loads(value)
    except TypeError, ValueError:
        raise ApiError.from_status(500) from None
    if not isinstance(rows, list) or not all(isinstance(row, dict) for row in rows):
        raise ApiError.from_status(500)
    return rows


async def target_state(c: AsyncConnection, guild: str) -> TargetState:
    domain_rows = await _count_tables(c, guild, DOMAIN_TABLES)
    active_season_id = await c.scalar(
        text("""
        SELECT id FROM seasons WHERE guild_id=:guild
        AND status='ACTIVE' AND ended_at IS NULL
    """),
        {"guild": guild},
    )
    active_domain_rows = (
        await _count_tables(c, guild, ACTIVE_SEASON_TABLES, active_season_id)
        if active_season_id
        else 0
    )
    bases = {}
    for table, key in COUNTER_NAMESPACES:
        value = await c.scalar(
            text(f"""
            SELECT GREATEST(
                COALESCE((SELECT max(display_number) FROM {table} WHERE guild_id=:guild), 0),
                COALESCE((SELECT max(next_value - 1) FROM guild_counters
                    WHERE guild_id=:guild AND counter_key=:key), 0)
            )::bigint
        """),
            {"guild": guild, "key": key},
        )
        bases[key] = int(value)
    members = await _json_rows(
        c,
        """
        SELECT COALESCE(jsonb_agg(to_jsonb(row_data) ORDER BY row_data.user_id), '[]'::jsonb)::text
        FROM (SELECT user_id, status, main_lane, registered_at, inactivated_at
              FROM guild_members WHERE guild_id=:guild) AS row_data
    """,
        guild,
    )
    counters = await _json_rows(
        c,
        """
        SELECT COALESCE(
            jsonb_agg(to_jsonb(row_data) ORDER BY row_data.counter_key), '[]'::jsonb
        )::text
        FROM (SELECT counter_key, next_value FROM guild_counters
              WHERE guild_id=:guild) AS row_data
    """,
        guild,
    )
    admin_state = await _json_rows(
        c,
        """
        SELECT COALESCE(jsonb_agg(to_jsonb(row_data) ORDER BY row_data.scope), '[]'::jsonb)::text
        FROM (SELECT scope, revision FROM guild_admin_state
              WHERE guild_id=:guild) AS row_data
    """,
        guild,
    )
    seasons = await _json_rows(
        c,
        """
        SELECT COALESCE(jsonb_agg(to_jsonb(row_data) ORDER BY row_data.id), '[]'::jsonb)::text
        FROM (SELECT id, name, status, started_at, ended_at, created_by, created_at
              FROM seasons WHERE guild_id=:guild) AS row_data
    """,
        guild,
    )
    return TargetState(
        domain_rows,
        active_domain_rows,
        active_season_id,
        bases,
        members,
        counters,
        admin_state,
        seasons,
    )


async def store_preview(c: AsyncConnection, p: dict[str, Any]) -> None:
    await c.execute(
        text("""
        INSERT INTO guild_full_data_imports (
            guild_id, id, actor_id, status, archive_digest, archive_payload,
            source_guild_id, target_state_digest, summary, conflicts, conflict_count,
            request_id, created_at, expires_at, applied_at
        ) VALUES (
            :guild, :import_id, :actor_id, 'PREVIEWED', :archive_digest,
            CAST(:archive_payload AS jsonb), :source_guild_id, :target_digest,
            CAST(:summary AS jsonb), CAST(:conflicts AS jsonb), :conflict_count,
            :request_id, :now, :expires, NULL
        )
    """),
        p,
    )


async def lock_preview(c: AsyncConnection, guild: str, import_id: uuid.UUID) -> dict[str, Any]:
    row = (
        (
            await c.execute(
                text("""
        SELECT actor_id, status, archive_digest, archive_payload, source_guild_id,
               target_state_digest, summary, conflict_count, expires_at
        FROM guild_full_data_imports
        WHERE guild_id=:guild AND id=:import_id FOR UPDATE
    """),
                {"guild": guild, "import_id": import_id},
            )
        )
        .mappings()
        .one_or_none()
    )
    if row is None:
        raise ApiError(
            404,
            "preview_not_found",
            "전체 데이터 가져오기 검토 결과를 찾을 수 없습니다. 파일을 다시 검증해 주세요.",
        )
    return dict(row)


async def mark_applied(c: AsyncConnection, guild: str, import_id: uuid.UUID, now: datetime) -> None:
    changed = await c.execute(
        text("""
        UPDATE guild_full_data_imports
        SET status='APPLIED', archive_payload=NULL, applied_at=:now
        WHERE guild_id=:guild AND id=:import_id AND status='PREVIEWED'
    """),
        {"guild": guild, "import_id": import_id, "now": now},
    )
    if changed.rowcount != 1:
        raise ApiError.from_status(500)


async def validate_insert(c: AsyncConnection, archive: dict[str, Any]) -> None:
    # Nested transaction rolls back every attempted row, including global user inserts.
    nested = await c.begin_nested()
    try:
        await import_rows(c, archive)
    finally:
        await nested.rollback()


async def import_rows(c: AsyncConnection, archive: dict[str, Any]) -> None:
    tables = archive["tables"]
    for table in BEFORE_TOURNAMENT_TEAMS:
        await insert_table(c, table, tables[table])
    terminal = tables["auction_tournaments"]
    staged = [
        {
            **row,
            "status": "BIDDING",
            "champion_team_id": None,
            "ended_at": None,
            "ended_early": False,
        }
        for row in terminal
    ]
    await insert_table(c, "auction_tournaments", staged)
    await insert_table(c, "auction_teams", tables["auction_teams"])
    if terminal:
        try:
            await c.execute(
                text("""
                UPDATE auction_tournaments AS target
                SET status=source.status, champion_team_id=source.champion_team_id,
                    ended_at=source.ended_at, ended_early=COALESCE(source.ended_early, false)
                FROM jsonb_populate_recordset(NULL::auction_tournaments,
                     CAST(:payload AS jsonb)) AS source
                WHERE target.guild_id=source.guild_id AND target.id=source.id
            """),
                {"payload": json.dumps(terminal)},
            )
        except DBAPIError:
            raise constraint_error("auction_tournaments") from None
    for table in AFTER_TOURNAMENT_TEAMS:
        await insert_table(c, table, tables[table])


def constraint_error(table: str) -> ApiError:
    if table in {
        "user_champion_preferences",
        "game_input_picks",
        "game_input_bans",
        "game_participants",
        "game_bans",
    }:
        message = "전체 백업의 챔피언 ID에 일반 게임에서 사용할 수 없는 값이 포함되어 있습니다."
    else:
        label = table_label(table)
        suffix = "관리자 감사 기록이" if table == "admin_audit_log" else f"{label} 데이터가"
        message = f"전체 백업의 {suffix} 현재 MonkeyBot 데이터 규칙과 맞지 않습니다."
    return ApiError(422, "full_archive_constraint_violation", message, {"table": table})


def table_label(table: str) -> str:
    if table in {"users", "user_discord_identities", "guild_members"}:
        return "사용자·멤버"
    if table in {"riot_accounts", "user_champion_preferences"}:
        return "Riot 계정·챔피언 선호"
    if table in {"seasons", "user_lane_mmr", "mmr_ledger_entries"}:
        return "시즌·MMR 이력"
    if table in {
        "recruitments",
        "recruitment_participants",
        "recruitment_preferred_lanes",
        "recruitment_entry_drafts",
        "recruitment_entry_assignments",
    }:
        return "일반 모집"
    if table in {"series", "series_participants"}:
        return "일반 내전"
    if table in {"game_inputs", "game_input_picks", "game_input_bans"}:
        return "픽·밴 초안"
    if table in {"games", "game_participants", "game_participant_stats", "game_bans"}:
        return "경기 전적"
    if table.startswith("auction_"):
        return "경매"
    if table == "admin_audit_log":
        return "관리자 감사 기록"
    return "보관"


async def insert_table(c: AsyncConnection, table: str, rows: list[dict[str, Any]]) -> None:
    if not rows:
        return
    if table not in ARCHIVE_TABLE_NAMES:
        raise ApiError.from_status(500)
    if table in {"auction_tournaments", "auction_matches"}:
        rows = [{"ended_early": False, **row} for row in rows]
    recordset = f"jsonb_populate_recordset(NULL::{table}, CAST(:payload AS jsonb))"
    if table in {
        "users",
        "user_discord_identities",
        "user_privacy_settings",
        "dummy_user_profiles",
        "user_guild_preferences",
    }:
        query = f"INSERT INTO {table} SELECT * FROM {recordset} ON CONFLICT (user_id) DO NOTHING"
    elif table == "user_champion_preferences":
        query = f"""
            WITH source AS (SELECT * FROM {recordset})
            INSERT INTO user_champion_preferences SELECT source.* FROM source
            WHERE NOT EXISTS (
                SELECT 1 FROM user_champion_preferences AS existing
                WHERE existing.user_id=source.user_id AND existing.lane=source.lane
            ) ON CONFLICT (user_id, lane, position) DO NOTHING
        """
    elif table == "riot_accounts":
        columns = (
            "puuid, user_id, platform, routing_region, game_name, tag_line, "
            "normalized_game_name, normalized_tag_line, profile_icon_id, is_main, "
            "main_position, main_position_updated_at, last_synced_at, "
            "created_at, updated_at, origin"
        )
        query = f"""
            WITH source AS (SELECT {columns} FROM {recordset})
            INSERT INTO riot_accounts ({columns})
            SELECT {columns} FROM source
            WHERE NOT EXISTS (
                SELECT 1 FROM riot_accounts AS existing WHERE existing.user_id=source.user_id
            ) AND NOT EXISTS (
                SELECT 1 FROM riot_accounts AS existing WHERE existing.puuid=source.puuid
            ) AND CASE WHEN source.origin='RIOT' THEN NOT EXISTS (
                SELECT 1 FROM riot_accounts AS existing
                WHERE existing.origin='RIOT'
                  AND existing.routing_region=source.routing_region
                  AND existing.normalized_game_name=source.normalized_game_name
                  AND existing.normalized_tag_line=source.normalized_tag_line
            ) ELSE true END
            ON CONFLICT (puuid) DO NOTHING
        """
    elif table == "guild_members":
        query = (
            f"INSERT INTO guild_members SELECT * FROM {recordset} "
            "ON CONFLICT (guild_id, user_id) DO NOTHING"
        )
    elif table == "guild_counters":
        query = f"""
            INSERT INTO guild_counters SELECT * FROM {recordset}
            ON CONFLICT (guild_id, counter_key) DO UPDATE
            SET next_value=GREATEST(guild_counters.next_value, EXCLUDED.next_value)
        """
    elif table == "admin_audit_log":
        query = (
            f"INSERT INTO admin_audit_log SELECT * FROM {recordset} "
            "ON CONFLICT (guild_id, id) DO NOTHING"
        )
    else:
        query = f"INSERT INTO {table} SELECT * FROM {recordset}"
    try:
        await c.execute(text(query), {"payload": json.dumps(rows, ensure_ascii=False)})
    except DBAPIError:
        raise constraint_error(table) from None
