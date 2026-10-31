"""Complete guild data-transfer archive queries."""

import json
from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

ARCHIVE_TABLE_NAMES = (
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
    "admin_audit_log",
)

GUILD_SCOPED_TABLE_NAMES = (
    "guild_members",
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
)


async def guild_display_name(c: AsyncConnection, guild: str) -> str | None:
    value = await c.scalar(
        text("SELECT display_name FROM guild_config WHERE guild_id = :guild"),
        {"guild": guild},
    )
    return value if isinstance(value, str) else None


async def export_tables(c: AsyncConnection, guild: str) -> dict[str, list[dict[str, Any]]]:
    tables: dict[str, list[dict[str, Any]]] = {}
    for table in (
        "users",
        "user_discord_identities",
        "user_privacy_settings",
        "user_champion_preferences",
        "dummy_user_profiles",
    ):
        tables[table] = await export_member_global_table(c, guild, table)
    tables["riot_accounts"] = await export_riot_accounts(c, guild)
    tables["user_guild_preferences"] = await export_user_guild_preferences(c, guild)
    for table in GUILD_SCOPED_TABLE_NAMES:
        tables[table] = await export_guild_table(c, guild, table)
    tables["mmr_ledger_entries"] = await export_guild_table(c, guild, "mmr_ledger_entries")
    tables["admin_audit_log"] = await export_admin_audit(c, guild)
    return {name: tables[name] for name in sorted(tables)}


async def export_member_global_table(
    c: AsyncConnection, guild: str, table: str
) -> list[dict[str, Any]]:
    query = f"""
        WITH selected_users AS (
             SELECT user_id FROM guild_members WHERE guild_id = :guild
             UNION
             SELECT profile.created_by
             FROM dummy_user_profiles AS profile
             JOIN guild_members AS member ON member.user_id = profile.user_id
             WHERE member.guild_id = :guild
             UNION
             SELECT profile.updated_by
             FROM dummy_user_profiles AS profile
             JOIN guild_members AS member ON member.user_id = profile.user_id
             WHERE member.guild_id = :guild
         )
         SELECT COALESCE(
             jsonb_agg(to_jsonb(row_data) ORDER BY to_jsonb(row_data)::text),
             '[]'::jsonb
         )::text
         FROM (
             SELECT source.*
             FROM {table} AS source
             JOIN selected_users ON selected_users.user_id = source.user_id
         ) AS row_data
    """
    return await json_array_query(c, query, guild)


async def export_riot_accounts(c: AsyncConnection, guild: str) -> list[dict[str, Any]]:
    query = """
        WITH selected_users AS (
             SELECT user_id FROM guild_members WHERE guild_id = :guild
         )
         SELECT COALESCE(
             jsonb_agg(to_jsonb(row_data) ORDER BY row_data.user_id, row_data.puuid),
             '[]'::jsonb
         )::text
         FROM (
             SELECT account.puuid, account.user_id, account.platform, account.routing_region,
                    account.game_name, account.tag_line, account.normalized_game_name,
                    account.normalized_tag_line, account.profile_icon_id, account.is_main,
                    account.main_position, account.main_position_updated_at,
                    account.last_synced_at, account.created_at, account.updated_at, account.origin
             FROM riot_accounts AS account
             JOIN selected_users ON selected_users.user_id = account.user_id
         ) AS row_data
    """
    return await json_array_query(c, query, guild)


async def export_user_guild_preferences(c: AsyncConnection, guild: str) -> list[dict[str, Any]]:
    query = """
        SELECT COALESCE(
             jsonb_agg(to_jsonb(row_data) ORDER BY row_data.user_id),
             '[]'::jsonb
         )::text
         FROM (
             SELECT preference.*
             FROM user_guild_preferences AS preference
             JOIN guild_members AS member ON member.user_id = preference.user_id
             WHERE member.guild_id = :guild AND preference.default_guild_id = :guild
         ) AS row_data
    """
    return await json_array_query(c, query, guild)


async def export_guild_table(c: AsyncConnection, guild: str, table: str) -> list[dict[str, Any]]:
    query = f"""
        SELECT COALESCE(
             jsonb_agg(to_jsonb(row_data) ORDER BY to_jsonb(row_data)::text),
             '[]'::jsonb
         )::text
         FROM (SELECT * FROM {table} WHERE guild_id = :guild) AS row_data
    """
    return await json_array_query(c, query, guild)


async def export_admin_audit(c: AsyncConnection, guild: str) -> list[dict[str, Any]]:
    query = """
        SELECT COALESCE(
             jsonb_agg(to_jsonb(row_data) ORDER BY row_data.created_at, row_data.id),
             '[]'::jsonb
         )::text
         FROM (
             SELECT guild_id, id, principal_type, principal_id, action, target_type, target_id,
                    NULL::uuid AS checkpoint_id, request_id, before_summary, after_summary,
                    result, note, created_at
             FROM admin_audit_log
             WHERE guild_id = :guild
         ) AS row_data
    """
    return await json_array_query(c, query, guild)


async def json_array_query(c: AsyncConnection, query: str, guild: str) -> list[dict[str, Any]]:
    value = await c.scalar(text(query), {"guild": guild})
    rows = json.loads(value)
    if not isinstance(rows, list) or not all(isinstance(row, dict) for row in rows):
        raise ValueError("archive query did not return a JSON object array")
    return rows


async def insert_audit(c: AsyncConnection, p: dict[str, Any]) -> None:
    await c.execute(
        text("""
        INSERT INTO admin_audit_log (
             guild_id, id, principal_type, principal_id, action, target_type, target_id,
             checkpoint_id, request_id, before_summary, after_summary, result, note, created_at
        ) VALUES (
             :guild, :audit_id, 'USER', :actor_id, 'FULL_DATA_TRANSFER_EXPORTED',
             'GUILD_FULL_SNAPSHOT', :guild, NULL, :request_id,
             CAST(:before AS jsonb), CAST(:after AS jsonb), 'APPLIED', NULL, :now
        )
    """),
        p,
    )
