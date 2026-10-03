import asyncio
import hashlib
import json
import uuid

import pytest
from sqlalchemy import text

from monkeybot_api.schemas.users import LANES
from tests.support.assertions import assert_failure
from tests.support.auctions.lineups import PATH, command, denied, fill, run, seeded
from tests.support.guilds.context import GUILD_A, GUILD_B, ROLE_B, USER_ID, guild_fixture
from tests.support.recruitments.creation import BOT

pytestmark = pytest.mark.postgres


@pytest.mark.parametrize("count", [10, 20])
async def test_lineups_lock_barrier_unlock_and_immutable_notification(owner, runtime_client, count):
    h, bracket, initial = await seeded(owner, runtime_client, count)
    url = f"{PATH}/{initial['match']['id']}"
    assert set(initial) == {"match", "teams", "lineups", "games", "permissions"}
    assert initial["lineups"] == initial["games"] == []
    assert initial["permissions"] == {
        "canManageLineup": True,
        "canEnterResults": False,
        "canUnlockLineup": False,
        "canRevertLastGame": False,
        "canCompleteFinalAsBo1": False,
        "canFinishTournament": True,
    }
    assert initial["match"]["label"] == ("결승" if count == 10 else "준결승 A")
    head = await runtime_client.head(url)
    assert head.status_code == 200 and head.content == b""
    async with owner.begin() as c:
        await c.execute(text("UPDATE guild_config SET entry_channel_id='123456789012345678'"))
    before_lock = await fill(runtime_client, h, initial)
    assert before_lock["match"]["revision"] == 10
    state = await run(runtime_client, h, before_lock, "LOCK", key="lineup-lock")
    assert state["match"]["status"] == "READY" and state["match"]["lineupsLocked"]
    assert state["permissions"]["canUnlockLineup"]
    assert state["permissions"]["canEnterResults"] == (count == 10)
    assert not state["permissions"]["canManageLineup"]
    assert state["lineups"] == before_lock["lineups"]
    assert [r["lane"] for r in state["lineups"]] == list(LANES) * 2
    async with owner.connect() as c:
        tournament = (
            await c.execute(text("SELECT status,revision FROM auction_tournaments"))
        ).one()
        assert tournament.status == "IN_GAME"
        assert tournament.revision == bracket["tournament"]["revision"] + 1
        payload = await c.scalar(
            text(
                "SELECT payload FROM bot_notification_outbox "
                "WHERE notification_type='AUCTION_MATCH_LINEUP_CONFIRMATION_CREATE'"
            )
        )
        assert payload["schemaVersion"] == 1 and payload["matchId"] == state["match"]["id"]
        notice = payload["snapshot"]
        assert notice["tournament"] == {
            "id": bracket["tournament"]["id"],
            "title": bracket["tournament"]["title"],
            "displayNumber": 1,
            "format": "TEN_PERSON" if count == 10 else "TWENTY_PERSON",
            "hardFearless": False,
        }
        assert notice["matchup"]["label"] == state["match"]["label"]
        assert notice["links"]["pickBan"].endswith(state["match"]["id"])
        assert all([p["lane"] for p in t["players"]] == list(LANES) for t in notice["teams"])
        stored = (
            await c.execute(
                text(
                    "SELECT request_hash,response_body FROM idempotency_records "
                    "WHERE idempotency_key='lineup-lock'"
                )
            )
        ).one()
        serialized = json.dumps(
            [
                GUILD_A,
                {
                    "match_id": state["match"]["id"],
                    "command": {"kind": "LOCK", "expectedRevision": 10},
                },
            ],
            separators=(",", ":"),
        )
        assert stored.request_hash == hashlib.sha256(serialized.encode()).digest()
        assert stored.response_body == state
    assert (await run(runtime_client, h, before_lock, "LOCK", key="lineup-lock")) == state
    if count == 20:
        pending = bracket["bracket"]["matches"][-1]["id"]
        assert_failure(
            await runtime_client.get(f"{PATH}/{pending}"),
            409,
            "auction_match_teams_pending",
            "이 경기는 아직 양 팀이 확정되지 않았습니다.",
        )
        second_id = bracket["bracket"]["matches"][1]["id"]
        second = (await runtime_client.get(f"{PATH}/{second_id}")).json()["data"]
        denied(
            await command(runtime_client, h, second, "LOCK", key="lineup-lock"),
            "idempotency_conflict",
        )
        second = await fill(runtime_client, h, second)
        second = await run(runtime_client, h, second, "LOCK")
        assert second["permissions"]["canEnterResults"]
        state = (await runtime_client.get(url)).json()["data"]
        assert state["permissions"]["canEnterResults"]
        second = await run(runtime_client, h, second, "UNLOCK")
        assert not second["permissions"]["canEnterResults"]
        assert not (await runtime_client.get(url)).json()["data"]["permissions"]["canEnterResults"]
        async with owner.connect() as c:
            assert await c.scalar(text("SELECT status FROM auction_tournaments")) == "IN_GAME"
    state = await run(runtime_client, h, state, "UNLOCK")
    assert state["match"]["status"] == "LINEUP_PENDING"
    assert state["lineups"] == before_lock["lineups"] and state["permissions"]["canManageLineup"]
    async with owner.begin() as c:
        assert await c.scalar(text("SELECT status FROM auction_tournaments")) == "BRACKET_SETUP"
        await c.execute(text("UPDATE guild_members SET display_name='변경한 이름'"))
        old = await c.scalar(
            text("SELECT payload FROM bot_notification_outbox WHERE deduplication_key=:key"),
            {"key": f"auction-match:{state['match']['id']}:lineup-confirmation:11"},
        )
        assert old == payload
    await run(runtime_client, h, state, "LOCK")
    async with owner.connect() as c:
        notices = (
            (
                await c.execute(
                    text(
                        "SELECT payload FROM bot_notification_outbox "
                        "WHERE notification_type='AUCTION_MATCH_LINEUP_CONFIRMATION_CREATE'"
                    )
                )
            )
            .scalars()
            .all()
        )
        assert len(notices) == (2 if count == 10 else 3)
        assert any(
            n["snapshot"]["teams"][0]["players"][0]["displayName"] == "변경한 이름" for n in notices
        )
        events = await c.scalar(
            text(
                "SELECT count(*) FROM outbox_events WHERE event_type='AUCTION_MATCH_LINEUP_UPDATED'"
            )
        )
        audits = await c.scalar(
            text("SELECT count(*) FROM admin_audit_log WHERE target_type='AUCTION_MATCH'")
        )
        assert events == audits == (13 if count == 10 else 25)


async def test_assignment_denials_clearing_revision_and_hash(owner, runtime_client):
    h, _, state = await seeded(owner, runtime_client)
    denied(await command(runtime_client, h, state, "LOCK"), "auction_match_lineup_incomplete")
    denied(
        await command(runtime_client, h, state, "UNLOCK"), "auction_match_lineup_unlock_unavailable"
    )
    team = state["teams"][0]
    body = {
        "teamId": team["id"].upper(),
        "lane": "TOP",
        "teamMemberId": team["members"][0]["teamMemberId"].upper(),
    }
    for extra, code in [
        ({**body, "teamId": str(uuid.uuid7())}, "auction_match_lineup_team_not_found"),
        ({**body, "teamMemberId": str(uuid.uuid7())}, "auction_match_lineup_member_not_found"),
    ]:
        denied(await command(runtime_client, h, state, "ASSIGN", **extra), code)
    initial = state
    state = await run(runtime_client, h, state, "ASSIGN", key="assignment", **body)
    denied(
        await command(runtime_client, h, state, "ASSIGN", **{**body, "lane": "MID"}),
        "auction_match_lineup_player_assigned",
    )
    conflict = await command(runtime_client, h, initial, "ASSIGN", **body)
    denied(conflict, "auction_match_revision_conflict")
    assert conflict.json()["error"]["details"] == {"expectedRevision": 0, "actualRevision": 1}
    # Serde canonicalizes UUIDs, serializes omitted Option as null, and hashes match identity.
    async with owner.connect() as c:
        stored = await c.scalar(
            text("SELECT request_hash FROM idempotency_records WHERE idempotency_key='assignment'")
        )
    canonical = {
        "kind": "ASSIGN",
        "expectedRevision": 0,
        "teamId": team["id"],
        "lane": "TOP",
        "teamMemberId": team["members"][0]["teamMemberId"],
    }
    expected = json.dumps(
        [GUILD_A, {"match_id": state["match"]["id"], "command": canonical}], separators=(",", ":")
    )
    assert stored == hashlib.sha256(expected.encode()).digest()
    state = await run(runtime_client, h, state, "ASSIGN", teamId=team["id"], lane="TOP")
    assert state["lineups"] == []
    state = await run(
        runtime_client, h, state, "ASSIGN", teamId=team["id"], lane="TOP", teamMemberId=None
    )
    assert state["match"]["revision"] == 3
    state = await fill(runtime_client, h, state)
    state = await run(runtime_client, h, state, "LOCK")
    denied(
        await command(runtime_client, h, state, "ASSIGN", **body),
        "auction_match_lineup_not_editable",
    )


@pytest.mark.parametrize("status", ["DRAFT", "SUBMITTED", "SUPERSEDED"])
async def test_any_result_input_blocks_unlock(owner, runtime_client, status):
    h, _, state = await seeded(owner, runtime_client)
    state = await fill(runtime_client, h, state)
    state = await run(runtime_client, h, state, "LOCK")
    async with owner.begin() as c:
        await c.execute(
            text("""
            INSERT INTO game_inputs (guild_id,id,auction_match_id,set_number,side_preset,
                winning_side,status,updated_at)
            VALUES (:guild,:id,:match,1,'DEFAULT','BLUE',:status,now())
        """),
            {
                "guild": GUILD_A,
                "id": uuid.uuid7(),
                "match": uuid.UUID(state["match"]["id"]),
                "status": status,
            },
        )
    denied(
        await command(runtime_client, h, state, "UNLOCK"),
        "auction_match_lineup_result_artifacts_present",
    )
    current = (await runtime_client.get(f"{PATH}/{state['match']['id']}")).json()["data"]
    assert not current["permissions"]["canUnlockLineup"]
    assert current["match"] == state["match"]


async def test_concurrency_replay_and_current_authority(owner, runtime_client):
    h, _, state = await seeded(owner, runtime_client)
    state = await fill(runtime_client, h, state)
    replies = await asyncio.gather(
        *[command(runtime_client, h, state, "LOCK", key=f"race-{n}") for n in range(2)]
    )
    assert sorted(r.status_code for r in replies) == [200, 409]
    winner = next(i for i, r in enumerate(replies) if r.status_code == 200)
    denied(replies[1 - winner], "auction_match_revision_conflict")
    assert (
        await run(runtime_client, h, state, "LOCK", key=f"race-{winner}")
        == replies[winner].json()["data"]
    )
    async with owner.begin() as c:
        await c.execute(text("DELETE FROM guild_member_discord_roles"))
    denied(
        await command(runtime_client, h, state, "LOCK", key=f"race-{winner}"), "operator_required"
    )
    read = (await runtime_client.get(f"{PATH}/{state['match']['id']}")).json()["data"]
    assert not any(read["permissions"].values())
    async with owner.begin() as c:
        await c.execute(
            text("UPDATE guild_members SET status='INACTIVE' WHERE user_id=:user"),
            {"user": USER_ID},
        )
    denied(
        await command(runtime_client, h, state, "LOCK", key=f"race-{winner}"),
        "guild_membership_required",
    )
    assert_failure(
        await runtime_client.get(f"{PATH}/{state['match']['id']}"),
        404,
        "not_found",
        "요청한 길드를 찾을 수 없습니다.",
    )


@pytest.mark.parametrize(
    "table,kind",
    [
        ("bot_notification_outbox", "LOCK"),
        ("admin_audit_log", "ASSIGN"),
        ("outbox_events", "UNLOCK"),
    ],
)
async def test_effect_failure_rolls_back_lineup_tournament_and_idempotency(
    owner, runtime_client, table, kind
):
    h, _, state = await seeded(owner, runtime_client)
    state = await fill(runtime_client, h, state)
    if kind == "UNLOCK":
        state = await run(runtime_client, h, state, "LOCK")
    extra = {"teamId": state["teams"][0]["id"], "lane": "TOP"} if kind == "ASSIGN" else {}
    async with owner.begin() as c:
        await c.execute(text("UPDATE guild_config SET entry_channel_id='123456789012345678'"))
        tournament = (
            await c.execute(text("SELECT status,revision FROM auction_tournaments"))
        ).one()
        counts = {
            t: await c.scalar(text(f"SELECT count(*) FROM {t}"))
            for t in (
                "idempotency_records",
                "admin_audit_log",
                "outbox_events",
                "bot_notification_outbox",
            )
        }
        await c.execute(text(f"REVOKE INSERT ON {table} FROM fastapi_test_runtime"))
    try:
        assert_failure(
            await command(runtime_client, h, state, kind, key="retry", **extra),
            500,
            "internal_error",
            "요청을 처리하는 중 내부 오류가 발생했습니다.",
        )
        assert (await runtime_client.get(f"{PATH}/{state['match']['id']}")).json()["data"] == state
        async with owner.connect() as c:
            assert (
                await c.execute(text("SELECT status,revision FROM auction_tournaments"))
            ).one() == tournament
            for t, n in counts.items():
                assert await c.scalar(text(f"SELECT count(*) FROM {t}")) == n
    finally:
        async with owner.begin() as c:
            await c.execute(text(f"GRANT INSERT ON {table} TO fastapi_test_runtime"))
    await run(runtime_client, h, state, kind, key="retry", **extra)


async def test_input_auth_tenant_state_error_priority(owner, runtime_client):
    h, _, state = await seeded(owner, runtime_client)
    url = f"{PATH}/{state['match']['id']}"
    for body in (
        {"expectedRevision": True},
        {"expectedRevision": 0, "extra": 1},
        {"expectedRevision": 2**63},
    ):
        assert_failure(
            await runtime_client.post(url + "/lineup/lock", json=body),
            422,
            "unprocessable_entity",
            "요청 JSON의 필드 또는 값이 API 계약과 일치하지 않습니다.",
        )
    denied(
        await runtime_client.post(url + "/lineup/lock", json={"expectedRevision": -1}),
        "invalid_auction_match_lineup_request",
    )
    assert_failure(
        await command(runtime_client, {}, state, "LOCK"),
        403,
        "csrf_validation_failed",
        "요청 출처를 확인할 수 없습니다.",
    )
    assert_failure(
        await runtime_client.get(f"{PATH}/{uuid.uuid7()}"),
        404,
        "not_found",
        "요청한 경매 경기를 찾을 수 없습니다.",
    )
    await guild_fixture(owner, GUILD_B, "Other", ROLE_B)
    assert_failure(
        await runtime_client.get(url.replace(GUILD_A, GUILD_B)),
        404,
        "not_found",
        "요청한 경매 경기를 찾을 수 없습니다.",
    )
    async with owner.begin() as c:
        await c.execute(text("UPDATE auction_tournaments SET status='BIDDING'"))
    denied(await command(runtime_client, h, state, "LOCK"), "auction_match_tournament_not_ready")
    runtime_client.cookies.clear()
    assert_failure(
        await runtime_client.get(url, headers={"authorization": f"Bearer {BOT}"}),
        401,
        "unauthorized",
        "인증이 필요합니다.",
    )
