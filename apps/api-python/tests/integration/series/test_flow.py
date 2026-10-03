import asyncio

import pytest
from sqlalchemy import text

from tests.support.assertions import assert_failure
from tests.support.guilds.context import GUILD_A
from tests.support.series import command, game, rust, setup

pytestmark = pytest.mark.postgres


async def test_series_flow_full_cross_language_pickban_submit_revert_complete(
    owner, runtime_client
):
    p, h, _ = await setup(owner, runtime_client)
    initial = await runtime_client.get(p)
    assert initial.status_code == 200, initial.text
    assert initial.json()["data"] == await rust(p, "detail")
    listed = await runtime_client.get(f"/api/guilds/{GUILD_A}/series?state=active")
    assert listed.status_code == 200, listed.text
    assert listed.json()["data"]["series"][0]["currentSetNumber"] == 1
    rev = 0
    for op in [
        {"type": "SET_SIDE", "sidePreset": "SWAPPED"},
        {"type": "SET_PICK", "playerId": "1001", "championId": "Ahri"},
        {"type": "SET_BAN", "side": "RED", "slot": 1, "championId": "Gnar"},
        {"type": "CLEAR_PICK", "playerId": "1001"},
        {"type": "CLEAR_BAN", "side": "RED", "slot": 1},
        {
            "type": "APPLY_BULK",
            "items": [
                {"type": "SET_PICK", "playerId": "1001", "championId": "Ahri"},
                {"type": "SET_BAN", "side": "BLUE", "slot": 2, "championId": "Gnar"},
            ],
        },
    ]:
        response = await command(runtime_client, p, h, "pickban", rev, body={"operation": op})
        assert response.status_code == 200, response.text
        rev += 1
        assert response.json()["data"] == await rust(p, "detail")
    # Rust executes another pick-ban mutation; Python must project it exactly.
    expected = await rust(
        p, "pickban", rev, {"type": "SET_BAN", "side": "RED", "slot": 1, "championId": "Lux"}
    )
    rev += 1
    assert (await runtime_client.get(p)).json()["data"] == expected
    body = game(side="SWAPPED", win="RED")
    response = await command(runtime_client, p, h, "submit", rev, body=body)
    assert response.status_code == 201, response.text
    assert response.json()["data"] == await rust(
        p, "submit", rev, {"expectedRevision": rev, **body}
    )
    rev += 1
    assert (await runtime_client.get(p)).json()["data"] == await rust(p, "detail")
    # Reverse games across Rust/Python in both directions.
    expected = await rust(p, "last", rev)
    response = await command(runtime_client, p, h, "last", rev)
    assert response.status_code == 200 and response.json()["data"] == expected
    rev += 1
    expected = await rust(p, "submit", rev, {"expectedRevision": rev, **game()})
    response = await command(runtime_client, p, h, "submit", rev, body=game())
    assert response.status_code == 201 and response.json()["data"] == expected
    rev += 1
    response = await command(runtime_client, p, h, "last", rev)
    assert response.status_code == 200, response.text
    assert response.json()["data"] == await rust(p, "last", rev)
    rev += 1
    async with owner.connect() as c:
        assert await c.scalar(text("SELECT mmr FROM user_lane_mmr WHERE user_id='1001'")) == 1732.5
        assert (
            await c.scalar(text("SELECT games_played FROM user_lane_mmr WHERE user_id='1001'"))
            == 12
        )
        assert (
            await c.scalar(
                text("SELECT count(*) FROM mmr_ledger_entries WHERE source_type='GAME_REVERSAL'")
            )
            == 20
        )
    for number in (1, 2):
        response = await command(runtime_client, p, h, "submit", rev, body=game(number))
        assert response.status_code == 201, response.text
        rev += 1
    result = await runtime_client.get(p + "/result")
    assert result.status_code == 200, result.text
    assert result.json()["data"] == await rust(p, "result")
    assert result.json()["data"]["score"] == {"team1": 2, "team2": 0}
    assert (await runtime_client.get(p)).status_code == 404
    listed = (await runtime_client.get(f"/api/guilds/{GUILD_A}/series?state=completed")).json()[
        "data"
    ]["series"]
    assert listed[0]["outcome"] == "TEAM_1" and listed[0]["currentSetNumber"] is None
    async with owner.connect() as c:
        payload = await c.scalar(
            text(
                "SELECT payload FROM bot_notification_outbox "
                "WHERE notification_type='SERIES_RESULT_CREATE'"
            )
        )
        assert payload["schemaVersion"] == 2
        assert payload["snapshot"]["score"] == {"team1": 2, "team2": 0}
        assert len(payload["snapshot"]["games"]) == 2
        assert payload["snapshot"]["games"][0]["winner"] == "TEAM_1"


@pytest.mark.parametrize(
    "action,rust_first", [("revert", False), ("delete", False), ("revert", True), ("delete", True)]
)
async def test_series_flow_lifecycle_rolls_back_all_games(
    owner, runtime_client, action, rust_first
):
    p, h, source = await setup(owner, runtime_client)
    for rev in (0, 1):
        response = await command(runtime_client, p, h, "submit", rev, body=game(rev + 1))
        assert response.status_code == 201, response.text
    expected = await rust(p, action, 2) if rust_first else None
    response = await command(runtime_client, p, h, action, 2)
    assert response.status_code == 200, response.text
    assert response.json()["data"] == (expected or await rust(p, action, 2))
    assert (await runtime_client.get(p)).status_code == 404
    source_data = (await runtime_client.get(source)).json()["data"]["recruitment"]
    assert source_data["status"] == ("CLOSED" if action == "revert" else "CANCELLED")
    async with owner.connect() as c:
        assert await c.scalar(text("SELECT mmr FROM user_lane_mmr WHERE user_id='1001'")) == 1732.5
        assert await c.scalar(text("SELECT count(*) FROM games WHERE status='VOIDED'")) == 2
        assert await c.scalar(text("SELECT count(*) FROM recruitment_entry_assignments")) == 10


@pytest.mark.parametrize(
    "best,outcome,rust_first", [(3, "TEAM_2", False), (5, "DRAW", False), (5, "TEAM_1", True)]
)
async def test_series_flow_early_completion(owner, runtime_client, best, outcome, rust_first):
    p, h, _ = await setup(owner, runtime_client, best)
    count = 2 if outcome == "DRAW" else 1
    for rev in range(count):
        response = await command(
            runtime_client,
            p,
            h,
            "submit",
            rev,
            body=game(rev + 1, win="BLUE" if rev == 0 else "RED"),
        )
        assert response.status_code == 201, response.text
    body = {"outcome": outcome}
    expected = (
        await rust(p, "complete", count, {"expectedRevision": count, **body})
        if rust_first
        else None
    )
    response = await command(runtime_client, p, h, "complete", count, body=body)
    assert response.status_code == 200, response.text
    assert response.json()["data"] == (
        expected or await rust(p, "complete", count, {"expectedRevision": count, **body})
    )
    result = await runtime_client.get(p + "/result")
    assert result.json()["data"] == await rust(p, "result")
    # Completed series can return to IN_PROGRESS by reversing the last confirmed game.
    response = await command(runtime_client, p, h, "last", count + 1)
    assert response.status_code == 200, response.text
    assert (await runtime_client.get(p)).json()["data"] == await rust(p, "detail")


async def test_series_flow_pickban_errors_fearless_and_concurrency(owner, runtime_client):
    p, h, _ = await setup(owner, runtime_client)
    pick = {"type": "SET_PICK", "playerId": "1001", "championId": "Ahri"}
    response = await command(runtime_client, p, h, "pickban", 0, body={"operation": pick})
    assert_failure(
        response,
        422,
        "series_game_side_required",
        "픽밴을 입력하기 전에 현재 세트의 블루/레드 사이드를 먼저 선택해 주세요.",
    )
    responses = await asyncio.gather(
        *[
            command(
                runtime_client,
                p,
                h,
                "pickban",
                0,
                body={"operation": {"type": "SET_SIDE", "sidePreset": s}},
            )
            for s in ("DEFAULT", "SWAPPED")
        ]
    )
    assert sorted(r.status_code for r in responses) == [200, 409]
    response = await command(runtime_client, p, h, "pickban", 1, body={"operation": pick})
    assert response.status_code == 200, response.text
    response = await command(
        runtime_client,
        p,
        h,
        "pickban",
        2,
        body={"operation": {"type": "SET_BAN", "side": "BLUE", "slot": 1, "championId": "Ahri"}},
    )
    assert_failure(
        response,
        422,
        "duplicate_series_game_champion",
        "같은 세트에서 픽과 밴은 같은 챔피언을 중복할 수 없습니다.",
    )
    preset = (await runtime_client.get(p)).json()["data"]["currentGame"]["sidePreset"]
    assert (
        await command(runtime_client, p, h, "submit", 2, body=game(side=preset))
    ).status_code == 201
    assert (
        await command(
            runtime_client,
            p,
            h,
            "pickban",
            3,
            body={"operation": {"type": "SET_SIDE", "sidePreset": "DEFAULT"}},
        )
    ).status_code == 200
    response = await command(runtime_client, p, h, "pickban", 4, body={"operation": pick})
    assert_failure(
        response,
        422,
        "hard_fearless_champion_used",
        "하드 피어리스에서는 이전 세트에서 픽한 챔피언을 다시 픽할 수 없습니다.",
    )


async def test_series_flow_late_failure_and_advanced_mmr_blocks_reversal(owner, runtime_client):
    p, h, _ = await setup(owner, runtime_client)
    async with owner.begin() as c:
        await c.execute(text("REVOKE INSERT ON admin_audit_log FROM fastapi_test_runtime"))
    try:
        response = await command(runtime_client, p, h, "submit", 0, body=game())
        assert_failure(
            response, 500, "internal_error", "요청을 처리하는 중 내부 오류가 발생했습니다."
        )
        async with owner.connect() as c:
            assert await c.scalar(text("SELECT count(*) FROM games")) == 0
            assert await c.scalar(text("SELECT count(*) FROM mmr_ledger_entries")) == 0
            assert await c.scalar(text("SELECT revision FROM series")) == 0
    finally:
        async with owner.begin() as c:
            await c.execute(text("GRANT INSERT ON admin_audit_log TO fastapi_test_runtime"))
    assert (await command(runtime_client, p, h, "submit", 0, body=game())).status_code == 201
    async with owner.begin() as c:
        await c.execute(text("UPDATE user_lane_mmr SET mmr=mmr+1 WHERE user_id='1001'"))
    response = await command(runtime_client, p, h, "last", 1)
    assert_failure(
        response,
        409,
        "series_game_reversal_not_safe",
        "이후 MMR 변경이 있어 게임 결과를 안전하게 되돌릴 수 없습니다.",
    )
    async with owner.connect() as c:
        assert await c.scalar(text("SELECT count(*) FROM games WHERE status='CONFIRMED'")) == 1


async def test_series_flow_bo1_and_result_read_rules(owner, runtime_client):
    p, h, _ = await setup(owner, runtime_client, 1)
    response = await runtime_client.get(p + "/result")
    assert_failure(
        response,
        409,
        "series_result_unavailable",
        "시리즈가 아직 완료되지 않아 결과를 확인할 수 없습니다.",
    )
    response = await command(runtime_client, p, h, "complete", 0, body={"outcome": "TEAM_1"})
    assert_failure(
        response,
        409,
        "series_early_completion_unavailable",
        "BO3 또는 BO5에서 1경기 이상 확정한 뒤에만 조기 종료할 수 있습니다.",
    )
    picks = {str(user): None for user in range(1001, 1011)}
    response = await command(runtime_client, p, h, "submit", 0, body=game(picks=picks))
    assert response.status_code == 201, response.text
    assert response.json()["data"]["series"]["status"] == "COMPLETED"
    assert (await runtime_client.get(p + "/result")).json()["data"] == await rust(p, "result")
    async with owner.connect() as c:
        assert await c.scalar(text("SELECT count(*) FROM mmr_ledger_entries")) == 10


async def test_series_flow_operator_and_csrf_denials(owner, runtime_client):
    p, h, _ = await setup(owner, runtime_client)
    response = await runtime_client.post(p + "/games", json={"expectedRevision": 0, **game()})
    assert response.status_code == 403
    async with owner.begin() as c:
        await c.execute(text("DELETE FROM guild_member_discord_roles"))
    response = await command(runtime_client, p, h, "submit", 0, body=game())
    assert_failure(response, 403, "operator_required", "길드 운영자 권한이 필요합니다.")
    assert not (await runtime_client.get(p)).json()["data"]["capabilities"]["canEdit"]
    async with owner.connect() as c:
        assert await c.scalar(text("SELECT count(*) FROM game_inputs")) == 0


@pytest.mark.parametrize(
    "operation",
    [
        {"type": "SET_SIDE", "sidePreset": "invalid"},
        {"type": "SET_PICK", "playerId": "1001", "championId": "한글"},
        {"type": "APPLY_BULK", "items": []},
        {"type": "SET_BAN", "side": "BLUE", "slot": 6, "championId": "Ahri"},
    ],
)
async def test_series_flow_operation_validation(owner, runtime_client, operation):
    p, h, _ = await setup(owner, runtime_client)
    response = await command(runtime_client, p, h, "pickban", 0, body={"operation": operation})
    assert_failure(
        response, 422, "invalid_series_pickban_request", "픽밴 변경 요청 형식이 올바르지 않습니다."
    )
    async with owner.connect() as c:
        assert await c.scalar(text("SELECT revision FROM series")) == 0
        assert await c.scalar(text("SELECT count(*) FROM game_inputs")) == 0


async def test_series_flow_list_cursor_boundary_and_ignored_season(owner, runtime_client):
    import uuid

    from tests.support.recruitments.creation import BODY, PATH

    p, h, _ = await setup(owner, runtime_client)
    created = await runtime_client.post(
        PATH, json=BODY, headers={**h, "idempotency-key": "list-source"}
    )
    assert created.status_code == 201
    source = uuid.UUID(created.json()["data"]["recruitment"]["id"])
    second = uuid.uuid7()
    async with owner.begin() as c:
        await c.execute(text("UPDATE series SET updated_at='2026-09-01T00:00:00Z'"))
        await c.execute(
            text("""
            INSERT INTO series (guild_id,id,season_id,source_recruitment_id,display_number,
                title,best_of,hard_fearless,status,revision,started_at,created_by,updated_by,updated_at)
            SELECT guild_id,:second,season_id,:source,2,'Second',best_of,hard_fearless,
                status,0,started_at,created_by,updated_by,updated_at FROM series WHERE id=:first
        """),
            {"second": second, "source": source, "first": uuid.UUID(p.split("/")[-1])},
        )
    url = f"/api/guilds/{GUILD_A}/series"
    first = await runtime_client.get(
        url, params={"state": "active", "limit": 1, "seasonId": "ignored"}
    )
    assert first.status_code == 200, first.text
    data = first.json()["data"]
    assert data["series"][0]["id"] == str(second) and data["nextCursor"]
    next_page = await runtime_client.get(
        url, params={"state": "active", "limit": 1, "cursor": data["nextCursor"]}
    )
    assert next_page.status_code == 200
    assert next_page.json()["data"]["series"][0]["id"] == p.split("/")[-1]
    assert next_page.json()["data"]["nextCursor"] is None
