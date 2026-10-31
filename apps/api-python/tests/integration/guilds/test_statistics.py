import uuid

import pytest
from sqlalchemy import text

from monkeybot_api.repositories.guilds import statistics as repository
from tests.support.assertions import assert_failure
from tests.support.guilds.context import (
    GUILD_A,
    GUILD_B,
    ROLE_B,
    USER_ID,
    authenticated_setup,
    guild_fixture,
)
from tests.support.guilds.statistics import PATH, seeded

pytestmark = pytest.mark.postgres


async def snapshot(client, query=""):
    response = await client.get(PATH + query)
    assert response.status_code == 200, response.text
    body = response.json()
    assert body["meta"]["requestId"] == response.headers["x-request-id"]
    return body["data"]


async def test_statistics_aggregates_confirmed_games_all_scopes_and_final_auction_rosters(
    owner,
    runtime_client,
):
    fixture = await seeded(owner, runtime_client)
    data = await snapshot(runtime_client)
    assert data["selectedSeasonId"] == str(fixture["season"])
    assert data["scope"] == "ALL"
    assert data["overview"] == {
        "games": 3,
        "regularGames": 2,
        "auctionGames": 1,
        "uniquePlayers": 10,
        "blueWins": 1,
        "redWins": 2,
        "averageDurationSeconds": 1500.0,
        "coverage": {
            "durationGames": 2,
            "participantStatGames": 1,
            "championPickGames": 3,
            "championBanGames": 3,
        },
    }
    assert data["players"]["minimumWinRateGames"] == 3
    assert data["players"]["mostActive"][0] == {
        "userId": "1000",
        "name": "선수 1000",
        "games": 3,
        "wins": 3,
        "losses": 0,
    }
    assert [row["userId"] for row in data["players"]["bestWinRates"][:2]] == ["1000", "1002"]
    assert len(data["players"]["bestWinRates"]) == 10
    assert len(data["relationships"]["bestDuos"]) == 10
    assert data["relationships"]["bestDuos"][0] == {
        "userAId": "1000",
        "userAName": "선수 1000",
        "userBId": "1002",
        "userBName": "선수 1002",
        "games": 3,
        "wins": 3,
        "losses": 0,
    }
    assert data["champions"] == {
        "picks": [{"championId": "Ahri", "lane": "TOP", "games": 3, "wins": 3, "losses": 0}],
        "bans": [{"championId": "Jax", "bans": 3}],
    }
    assert data["auction"]["completedTournaments"] == 1
    assert data["auction"]["captains"][0] == {
        "userId": "1000",
        "name": "선수 1000",
        "teamsCaptained": 1,
        "titles": 1,
        "finalAppearances": 1,
        "matchWins": 1,
        "matchLosses": 0,
        "gameWins": 1,
        "gameLosses": 0,
        "totalSpentPoints": 700,
    }
    losing_captain = data["auction"]["captains"][1]
    assert (
        losing_captain["titles"],
        losing_captain["matchLosses"],
        losing_captain["gameLosses"],
        losing_captain["totalSpentPoints"],
    ) == (0, 1, 1, 0)
    bid = data["auction"]["topBids"][0]
    assert bid["userId"] == fixture["bidder"] and bid["tournamentId"] == fixture["tournament"]
    assert (bid["points"], bid["games"], bid["wins"], bid["losses"], bid["wonTournament"]) == (
        700,
        1,
        1,
        0,
        True,
    )
    assert all(secret not in str(data) for secret in ("puuid", "riotId", "showRiotIds"))
    regular = await snapshot(runtime_client, "?scope=REGULAR")
    assert regular["overview"]["games"] == 2 and regular["overview"]["auctionGames"] == 0
    assert regular["players"]["bestWinRates"] == []
    assert regular["auction"] == {"completedTournaments": 0, "captains": [], "topBids": []}
    auction = await snapshot(runtime_client, "?scope=AUCTION")
    assert auction["overview"]["games"] == 1 and auction["overview"]["regularGames"] == 0
    assert auction["auction"] == data["auction"]
    response = await runtime_client.head(PATH)
    assert response.status_code == 200 and response.content == b""

    # Final active roster, not the historical award log, owns the bid ranking.
    async with owner.begin() as connection:
        await connection.execute(
            text("""
            UPDATE auction_team_members SET released_at=now() WHERE acquired_via='BID'
        """)
        )
    assert (await snapshot(runtime_client))["auction"]["topBids"] == []


async def test_statistics_excludes_deleted_and_cancelled_parents(owner, runtime_client):
    fixture = await seeded(owner, runtime_client)
    for table, identifier, column, value, games in [
        ("series", fixture["series"], "deleted_at", "now()", 1),
        ("auction_matches", uuid.UUID(fixture["match"]), "deleted_at", "now()", 2),
        ("auction_tournaments", uuid.UUID(fixture["tournament"]), "deleted_at", "now()", 2),
    ]:
        async with owner.begin() as connection:
            await connection.execute(
                text(f"UPDATE {table} SET {column}={value} WHERE id=:id"), {"id": identifier}
            )
        assert (await snapshot(runtime_client))["overview"]["games"] == games
        async with owner.begin() as connection:
            await connection.execute(
                text(f"UPDATE {table} SET {column}=NULL WHERE id=:id"), {"id": identifier}
            )
    for table, reset, cancel in [
        (
            "auction_matches",
            "status='COMPLETED',winner_team_id=left_team_id,lineups_locked_at=now(),"
            f"lineups_locked_by='{USER_ID}'",
            "status='CANCELLED',winner_team_id=NULL,lineups_locked_at=NULL,lineups_locked_by=NULL",
        ),
        (
            "auction_tournaments",
            "status='COMPLETED',champion_team_id=(SELECT left_team_id "
            "FROM auction_matches LIMIT 1)",
            "status='CANCELLED',champion_team_id=NULL",
        ),
    ]:
        async with owner.begin() as connection:
            await connection.execute(text(f"UPDATE {table} SET {cancel}"))
        assert (await snapshot(runtime_client))["overview"]["games"] == 2
        async with owner.begin() as connection:
            await connection.execute(text(f"UPDATE {table} SET {reset}"))


async def test_statistics_limits_and_tie_order(owner, runtime_client):
    fixture = await seeded(owner, runtime_client)
    async with owner.begin() as connection:
        await connection.execute(
            text("""
            UPDATE game_participants SET user_id='1010'
            WHERE game_id=:game AND user_id='1009'
        """),
            {"game": fixture["second"]},
        )
        await connection.execute(
            text("""
            UPDATE game_participants SET champion_id=CASE
              WHEN game_id=:first THEN 'Alpha' || user_id ELSE 'Beta' || user_id END
            WHERE game_id IN (:first,:second)
        """),
            {"first": fixture["first"], "second": fixture["second"]},
        )
        await connection.execute(
            text("""
            INSERT INTO game_bans (guild_id,game_id,side,slot,champion_id)
            SELECT :guild,:game,'RED',n,'Ban' || n FROM generate_series(1,5) n
        """),
            {"guild": GUILD_A, "game": fixture["first"]},
        )
        await connection.execute(
            text("""
            INSERT INTO game_bans (guild_id,game_id,side,slot,champion_id)
            SELECT :guild,:game,'RED',n,'Ban' || (n+5) FROM generate_series(1,5) n
        """),
            {"guild": GUILD_A, "game": fixture["second"]},
        )
    data = await snapshot(runtime_client)
    assert data["overview"]["uniquePlayers"] == 11
    assert len(data["players"]["mostActive"]) == 10
    assert data["players"]["mostActive"][-1]["userId"] == "1009"
    assert len(data["players"]["bestWinRates"]) == 9
    assert len(data["champions"]["picks"]) == len(data["champions"]["bans"]) == 10
    assert data["champions"]["picks"][0]["championId"] == "Ahri"
    assert [row["championId"] for row in data["champions"]["bans"][:3]] == ["Jax", "Ban1", "Ban10"]


async def test_statistics_query_validation_preserves_extractor_order(owner, runtime_client):
    assert_failure(
        await runtime_client.get("/api/guilds/no/statistics"),
        400,
        "bad_request",
        "요청 형식이 올바르지 않습니다.",
    )
    for query in ("?unknown=1", "?scope=ALL&scope=REGULAR", "?seasonId=x&seasonId=y"):
        assert_failure(
            await runtime_client.get(PATH + query),
            400,
            "bad_request",
            "요청 형식이 올바르지 않습니다.",
        )
    assert_failure(
        await runtime_client.get(PATH + "?seasonId=no&scope=no"),
        401,
        "unauthorized",
        "인증이 필요합니다.",
    )
    await authenticated_setup(owner, runtime_client)
    assert_failure(
        await runtime_client.get(PATH + "?seasonId=no&scope=no"),
        400,
        "invalid_season_id",
        "seasonId 형식이 올바르지 않습니다.",
    )
    current = uuid.UUID((await snapshot(runtime_client))["selectedSeasonId"])
    for value in (str(current), current.hex, "{" + str(current) + "}", "urn:uuid:" + str(current)):
        response = await runtime_client.get(PATH, params={"seasonId": value})
        assert response.status_code == 200
        assert response.json()["data"]["selectedSeasonId"] == str(current)
    for value in ("{" + current.hex + "}", current.hex[:4] + "-" + current.hex[4:]):
        assert_failure(
            await runtime_client.get(PATH, params={"seasonId": value}),
            400,
            "invalid_season_id",
            "seasonId 형식이 올바르지 않습니다.",
        )
    response = await runtime_client.get(PATH + "?scope=all")
    assert_failure(
        response, 400, "invalid_scope", "scope은 ALL, REGULAR, AUCTION 중 하나여야 합니다."
    )
    assert response.json()["error"]["details"] == {"allowed": ["ALL", "REGULAR", "AUCTION"]}


@pytest.mark.parametrize("mode", ["missing", "disabled", "inactive", "not_member"])
async def test_statistics_hides_inaccessible_guild_even_from_service_admin(
    owner,
    runtime_client,
    mode,
):
    await authenticated_setup(owner, runtime_client)
    path = PATH
    async with owner.begin() as connection:
        if mode == "missing":
            path = f"/api/guilds/{GUILD_B}/statistics"
        elif mode == "disabled":
            await connection.execute(text("UPDATE guilds SET status='DISABLED'"))
        elif mode == "inactive":
            await connection.execute(text("UPDATE guild_members SET status='INACTIVE'"))
        else:
            await connection.execute(text("DELETE FROM guild_members"))
    assert_failure(
        await runtime_client.get(path), 404, "not_found", "요청한 길드를 찾을 수 없습니다."
    )


async def test_statistics_empty_closed_foreign_and_absent_seasons(owner, runtime_client):
    await authenticated_setup(owner, runtime_client)
    foreign = await guild_fixture(owner, GUILD_B, "Foreign", ROLE_B)
    empty = await snapshot(runtime_client)
    assert empty["overview"]["games"] == 0
    assert empty["overview"]["averageDurationSeconds"] is None
    assert empty["players"]["mostActive"] == empty["players"]["bestWinRates"] == []
    assert empty["relationships"] == {"bestDuos": []}
    assert empty["champions"] == {"picks": [], "bans": []}
    assert empty["auction"] == {"completedTournaments": 0, "captains": [], "topBids": []}
    assert_failure(
        await runtime_client.get(PATH + f"?seasonId={foreign}"),
        404,
        "season_not_found",
        "요청한 시즌을 찾을 수 없습니다.",
    )
    async with owner.begin() as connection:
        await connection.execute(
            text("""
            UPDATE seasons SET status='CLOSED',ended_at=now() WHERE guild_id=:guild
        """),
            {"guild": GUILD_A},
        )
    closed = await snapshot(runtime_client, "?seasonId=" + empty["selectedSeasonId"])
    assert closed["selectedSeasonId"] == empty["selectedSeasonId"]
    assert closed["seasons"][0]["status"] == "CLOSED"
    absent = await snapshot(runtime_client)
    assert absent["selectedSeasonId"] is None and absent["overview"] == empty["overview"]


async def test_statistics_uses_read_only_repeatable_snapshot(owner, runtime_client, monkeypatch):
    await seeded(owner, runtime_client)
    original = repository.overview

    async def concurrent_change(connection, *args):
        assert await connection.scalar(text("SHOW transaction_isolation")) == "repeatable read"
        assert await connection.scalar(text("SHOW transaction_read_only")) == "on"
        result = await original(connection, *args)
        async with owner.begin() as writer:
            await writer.execute(text("UPDATE guild_members SET display_name='이후 이름'"))
        return result

    monkeypatch.setattr(repository, "overview", concurrent_change)
    data = await snapshot(runtime_client)
    assert data["players"]["mostActive"][0]["name"] == "선수 1000"
    assert data["auction"]["captains"][0]["name"] == "선수 1000"


async def test_statistics_invalid_stored_projection_is_safe_and_correlated(
    owner,
    runtime_client,
    monkeypatch,
    caplog,
):
    await authenticated_setup(owner, runtime_client)
    original = repository.list_seasons

    async def corrupted_season(*args):
        rows = await original(*args)
        rows[0]["name"] = " "
        return rows

    monkeypatch.setattr(repository, "list_seasons", corrupted_season)
    response = await runtime_client.get(PATH, headers={"x-request-id": "statistics-corrupt"})
    assert_failure(response, 500, "internal_error", "요청을 처리하는 중 내부 오류가 발생했습니다.")
    assert any(
        record.message == "guild_statistics_projection_failed"
        and record.request_id == response.json()["error"]["requestId"]
        for record in caplog.records
        if hasattr(record, "request_id")
    )
