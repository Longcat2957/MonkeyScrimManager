import pytest

from tests.support.auctions.bidding import PATH as TOURNAMENTS
from tests.support.auctions.finish import finish, scenario
from tests.support.auctions.games import play
from tests.support.guilds.record_search import PATH

pytestmark = pytest.mark.postgres


@pytest.mark.parametrize("kind", ["tied", "twenty-semifinals"])
async def test_finished_tied_auction_is_a_draw_in_record_search(owner, runtime_client, kind):
    headers, tournament, played = await scenario(owner, runtime_client, kind)
    tied_match = played[-1]
    if kind == "twenty-semifinals":
        tied_match = await play(runtime_client, headers, tied_match, winner="RED")
    target = tied_match["lineups"][0]["userId"]
    preview = await runtime_client.get(f"{TOURNAMENTS}/{tournament['id']}/finish-preview")
    assert preview.status_code == 200, preview.text
    response = await finish(runtime_client, headers, preview.json()["data"]["tournament"])
    assert response.status_code == 200, response.text
    assert response.json()["data"]["championTeamId"] is None

    response = await runtime_client.get(PATH, params={"q": target})
    assert response.status_code == 200, response.text
    data = response.json()["data"]
    assert data["summary"] == {"games": 2, "wins": 1, "losses": 1}
    assert len(data["records"]) == 1
    record = data["records"][0]
    if kind == "twenty-semifinals":
        assert record["kind"] == "TOURNAMENT"
        assert record["id"] == tournament["id"]
        assert record["participantCount"] == 20
        assert record["placement"] is None
        assert len(record["series"]) == 1
        record = record["series"][0]
        assert record["stage"] == "SEMIFINAL_B"
    else:
        assert record["kind"] == "SERIES"
        assert record["source"] == "AUCTION"
        assert record["participantCount"] == 10
        assert record["stage"] == "FINAL"
    assert record["id"] == tied_match["match"]["id"]
    assert record["result"] == "DRAW"
    assert (record["wins"], record["losses"]) == (1, 1)
    assert {game["id"] for game in record["games"]} == {game["id"] for game in tied_match["games"]}
