import uuid

import pytest
from sqlalchemy import text

from tests.support.assertions import assert_failure
from tests.support.auctions.bidding import PATH, prepared
from tests.support.auctions.bidding import request as draft
from tests.support.auctions.bracket import assigned, ready, run
from tests.support.guilds.context import GUILD_A
from tests.support.recruitments.creation import BOT

pytestmark = pytest.mark.postgres


@pytest.mark.parametrize("count", [10, 20])
async def test_result_snapshot_current_bracket_and_member_permissions(owner, runtime_client, count):
    h, state = await assigned(owner, runtime_client, count)
    state = await run(runtime_client, h, state, "LOCK_BRACKET")
    identifier = state["tournament"]["id"]
    url = f"{PATH}/{identifier}/result"
    async with owner.begin() as c:
        await c.execute(text("UPDATE auction_tournaments SET status='IN_GAME'"))
    response = await runtime_client.get(url)
    assert response.status_code == 200, response.text
    result = response.json()["data"]
    assert set(result) == {"tournament", "teams", "matches", "players", "permissions"}
    assert result["tournament"] == {
        **state["tournament"],
        "status": "IN_GAME",
        "championTeamId": None,
        "endedEarly": False,
        "href": f"/g/{GUILD_A}/auction/tournaments/{identifier}/result",
    }
    assert len(result["teams"]) == count // 5
    assert len(result["players"]) == count
    assert all(
        p["games"] == p["wins"] == p["losses"] == 0 and p["mmrDelta"] is None
        for p in result["players"]
    )
    for team in result["teams"]:
        assert team["spentPoints"] == 0 and team["currentPoints"] == team["initialPoints"]
        assert len(team["members"]) == 5
        assert all(m["acquisitionPoints"] is None for m in team["members"])
    assert [m["id"] for m in result["matches"]] == [m["id"] for m in state["bracket"]["matches"]]
    assert all(m["games"] == m["lineups"] == [] for m in result["matches"])
    assert result["permissions"] == {
        "canRevertResults": True,
        "canRecoverBracket": True,
        "canCancel": True,
    }
    head = await runtime_client.head(url)
    assert head.status_code == 200 and head.content == b""
    async with owner.begin() as c:
        await c.execute(text("DELETE FROM guild_member_discord_roles"))
    member = (await runtime_client.get(url)).json()["data"]
    assert member == {**result, "permissions": dict.fromkeys(result["permissions"], False)}


async def test_result_cancelled_bidding_and_recovered_bracket_history(owner, runtime_client):
    h, source = await prepared(owner, runtime_client)
    state = await draft(runtime_client, h, None, source)
    url = f"{PATH}/{state['tournament']['id']}/result"
    async with owner.begin() as c:
        await c.execute(text("UPDATE auction_tournaments SET status='CANCELLED',ended_at=now()"))
    response = await runtime_client.get(url)
    assert response.status_code == 200, response.text
    result = response.json()["data"]
    assert result["matches"] == [] and len(result["players"]) == 2
    assert not any(result["permissions"].values())


async def test_result_recovery_excludes_old_matches_until_cancelled(owner, runtime_client):
    h, state = await assigned(owner, runtime_client)
    state = await run(runtime_client, h, state, "LOCK_BRACKET")
    old_id = state["bracket"]["matches"][0]["id"]
    state = await run(runtime_client, h, state, "UNLOCK_BRACKET")
    state = await run(runtime_client, h, state, "LOCK_BRACKET")
    new_id = state["bracket"]["matches"][0]["id"]
    url = f"{PATH}/{state['tournament']['id']}/result"
    async with owner.begin() as c:
        await c.execute(text("UPDATE auction_tournaments SET status='IN_GAME'"))
    response = await runtime_client.get(url)
    assert response.status_code == 200, response.text
    assert [m["id"] for m in response.json()["data"]["matches"]] == [new_id]
    async with owner.begin() as c:
        await c.execute(text("UPDATE auction_tournaments SET status='CANCELLED',ended_at=now()"))
        await c.execute(text("UPDATE auction_matches SET status='CANCELLED',ended_at=now()"))
    archived = await runtime_client.get(url)
    assert archived.status_code == 200, archived.text
    assert {m["id"] for m in archived.json()["data"]["matches"]} == {old_id, new_id}
    assert not any(archived.json()["data"]["permissions"].values())


async def test_result_state_auth_and_tenant_denials(owner, runtime_client):
    _, state = await ready(owner, runtime_client)
    url = f"{PATH}/{state['tournament']['id']}/result"
    for status in ("BRACKET_SETUP", "BIDDING"):
        async with owner.begin() as c:
            await c.execute(
                text("UPDATE auction_tournaments SET status=:status"), {"status": status}
            )
        response = await runtime_client.get(url)
        assert_failure(
            response,
            409,
            "auction_tournament_result_unavailable",
            "진행 중, 완료 또는 취소된 경매 내전에서만 기록을 확인할 수 있습니다.",
        )
        assert response.json()["error"]["details"] == {"currentStatus": status}
    response = await runtime_client.get(f"{PATH}/{uuid.uuid7()}/result")
    assert_failure(response, 404, "not_found", "요청한 경매 내전을 찾을 수 없습니다.")
    response = await runtime_client.get(url.replace(GUILD_A, "999999"))
    assert_failure(response, 404, "not_found", "요청한 길드를 찾을 수 없습니다.")
    runtime_client.cookies.clear()
    response = await runtime_client.get(url, headers={"authorization": f"Bearer {BOT}"})
    assert response.status_code == 401 and response.json()["error"]["code"] == "unauthorized"


async def test_result_rejects_incomplete_active_bracket_without_exposing_storage(
    owner, runtime_client
):
    _, state = await ready(owner, runtime_client)
    async with owner.begin() as c:
        await c.execute(text("UPDATE auction_tournaments SET status='IN_GAME'"))
    response = await runtime_client.get(f"{PATH}/{state['tournament']['id']}/result")
    assert_failure(
        response,
        500,
        "internal_error",
        "경매 내전 결과를 불러오는 중 내부 오류가 발생했습니다.",
    )
