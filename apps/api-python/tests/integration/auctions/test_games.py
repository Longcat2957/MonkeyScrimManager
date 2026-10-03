import asyncio
import hashlib
import json
import uuid

import pytest
from sqlalchemy import text

from monkeybot_api.schemas.auctions.games import MESSAGES
from tests.support.assertions import assert_failure
from tests.support.auctions.bidding import PATH as TOURNAMENTS
from tests.support.auctions.games import PATH, body, changed, command, failure, play, prepared
from tests.support.auctions.lineups import fill, run
from tests.support.guilds.context import GUILD_A

pytestmark = pytest.mark.postgres


async def test_match_game_submit_replay_early_complete_result_revert(owner, runtime_client):
    h, bracket, initial = await prepared(owner, runtime_client)
    tournament = bracket["tournament"]["id"]
    async with owner.begin() as c:
        await c.execute(text("UPDATE guild_config SET result_channel_id='5555555'"))
    payload = body(initial, sidePreset="SWAPPED", patchVersion="26.18")
    picked = next(iter(payload["picks"]))
    payload["picks"][picked] = "Ahri"
    payload["bans"]["RED"][2] = "Lux"
    first = await play(runtime_client, h, initial, payload=payload, key="first-game")
    assert first["match"]["status"] == "IN_PROGRESS"
    assert (first["match"]["leftScore"], first["match"]["rightScore"]) == (0, 1)
    assert first["games"][0]["picks"] == payload["picks"]
    assert first["games"][0]["bans"] == payload["bans"]
    assert first["permissions"]["canCompleteFinalAsBo1"]
    replay = await play(runtime_client, h, initial, payload=payload, key="first-game")
    assert replay == first
    completed = await changed(runtime_client, h, first, "complete", key="early")
    assert completed["match"]["bestOf"] == 1
    assert completed["match"]["status"] == "COMPLETED"
    assert completed["match"]["winnerTeamId"] == completed["match"]["rightTeamId"]
    assert await changed(runtime_client, h, first, "complete", key="early") == completed
    result = (await runtime_client.get(f"{TOURNAMENTS}/{tournament}/result")).json()["data"]
    assert result["tournament"]["championTeamId"] == completed["match"]["rightTeamId"]
    assert len(result["players"]) == 10
    assert all(p["games"] == 1 for p in result["players"])
    assert sum(p["wins"] for p in result["players"]) == 5
    assert sum(p["mmrDelta"] for p in result["players"]) == pytest.approx(0)
    async with owner.connect() as c:
        assert await c.scalar(text("SELECT count(*) FROM mmr_ledger_entries")) == 10
        assert (
            await c.scalar(
                text("SELECT count(*) FROM admin_audit_log WHERE target_type='AUCTION_MATCH_GAME'")
            )
            == 2
        )
        assert await c.scalar(text("SELECT final_best_of FROM auction_recruitments")) == 1
        assert await c.scalar(text("SELECT final_best_of FROM auction_tournaments")) == 1
        card = (
            await c.execute(
                text(
                    "SELECT payload FROM bot_notification_outbox "
                    "WHERE notification_type='AUCTION_TOURNAMENT_RESULT_CREATE'"
                )
            )
        ).scalar_one()
        assert card["schemaVersion"] == 1 and card["tournamentId"] == tournament
        assert card["champion"]["teamId"] == completed["match"]["rightTeamId"]
        assert card["matches"][0]["rightScore"] == 1
        assert card["matches"][0]["label"] == "결승"
        stored = (
            await c.execute(
                text(
                    "SELECT request_hash FROM idempotency_records "
                    "WHERE idempotency_key='first-game'"
                )
            )
        ).scalar_one()
        ordered = {
            "kind": "SUBMIT",
            "expectedRevision": payload["expectedRevision"],
            "setNumber": 1,
            "sidePreset": "SWAPPED",
            "winner": "BLUE",
            "picks": dict(sorted(payload["picks"].items())),
            "bans": payload["bans"],
            "patchVersion": "26.18",
        }
        assert (
            stored
            == hashlib.sha256(
                json.dumps(
                    [GUILD_A, {"match_id": initial["match"]["id"], "command": ordered}],
                    ensure_ascii=False,
                    separators=(",", ":"),
                ).encode()
            ).digest()
        )
    reverted = await changed(runtime_client, h, completed, "last", key="undo")
    assert reverted["match"]["status"] == "READY" and reverted["games"] == []
    assert reverted["match"]["bestOf"] == 1
    assert await changed(runtime_client, h, completed, "last", key="undo") == reverted
    result = (await runtime_client.get(f"{TOURNAMENTS}/{tournament}/result")).json()["data"]
    assert result["tournament"]["status"] == "IN_GAME"
    assert result["tournament"]["championTeamId"] is None
    assert all(p["games"] == 0 and p["mmrDelta"] is None for p in result["players"])
    async with owner.connect() as c:
        assert (
            await c.scalar(
                text("SELECT count(*) FROM mmr_ledger_entries WHERE source_type='GAME_REVERSAL'")
            )
            == 10
        )
        assert await c.scalar(text("SELECT status FROM game_inputs")) == "SUPERSEDED"
        assert (
            await c.scalar(
                text(
                    "SELECT failure_code FROM bot_notification_outbox "
                    "WHERE notification_type='AUCTION_TOURNAMENT_RESULT_CREATE'"
                )
            )
            == "RESULT_REVERTED"
        )
    replacement = await play(runtime_client, h, reverted)
    assert replacement["match"]["status"] == "COMPLETED"
    assert replacement["games"][0]["setNumber"] == 1


async def test_match_game_validation_fearless_and_atomic_rejection(owner, runtime_client):
    h, _, state = await prepared(owner, runtime_client)
    async with owner.begin() as c:
        await c.execute(text("UPDATE auction_matches SET hard_fearless=true"))
    failure(await command(runtime_client, h, state, "last"), "auction_match_no_confirmed_games")
    failure(
        await command(runtime_client, h, state, "complete"), "auction_match_result_not_editable"
    )
    failure(await command(runtime_client, h, state, setNumber=2), "auction_match_set_not_current")
    response = await command(runtime_client, h, state, picks={})
    failure(response, "invalid_auction_match_game_request", 422)
    payload = body(state)
    first_user, second_user = list(payload["picks"])[:2]
    payload["picks"][first_user] = "Ahri"
    payload["picks"][second_user] = "Ahri"
    failure(
        await command(runtime_client, h, state, payload=payload),
        "duplicate_auction_match_game_champion",
        422,
    )
    payload["picks"][second_user] = None
    payload["bans"]["RED"][0] = "Ahri"
    failure(
        await command(runtime_client, h, state, payload=payload),
        "duplicate_auction_match_game_champion",
        422,
    )
    payload["bans"]["RED"][0] = None
    first = await play(runtime_client, h, state, payload=payload, key="confirmed")
    failure(await command(runtime_client, h, state), "auction_match_revision_conflict")
    failure(
        await command(runtime_client, h, state, key="confirmed", sidePreset="SWAPPED"),
        "idempotency_conflict",
    )
    response = await command(runtime_client, h, first, picks=payload["picks"])
    failure(response, "auction_match_hard_fearless_champion_used", 422)
    assert response.json()["error"]["details"] == {"championId": "Ahri"}
    # Previous picks remain legal as bans; only picks are subject to fearless.
    bans = {"BLUE": ["Ahri", None, None, None, None], "RED": [None] * 5}
    second = await play(runtime_client, h, first, bans=bans, winner="RED")
    failure(
        await command(runtime_client, h, second, "complete"),
        "auction_final_bo1_completion_unavailable",
    )
    async with owner.connect() as c:
        assert await c.scalar(text("SELECT count(*) FROM games")) == 2
        assert await c.scalar(text("SELECT count(*) FROM game_inputs")) == 2
        assert await c.scalar(text("SELECT count(*) FROM mmr_ledger_entries")) == 20
    async with owner.begin() as c:
        await c.execute(text("DELETE FROM guild_member_discord_roles"))
    failure(await command(runtime_client, h, second), "operator_required", 403)


@pytest.mark.parametrize("delivery", ["leased", "attempted", "failed", "delivered"])
async def test_match_result_delivery_reconciliation(owner, runtime_client, delivery):
    h, _, state = await prepared(owner, runtime_client)
    async with owner.begin() as c:
        await c.execute(text("UPDATE guild_config SET result_channel_id='5555555'"))
        await c.execute(text("UPDATE auction_matches SET best_of=1"))
        await c.execute(text("UPDATE auction_tournaments SET final_best_of=1"))
    state = await play(runtime_client, h, state)
    async with owner.begin() as c:
        update = {
            "leased": (
                "attempts=1,lease_owner='worker',lease_token=gen_random_uuid(),"
                "lease_until=now()+interval '1 minute'"
            ),
            "attempted": "attempts=1",
            "failed": (
                "attempts=1,failed_at=now(),failure_code='DELIVERY_FAILED',"
                "last_error='test delivery failure'"
            ),
            "delivered": "attempts=1,delivered_at=now(),discord_message_id='987654321'",
        }[delivery]
        await c.execute(
            text(
                f"UPDATE bot_notification_outbox SET first_attempt_at=now(),{update} "
                "WHERE notification_type='AUCTION_TOURNAMENT_RESULT_CREATE'"
            )
        )
    response = await command(runtime_client, h, state, "last")
    if delivery == "delivered":
        assert response.status_code == 200, response.text
        async with owner.connect() as c:
            payload = (
                await c.execute(
                    text(
                        "SELECT payload FROM bot_notification_outbox "
                        "WHERE notification_type='AUCTION_TOURNAMENT_RESULT_DELETE'"
                    )
                )
            ).scalar_one()
            assert payload["discordMessageId"] == "987654321"
            assert payload["guildId"] == GUILD_A and payload["schemaVersion"] == 1
    else:
        failure(
            response,
            "auction_tournament_result_delivery_in_progress"
            if delivery == "leased"
            else "auction_tournament_result_delivery_unresolved",
        )
        async with owner.connect() as c:
            assert await c.scalar(text("SELECT status FROM games")) == "CONFIRMED"
            assert await c.scalar(text("SELECT count(*) FROM mmr_ledger_entries")) == 10


async def test_twenty_person_barrier_progression_and_downstream_reversal(owner, runtime_client):
    h, bracket, a = await prepared(owner, runtime_client, 20)
    b_id = next(m["id"] for m in bracket["bracket"]["matches"] if m["matchKey"] == "SEMIFINAL_B")
    final_id = next(m["id"] for m in bracket["bracket"]["matches"] if m["matchKey"] == "FINAL")
    failure(await command(runtime_client, h, a), "auction_match_semifinal_lineups_incomplete")
    b = (await runtime_client.get(f"{PATH}/{b_id}")).json()["data"]
    b = await fill(runtime_client, h, b)
    b = await run(runtime_client, h, b, "LOCK")
    a = await play(runtime_client, h, a)
    assert a["match"]["status"] == "COMPLETED"
    async with owner.connect() as c:
        final = (
            (
                await c.execute(
                    text("SELECT * FROM auction_matches WHERE id=:id"), {"id": uuid.UUID(final_id)}
                )
            )
            .mappings()
            .one()
        )
        assert str(final["left_team_id"]) == a["match"]["winnerTeamId"]
        assert final["right_team_id"] is None and final["status"] == "LINEUP_PENDING"
        assert (
            await c.scalar(
                text("SELECT count(*) FROM auction_match_lineups WHERE match_id=:id"),
                {"id": uuid.UUID(final_id)},
            )
            == 5
        )
    a = await changed(runtime_client, h, a, "last")
    async with owner.connect() as c:
        assert (
            await c.scalar(
                text("SELECT left_team_id FROM auction_matches WHERE id=:id"),
                {"id": uuid.UUID(final_id)},
            )
            is None
        )
        assert (
            await c.scalar(
                text("SELECT count(*) FROM auction_match_lineups WHERE match_id=:id"),
                {"id": uuid.UUID(final_id)},
            )
            == 0
        )
    a = await play(runtime_client, h, a)
    b = await play(runtime_client, h, b)
    final = (await runtime_client.get(f"{PATH}/{final_id}")).json()["data"]
    assert len(final["lineups"]) == 10 and not final["match"]["lineupsLocked"]
    failure(await command(runtime_client, h, a, "last"), "auction_match_downstream_work_exists")
    final = await run(runtime_client, h, final, "LOCK")
    final = await play(runtime_client, h, final, sidePreset="SWAPPED", winner="RED")
    final = await changed(runtime_client, h, final, "complete")
    async with owner.connect() as c:
        assert await c.scalar(text("SELECT count(*) FROM games WHERE status='CONFIRMED'")) == 3
        assert await c.scalar(text("SELECT status FROM auction_tournaments")) == "COMPLETED"
    final = await changed(runtime_client, h, final, "last")
    assert final["match"]["status"] == "READY"
    failure(await command(runtime_client, h, b, "last"), "auction_match_downstream_work_exists")


async def test_match_game_concurrent_revision_and_mmr_tail_guard(owner, runtime_client):
    h, _, state = await prepared(owner, runtime_client)
    replies = await asyncio.gather(
        command(runtime_client, h, state), command(runtime_client, h, state)
    )
    assert sorted(r.status_code for r in replies) == [201, 409]
    state = next(r for r in replies if r.status_code == 201).json()["data"]
    failure(next(r for r in replies if r.status_code == 409), "auction_match_revision_conflict")
    async with owner.begin() as c:
        await c.execute(text("UPDATE user_lane_mmr SET mmr=mmr+1 WHERE games_played>0"))
    failure(await command(runtime_client, h, state, "last"), "auction_match_game_reversal_not_safe")
    async with owner.connect() as c:
        assert await c.scalar(text("SELECT count(*) FROM mmr_ledger_entries")) == 10
        assert await c.scalar(text("SELECT status FROM games")) == "CONFIRMED"


@pytest.mark.parametrize(
    "case", ["negative", "unknown", "missing", "boolean", "invalid_champion", "short_bans"]
)
async def test_match_game_body_contract(owner, runtime_client, case):
    h, _, state = await prepared(owner, runtime_client)
    payload = body(state)
    expected = "invalid_auction_match_game_request"
    if case == "negative":
        payload["expectedRevision"] = -1
    elif case == "unknown":
        payload["unknown"] = True
        expected = "unprocessable_entity"
    elif case == "missing":
        del payload["picks"]
        expected = "unprocessable_entity"
    elif case == "boolean":
        payload["expectedRevision"] = True
        expected = "unprocessable_entity"
    elif case == "invalid_champion":
        payload["picks"][next(iter(payload["picks"]))] = "../Ahri"
    else:
        payload["bans"]["BLUE"] = []
    response = await command(runtime_client, h, state, payload=payload)
    if expected in MESSAGES:
        failure(response, expected, 422)
    else:
        assert_failure(
            response, 422, expected, "요청 JSON의 필드 또는 값이 API 계약과 일치하지 않습니다."
        )


async def test_match_game_late_audit_failure_rolls_back_all_effects(owner, runtime_client):
    h, _, state = await prepared(owner, runtime_client)
    async with owner.connect() as c:
        mmr_before = (
            await c.execute(
                text(
                    "SELECT user_id,lane,mmr,games_played,wins,revision FROM user_lane_mmr "
                    "ORDER BY user_id,lane"
                )
            )
        ).all()
        events_before = await c.scalar(text("SELECT count(*) FROM outbox_events"))
    async with owner.begin() as c:
        await c.execute(text("REVOKE INSERT ON admin_audit_log FROM fastapi_test_runtime"))
    try:
        response = await command(runtime_client, h, state, key="late-failure")
        assert_failure(
            response, 500, "internal_error", "요청을 처리하는 중 내부 오류가 발생했습니다."
        )
    finally:
        async with owner.begin() as c:
            await c.execute(text("GRANT INSERT ON admin_audit_log TO fastapi_test_runtime"))
    async with owner.connect() as c:
        for table in (
            "games",
            "game_inputs",
            "game_participants",
            "game_bans",
            "mmr_ledger_entries",
        ):
            assert await c.scalar(text(f"SELECT count(*) FROM {table}")) == 0
        assert (
            await c.execute(
                text(
                    "SELECT user_id,lane,mmr,games_played,wins,revision FROM user_lane_mmr "
                    "ORDER BY user_id,lane"
                )
            )
        ).all() == mmr_before
        assert await c.scalar(text("SELECT count(*) FROM outbox_events")) == events_before
        assert (
            await c.scalar(
                text(
                    "SELECT count(*) FROM idempotency_records WHERE idempotency_key='late-failure'"
                )
            )
            == 0
        )
    assert (await runtime_client.get(f"{PATH}/{state['match']['id']}")).json()["data"] == state
    assert (await play(runtime_client, h, state, key="late-failure"))["match"]["leftScore"] == 1
