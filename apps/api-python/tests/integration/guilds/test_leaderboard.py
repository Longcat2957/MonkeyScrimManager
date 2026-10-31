import logging

import httpx
import pytest
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncEngine

from monkeybot_api.repositories.guilds import leaderboard as repository
from tests.support.assertions import assert_failure
from tests.support.auth.discord import DiscordStub
from tests.support.guilds.context import GUILD_A, USER_ID, authenticated_setup
from tests.support.guilds.leaderboard import DUMMY, PATH, PLAYER, game, setup

pytestmark = pytest.mark.postgres


async def test_snapshot_privacy_played_lanes_dummy_members_and_unrounded_weighting(
    owner: AsyncEngine,
    runtime_client: httpx.AsyncClient,
    discord_stub: DiscordStub,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    season = await setup(owner, runtime_client)
    provider_calls = len(discord_stub.requests)

    async def unexpected_network(*args, **kwargs):
        raise AssertionError("leaderboard must not call an external provider")

    monkeypatch.setattr(httpx.AsyncHTTPTransport, "handle_async_request", unexpected_network)
    response = await runtime_client.get(PATH)
    assert response.status_code == 200, response.text
    body = response.json()
    assert body["meta"]["requestId"] == response.headers["x-request-id"]
    assert body["data"]["season"] == {"id": str(season), "name": "현재 시즌"}
    players = body["data"]["players"]
    assert [player["id"] for player in players] == [PLAYER, DUMMY, USER_ID]
    public, dummy, viewer = players
    assert public == {
        "id": PLAYER,
        "name": "Longcat",
        "tag": "Longcat#KR1",
        "profileIconId": 7,
        "compositeMmr": 4600.0 / 3.0,
        "mainRole": "BOTTOM",
        "isMe": False,
        "stats": {
            "MID": {"mmr": 1600.0, "games": 2, "wins": 1, "championId": None},
            "BOTTOM": {"mmr": 1400.0, "games": 1, "wins": 1, "championId": None},
        },
    }
    assert dummy["tag"] is None and dummy["profileIconId"] is None
    assert dummy["mainRole"] == "MID" and not dummy["isMe"]
    assert dummy["compositeMmr"] == public["compositeMmr"]
    assert viewer["tag"] is None and viewer["profileIconId"] is None
    assert viewer["mainRole"] == "TOP" and viewer["isMe"]
    assert viewer["compositeMmr"] == 1510.0
    for private in ("PrivateViewer", "Alternate", "puuid", "showRiotIds", "allowExternal"):
        assert private not in response.text
    assert len(discord_stub.requests) == provider_calls
    head = await runtime_client.head(PATH)
    assert head.status_code == 200 and head.content == b""


async def test_champions_count_only_confirmed_games_with_live_regular_and_auction_parents(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    season = await setup(owner, runtime_client)
    async with owner.begin() as c:
        await game(c, season, 1, "regular", "Zed")
        await game(c, season, 2, "auction", "Ahri")
        for number, kind in enumerate(
            (
                "series_cancelled",
                "series_deleted",
                "match_cancelled",
                "match_deleted",
                "tournament_cancelled",
                "tournament_deleted",
                "voided",
            ),
            start=3,
        ):
            await game(c, season, number, kind, "Aatrox")
        await game(c, season, 10, "regular", None)
    response = await runtime_client.get(PATH)
    assert response.status_code == 200, response.text
    player = response.json()["data"]["players"][0]
    assert player["stats"]["MID"]["championId"] == "Ahri"
    assert player["stats"]["BOTTOM"]["championId"] is None
    # Counts precede the champion-ID tiebreaker.
    async with owner.begin() as c:
        await game(c, season, 11, "regular", "Zed")
    player = (await runtime_client.get(PATH)).json()["data"]["players"][0]
    assert player["stats"]["MID"]["championId"] == "Zed"


async def test_main_role_fallback_uses_games_then_mmr_then_canonical_lane_order(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    await setup(owner, runtime_client)
    for changes, expected in (
        ("UPDATE user_lane_mmr SET mmr=1511 WHERE user_id=:user AND lane='JUNGLE'", "JUNGLE"),
        ("UPDATE user_lane_mmr SET games_played=4 WHERE user_id=:user AND lane='TOP'", "TOP"),
        ("UPDATE guild_members SET main_lane='JUNGLE' WHERE user_id=:user", "JUNGLE"),
    ):
        async with owner.begin() as c:
            await c.execute(
                text(changes + " AND guild_id=:guild"), {"user": USER_ID, "guild": GUILD_A}
            )
        players = (await runtime_client.get(PATH)).json()["data"]["players"]
        assert next(player for player in players if player["isMe"])["mainRole"] == expected


async def test_no_active_season_returns_empty_snapshot_and_query_parameters_are_ignored(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    await setup(owner, runtime_client)
    initial = (await runtime_client.get(PATH)).json()["data"]
    ignored = await runtime_client.get(PATH + "?seasonId=invalid&limit=-1&scope=nope&lane=MID")
    assert ignored.status_code == 200 and ignored.json()["data"] == initial
    async with owner.begin() as c:
        await c.execute(
            text("UPDATE seasons SET status='CLOSED', ended_at=now() WHERE guild_id=:guild"),
            {"guild": GUILD_A},
        )
    response = await runtime_client.get(PATH)
    assert response.status_code == 200
    assert response.json()["data"] == {"season": None, "players": []}


@pytest.mark.parametrize("mode", ["missing", "disabled", "not_member", "inactive"])
async def test_hidden_guild_or_membership_including_service_admin(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, mode: str
) -> None:
    await authenticated_setup(owner, runtime_client)
    path = PATH
    changes = {
        "disabled": "UPDATE guilds SET status='DISABLED'",
        "not_member": "DELETE FROM guild_members",
        "inactive": "UPDATE guild_members SET status='INACTIVE'",
    }
    if mode == "missing":
        path = "/api/guilds/100000000000000099/leaderboard"
    else:
        async with owner.begin() as c:
            await c.execute(text(changes[mode]))
    assert_failure(
        await runtime_client.get(path), 404, "not_found", "요청한 길드를 찾을 수 없습니다."
    )


async def test_invalid_path_precedes_session_and_valid_path_requires_authentication(
    runtime_client: httpx.AsyncClient,
) -> None:
    assert_failure(
        await runtime_client.get("/api/guilds/not-an-id/leaderboard"),
        400,
        "bad_request",
        "요청 형식이 올바르지 않습니다.",
    )
    assert_failure(await runtime_client.get(PATH), 401, "unauthorized", "인증이 필요합니다.")


async def test_membership_season_and_rows_share_a_read_only_repeatable_snapshot(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, monkeypatch: pytest.MonkeyPatch
) -> None:
    await setup(owner, runtime_client)
    original = repository.load_rows
    observations = []

    async def concurrent_update(connection, guild_id, season_id):
        observations.append(
            (
                await connection.scalar(text("SHOW transaction_isolation")),
                await connection.scalar(text("SHOW transaction_read_only")),
            )
        )
        async with owner.begin() as writer:
            await writer.execute(
                text("UPDATE user_lane_mmr SET mmr=5000 WHERE guild_id=:guild AND user_id=:user"),
                {"guild": GUILD_A, "user": PLAYER},
            )
        return await original(connection, guild_id, season_id)

    monkeypatch.setattr(repository, "load_rows", concurrent_update)
    response = await runtime_client.get(PATH)
    assert response.status_code == 200, response.text
    assert response.json()["data"]["players"][0]["compositeMmr"] == 4600.0 / 3.0
    assert observations == [("repeatable read", "on")]


@pytest.mark.parametrize(
    "corruption",
    [
        "nan",
        "infinity",
        "overflow",
        "zero_games",
        "invalid_wins",
        "negative_icon",
        "blank_tag",
        "duplicate_lane",
        "identity_mismatch",
        "invalid_lane",
        "invalid_main_lane",
        "season",
    ],
)
async def test_stored_invariants_return_safe_error_with_correlated_diagnostics(
    owner: AsyncEngine,
    runtime_client: httpx.AsyncClient,
    monkeypatch: pytest.MonkeyPatch,
    caplog: pytest.LogCaptureFixture,
    corruption: str,
) -> None:
    await authenticated_setup(owner, runtime_client)
    row = {
        "user_id": USER_ID,
        "name": "Viewer",
        "tag": None,
        "profile_icon_id": None,
        "main_lane": None,
        "lane": "TOP",
        "mmr": 1500.0,
        "games": 2,
        "wins": 1,
        "champion_id": None,
    }
    overrides = {
        "nan": {"mmr": float("nan")},
        "infinity": {"mmr": float("inf")},
        "overflow": {"mmr": 1e308},
        "zero_games": {"games": 0, "wins": 0},
        "invalid_wins": {"wins": 3},
        "negative_icon": {"profile_icon_id": -1},
        "blank_tag": {"tag": "  "},
        "invalid_lane": {"lane": "ADC"},
        "invalid_main_lane": {"main_lane": "ADC"},
    }
    row.update(overrides.get(corruption, {}))
    rows = [row]
    if corruption == "duplicate_lane":
        rows.append(dict(row))
    elif corruption == "identity_mismatch":
        rows.append({**row, "lane": "MID", "name": "Different"})
    elif corruption == "season":
        original = repository.load_access

        async def corrupt_access(*args):
            access = await original(*args)
            return {**access, "season_name": None}

        monkeypatch.setattr(repository, "load_access", corrupt_access)

    async def corrupt_rows(*args):
        return rows

    monkeypatch.setattr(repository, "load_rows", corrupt_rows)
    with caplog.at_level(logging.ERROR):
        response = await runtime_client.get(PATH)
    assert_failure(response, 500, "internal_error", "요청을 처리하는 중 내부 오류가 발생했습니다.")
    diagnostics = [r for r in caplog.records if r.message == "guild_leaderboard_projection_failed"]
    assert len(diagnostics) == 1
    assert diagnostics[0].request_id == response.headers["x-request-id"]
    assert diagnostics[0].exc_info is not None
    assert "SELECT" not in response.text and "Traceback" not in response.text
