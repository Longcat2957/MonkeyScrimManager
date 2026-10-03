import pytest
from sqlalchemy import text

from monkeybot_api.repositories.guilds import full_data_transfer as repository
from tests.support.assertions import assert_failure
from tests.support.guilds.context import GUILD_A, GUILD_B, ROLE_A, USER_ID
from tests.support.guilds.full_data_transfer import audit_actions, export, seed_rows, setup

pytestmark = pytest.mark.postgres


async def test_full_data_export_returns_portable_snapshot_and_records_audit(owner, runtime_client):
    headers = await setup(owner, runtime_client)
    seeded = await seed_rows(owner)
    assert_failure(
        await runtime_client.post(f"/api/guilds/{GUILD_B}/full-data-export", headers=headers),
        404,
        "not_found",
        "요청한 길드를 찾을 수 없습니다.",
    )
    response = await export(runtime_client, headers)
    assert response.status_code == 200, response.text
    data = response.json()["data"]
    archive = data["archive"]
    tables = archive["tables"]
    assert archive["format"] == "MONKEYBOT_DATA_TRANSFER"
    assert archive["kind"] == "GUILD_FULL_SNAPSHOT"
    assert archive["schemaVersion"] == 1
    assert archive["source"] == {"guildId": GUILD_A, "guildName": "Export guild"}
    assert data["fileName"] == f"monkeybot-full-{GUILD_A}-v1.json"
    assert set(tables) == {
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
    }
    assert {row["user_id"] for row in tables["guild_members"]} == {USER_ID, seeded["dummy"]}
    referenced_users = {USER_ID, seeded["creator"], seeded["updater"]}
    assert {row["user_id"] for row in tables["users"]} == referenced_users | {seeded["dummy"]}
    for name in ("user_discord_identities", "user_privacy_settings", "user_champion_preferences"):
        assert {row["user_id"] for row in tables[name]} == referenced_users
    assert {row["user_id"] for row in tables["riot_accounts"]} == {USER_ID, seeded["dummy"]}
    assert {row["origin"] for row in tables["riot_accounts"]} == {"RIOT", "DUMMY"}
    assert all("owner_identity_kind" not in row for row in tables["riot_accounts"])
    assert tables["dummy_user_profiles"][0]["created_by"] == seeded["creator"]
    assert tables["dummy_user_profiles"][0]["updated_by"] == seeded["updater"]
    assert [row["default_guild_id"] for row in tables["user_guild_preferences"]] == [GUILD_A]
    assert [row["id"] for row in tables["recruitments"]] == [seeded["recruitment"]]
    assert [row["id"] for row in tables["series"]] == [seeded["series"]]
    assert [row["action"] for row in tables["admin_audit_log"]] == ["TEST_SEED"]
    assert tables["admin_audit_log"][0]["checkpoint_id"] is None
    assert data["summary"]["users"] == 4
    assert data["summary"]["members"] == 2
    assert data["summary"]["recruitments"] == 1
    assert data["summary"]["series"] == 1
    assert data["summary"]["auditEntries"] == 1
    assert data["summary"]["totalRows"] == sum(len(rows) for rows in tables.values())
    assert await audit_actions(owner) == ["TEST_SEED", "FULL_DATA_TRANSFER_EXPORTED"]
    async with owner.connect() as c:
        audit = (
            (
                await c.execute(
                    text("""
                    SELECT request_id, after_summary FROM admin_audit_log
                    WHERE guild_id=:guild AND action='FULL_DATA_TRANSFER_EXPORTED'
                """),
                    {"guild": GUILD_A},
                )
            )
            .mappings()
            .one()
        )
    assert audit["request_id"] == response.headers["x-request-id"]
    assert audit["after_summary"] == data["summary"]


async def test_full_data_export_requires_csrf_and_operator(owner, runtime_client):
    assert_failure(await export(runtime_client, {}), 401, "unauthorized", "인증이 필요합니다.")
    headers = await setup(owner, runtime_client)
    assert_failure(
        await export(
            runtime_client,
            {key: value for key, value in headers.items() if key != "x-csrf-token"},
        ),
        403,
        "csrf_validation_failed",
        "요청 출처를 확인할 수 없습니다.",
    )
    async with owner.begin() as c:
        await c.execute(
            text("""
                DELETE FROM guild_member_discord_roles
                WHERE guild_id=:guild AND user_id=:user AND role_id=:role
            """),
            {"guild": GUILD_A, "user": USER_ID, "role": ROLE_A},
        )
    assert_failure(
        await export(runtime_client, headers),
        403,
        "operator_required",
        "길드 운영자 권한이 필요합니다.",
    )
    assert await audit_actions(owner) == []


@pytest.mark.parametrize("state", ["missing_guild", "inactive_member", "disabled_guild"])
async def test_full_data_export_hides_inaccessible_guilds(owner, runtime_client, state):
    headers = await setup(owner, runtime_client)
    guild = GUILD_A
    async with owner.begin() as c:
        if state == "missing_guild":
            guild = GUILD_B
        elif state == "inactive_member":
            await c.execute(
                text("""
                    UPDATE guild_members SET status='INACTIVE'
                    WHERE guild_id=:guild AND user_id=:user
                """),
                {"guild": GUILD_A, "user": USER_ID},
            )
        else:
            await c.execute(
                text("UPDATE guilds SET status='DISABLED' WHERE guild_id=:guild"),
                {"guild": GUILD_A},
            )
    assert_failure(
        await runtime_client.post(f"/api/guilds/{guild}/full-data-export", headers=headers),
        404,
        "not_found",
        "요청한 길드를 찾을 수 없습니다.",
    )
    assert await audit_actions(owner) == []


async def test_full_data_export_invalid_snapshot_rolls_back_without_audit(
    owner, runtime_client, monkeypatch
):
    headers = await setup(owner, runtime_client)
    original_export = repository.export_tables

    async def inconsistent_snapshot(c, guild):
        tables = await original_export(c, guild)
        await c.execute(
            text("UPDATE guild_config SET display_name='Uncommitted' WHERE guild_id=:guild"),
            {"guild": guild},
        )
        tables["seasons"][0]["guild_id"] = GUILD_B
        return tables

    monkeypatch.setattr(repository, "export_tables", inconsistent_snapshot)
    response = await export(runtime_client, headers)
    assert_failure(
        response,
        400,
        "invalid_full_archive",
        "지원되는 MonkeyBot 전체 데이터 백업 파일 형식이 아닙니다.",
    )
    assert response.json()["error"]["details"] == {
        "kind": "GUILD_FULL_SNAPSHOT",
        "schemaVersion": 1,
    }
    assert await audit_actions(owner) == []
    async with owner.connect() as c:
        assert (
            await c.scalar(
                text("SELECT display_name FROM guild_config WHERE guild_id=:guild"),
                {"guild": GUILD_A},
            )
            == "Export guild"
        )


async def test_full_data_export_rejects_empty_guild_name_without_audit(owner, runtime_client):
    headers = await setup(owner, runtime_client)
    async with owner.begin() as c:
        await c.execute(
            text("UPDATE guild_config SET display_name='' WHERE guild_id=:guild"),
            {"guild": GUILD_A},
        )
    assert_failure(
        await export(runtime_client, headers),
        500,
        "internal_error",
        "요청을 처리하는 중 내부 오류가 발생했습니다.",
    )
    assert await audit_actions(owner) == []
