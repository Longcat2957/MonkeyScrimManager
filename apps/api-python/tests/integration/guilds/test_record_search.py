import base64
import json
import uuid

import pytest
from sqlalchemy import text

from tests.support.assertions import assert_failure
from tests.support.guilds.context import (
    GUILD_B,
    ROLE_B,
    USER_ID,
    authenticated_setup,
    guild_fixture,
)
from tests.support.guilds.record_search import PATH, seeded

pytestmark = pytest.mark.postgres


async def search(client, **parameters):
    response = await client.get(PATH, params={"q": USER_ID, **parameters})
    assert response.status_code == 200, response.text
    body = response.json()
    assert body["meta"]["requestId"] == response.headers["x-request-id"]
    return body["data"]


async def test_search_all_formats_grouping_details_ledger_and_keyset(owner, runtime_client):
    fixture = await seeded(owner, runtime_client)
    data = await search(runtime_client)
    assert data["selectedSeasonId"] == str(fixture["season"])
    assert data["summary"] == {"games": 4, "wins": 4, "losses": 0}
    assert [lane["lane"] for lane in data["laneMmr"]] == [
        "TOP",
        "JUNGLE",
        "MID",
        "BOTTOM",
        "SUPPORT",
    ]
    assert data["laneMmr"][0] == {"lane": "TOP", "mmr": 1515.0, "games": 4, "wins": 3}
    assert all(row["mmr"] == 1500 and row["games"] == 0 for row in data["laneMmr"][1:])
    assert data["championRecords"] == [
        {"championId": "Ahri", "lane": "TOP", "games": 4, "wins": 4, "losses": 0}
    ]
    tournament, auction, regular = data["records"]
    assert [row["kind"] for row in data["records"]] == ["TOURNAMENT", "SERIES", "SERIES"]
    assert tournament["id"] == str(fixture["twenty"]["id"])
    assert (
        tournament["participantCount"],
        tournament["teamName"],
        tournament["spentPoints"],
        tournament["placement"],
        tournament["wins"],
        tournament["mmrDelta"],
    ) == (
        20,
        "팀 1",
        300,
        1,
        2,
        None,
    )
    assert [row["stage"] for row in tournament["series"]] == ["FINAL", "SEMIFINAL_A"]
    assert len(tournament["roster"]) == 5
    assert all(row["championId"] is None for row in tournament["roster"])
    assert tournament["roster"][0]["isTarget"]
    swapped = tournament["series"][1]["games"][0]
    assert swapped["winningSide"] == swapped["target"]["side"] == "RED"
    assert swapped["redTeam"][0]["isTarget"] and not swapped["blueTeam"][0]["isTarget"]
    assert auction["source"] == "AUCTION" and auction["stage"] == "FINAL"
    assert auction["participantCount"] == 10 and auction["mmrDelta"] is None
    assert regular["source"] == "REGULAR" and regular["stage"] is None
    assert regular["mmrDelta"] == 15.0 and regular["result"] == "WIN"
    game = regular["games"][0]
    assert game["target"] == {
        "side": "BLUE",
        "lane": "TOP",
        "championId": "Ahri",
        "mmrBefore": 1500.0,
        "mmrAfter": 1515.0,
    }
    assert game["blueBans"] == ["Jax"] and game["redBans"] == ["Vi"]
    assert all(key not in str(data) for key in ("userId", "puuid", "routingRegion", "source_input"))
    cursor = None
    for expected in data["records"]:
        page = await search(runtime_client, limit="1", **({"cursor": cursor} if cursor else {}))
        assert page["summary"] == data["summary"] and page["records"] == [expected]
        cursor = page["nextCursor"]
    assert cursor is None
    response = await runtime_client.head(PATH, params={"q": USER_ID})
    assert response.status_code == 200 and response.content == b""


async def test_search_empty_closed_foreign_and_no_active_season(owner, runtime_client):
    await authenticated_setup(owner, runtime_client)
    data = await search(runtime_client)
    season = data["selectedSeasonId"]
    assert data["summary"] == {"games": 0, "wins": 0, "losses": 0}
    assert data["records"] == data["championRecords"] == [] and data["nextCursor"] is None
    async with owner.begin() as c:
        assert await c.scalar(text("SELECT count(*) FROM user_lane_mmr")) == 0
        await c.execute(text("UPDATE seasons SET status='CLOSED',ended_at=now()"))
    data = await search(runtime_client)
    assert data["selectedSeasonId"] is None and len(data["laneMmr"]) == 5
    assert (await search(runtime_client, seasonId=season))["selectedSeasonId"] == season
    for valid in (season.replace("-", ""), "{" + season + "}", "urn:uuid:" + season):
        assert (await search(runtime_client, seasonId=valid))["selectedSeasonId"] == season
    foreign = await guild_fixture(owner, GUILD_B, "Other", ROLE_B)
    for missing in (foreign, uuid.uuid7()):
        assert_failure(
            await runtime_client.get(PATH, params={"q": USER_ID, "seasonId": str(missing)}),
            404,
            "season_not_found",
            "요청한 시즌을 찾을 수 없습니다.",
        )


@pytest.mark.parametrize(
    ("query", "code", "message"),
    [
        ({"q": " "}, "invalid_search_query", "검색어가 올바르지 않습니다."),
        ({"q": "x" * 101}, "invalid_search_query", "검색어가 올바르지 않습니다."),
        ({"limit": "0"}, "invalid_limit", "limit은 1 이상 50 이하이어야 합니다."),
        ({"limit": "51"}, "invalid_limit", "limit은 1 이상 50 이하이어야 합니다."),
        ({"limit": " 1"}, "invalid_limit", "limit은 1 이상 50 이하이어야 합니다."),
        ({"limit": "１"}, "invalid_limit", "limit은 1 이상 50 이하이어야 합니다."),
        ({"limit": "9" * 5000}, "invalid_limit", "limit은 1 이상 50 이하이어야 합니다."),
        ({"seasonId": "no"}, "invalid_season_id", "seasonId 형식이 올바르지 않습니다."),
        ({"cursor": "invalid!"}, "invalid_cursor", "cursor 형식이 올바르지 않습니다."),
        ({"cursor": ""}, "invalid_cursor", "cursor 형식이 올바르지 않습니다."),
        ({"type": "REGULAR"}, "bad_request", "요청 형식이 올바르지 않습니다."),
        ({"lane": "TOP"}, "bad_request", "요청 형식이 올바르지 않습니다."),
    ],
)
async def test_search_reports_query_errors(owner, runtime_client, query, code, message):
    await authenticated_setup(owner, runtime_client)
    response = await runtime_client.get(PATH, params={"q": USER_ID, **query})
    assert_failure(response, 400, code, message)


async def test_search_extractor_order_duplicate_shape_and_active_access(owner, runtime_client):
    assert_failure(
        await runtime_client.get(PATH + "?invalid=1"), 401, "unauthorized", "인증이 필요합니다."
    )
    await authenticated_setup(owner, runtime_client)
    for params in ({}, [("q", USER_ID), ("q", USER_ID)]):
        assert_failure(
            await runtime_client.get(PATH, params=params),
            400,
            "bad_request",
            "요청 형식이 올바르지 않습니다.",
        )
    for limit in ("+0001", "0" * 5000 + "1"):
        assert (await search(runtime_client, limit=limit))["records"] == []
    for target in ("missing", "999999999999999999"):
        assert_failure(
            await runtime_client.get(PATH, params={"q": target}),
            404,
            "not_found",
            "요청한 길드 멤버를 찾을 수 없습니다.",
        )
    async with owner.begin() as c:
        await c.execute(text("UPDATE guild_members SET status='INACTIVE',inactivated_at=now()"))
    assert_failure(
        await runtime_client.get(PATH, params={"q": USER_ID}),
        404,
        "not_found",
        "요청한 길드 멤버를 찾을 수 없습니다.",
    )


async def test_search_excludes_voided_games_deleted_and_cancelled_parents(owner, runtime_client):
    fixture = await seeded(owner, runtime_client)
    async with owner.begin() as c:
        await c.execute(
            text("""
            UPDATE games SET status='VOIDED',voided_by=:u,voided_by_type='USER',
                voided_at=now(),void_reason='test exclusion'
            WHERE id=:id
        """),
            {"u": USER_ID, "id": fixture["twenty"]["games"][0]},
        )
    data = await search(runtime_client)
    assert data["summary"]["games"] == 3 and len(data["records"][0]["series"]) == 1
    async with owner.begin() as c:
        await c.execute(
            text("UPDATE series SET deleted_at=now() WHERE id=:id"), {"id": fixture["series"]}
        )
        await c.execute(
            text("UPDATE auction_matches SET deleted_at=now() WHERE id=:id"),
            {"id": fixture["ten"]["matches"][0]},
        )
        await c.execute(
            text("""
            UPDATE auction_tournaments SET status='CANCELLED',champion_team_id=NULL WHERE id=:id
        """),
            {"id": fixture["twenty"]["id"]},
        )
    data = await search(runtime_client)
    assert data["summary"] == {"games": 0, "wins": 0, "losses": 0}
    assert data["records"] == data["championRecords"] == []


async def test_search_private_links_do_not_hide_internal_records(owner, runtime_client):
    await seeded(owner, runtime_client)
    async with owner.begin() as c:
        await c.execute(
            text("""
            INSERT INTO user_privacy_settings
                (user_id,public_profile_enabled,show_riot_ids,allow_external_record_search,updated_at)
            VALUES (:u,false,false,false,now())
            ON CONFLICT (user_id) DO UPDATE SET show_riot_ids=false,public_profile_enabled=false
        """),
            {"u": USER_ID},
        )
        await c.execute(
            text("""
            INSERT INTO riot_accounts
                (puuid,user_id,platform,routing_region,game_name,tag_line,normalized_game_name,
                 normalized_tag_line,is_main,created_at,updated_at)
            VALUES ('record-private-puuid',:u,'KR','ASIA','SearchUser','KR1',
                    'searchuser','kr1',true,now(),now())
        """),
            {"u": USER_ID},
        )
    data = await search(runtime_client)
    assert data["riotAccounts"] == [] and data["member"]["profileHref"] is None
    assert data["summary"]["games"] == 4 and len(data["records"]) == 3
    assert_failure(
        await runtime_client.get(PATH, params={"q": "SearchUser#KR1"}),
        404,
        "not_found",
        "요청한 길드 멤버를 찾을 수 없습니다.",
    )
    async with owner.begin() as c:
        await c.execute(text("UPDATE user_privacy_settings SET show_riot_ids=true"))
    visible = await search(runtime_client, q="ＳｅａｒｃｈＵｓｅｒ#ＫＲ１")
    assert visible["records"] == data["records"]
    assert visible["riotAccounts"][0]["gameName"] == "SearchUser"
    assert "record-private-puuid" not in str(visible)


async def test_search_invalid_lineup_returns_safe_correlated_error(owner, runtime_client):
    fixture = await seeded(owner, runtime_client)
    async with owner.begin() as c:
        await c.execute(
            text("DELETE FROM game_participants WHERE game_id=:id AND user_id='4009'"),
            {"id": fixture["regular"]},
        )
    assert_failure(
        await runtime_client.get(PATH, params={"q": USER_ID}),
        500,
        "internal_error",
        "요청을 처리하는 중 내부 오류가 발생했습니다.",
    )


async def test_search_cursor_canonical_encoding_and_nanosecond_boundary(owner, runtime_client):
    fixture = await seeded(owner, runtime_client)
    first = await search(runtime_client, limit="1")
    canonical = first["nextCursor"]
    payload = json.loads(base64.urlsafe_b64decode(canonical + "=" * (-len(canonical) % 4)))
    malformed = [canonical + "=", canonical[:-1] + "!", "a" * 513]
    for change in (
        {"kind": "OTHER"},
        {"playedAtNanos": True},
        {"playedAtNanos": 2**63},
        {"id": "bad"},
        {"extra": 1},
    ):
        token = base64.urlsafe_b64encode(
            json.dumps({**payload, **change}, separators=(",", ":")).encode()
        )
        malformed.append(token.rstrip(b"=").decode())
    for cursor in malformed:
        assert_failure(
            await runtime_client.get(PATH, params={"q": USER_ID, "cursor": cursor}),
            400,
            "invalid_cursor",
            "cursor 형식이 올바르지 않습니다.",
        )
    payload["playedAtNanos"] += 999
    token = base64.urlsafe_b64encode(json.dumps(payload, separators=(",", ":")).encode()).rstrip(
        b"="
    )
    page = await search(runtime_client, cursor=token.decode())
    assert [row["id"] for row in page["records"]] == [
        str(fixture["ten"]["matches"][0]),
        str(fixture["series"]),
    ]
