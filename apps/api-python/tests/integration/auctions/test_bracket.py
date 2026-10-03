import asyncio
import hashlib
import json
import uuid

import pytest
from sqlalchemy import text

from tests.support.assertions import assert_failure
from tests.support.auctions.bidding import PATH, prepared
from tests.support.auctions.bidding import request as draft
from tests.support.auctions.bracket import SLOTS, assigned, command, ready, run
from tests.support.guilds.context import GUILD_A, USER_ID
from tests.support.recruitments.creation import BOT

pytestmark = pytest.mark.postgres


@pytest.mark.parametrize("count", [10, 20])
async def test_bracket_assign_lock_unlock_relock_history_and_effects(owner, runtime_client, count):
    h, initial = await ready(owner, runtime_client, count)
    state = initial
    identifier = state["tournament"]["id"]
    url = f"{PATH}/{identifier}/bracket"
    assert set(state) == {"tournament", "teams", "bracket", "permissions"}
    assert state["permissions"] == {
        "canManageBracket": True,
        "canManageMatches": False,
        "canRecoverBracket": False,
        "canRevertDraft": True,
        "canCancel": True,
    }
    assert state["bracket"] == {
        "locked": False,
        "matches": [],
        "slots": [{"slotKey": s, "teamId": None} for s in SLOTS],
    }
    assert state["tournament"]["rules"] == {
        "semifinalBestOf": 1 if count == 20 else None,
        "finalBestOf": 5,
        "hardFearless": False,
    }
    head = await runtime_client.head(url)
    assert head.status_code == 200 and head.content == b""
    for slot, team in zip(SLOTS, state["teams"], strict=False):
        state = await run(runtime_client, h, state, "ASSIGN_TEAM", slotKey=slot, teamId=team["id"])
    before_lock = state
    state = await run(runtime_client, h, state, "LOCK_BRACKET", key="lock")
    assert state["tournament"]["revision"] == before_lock["tournament"]["revision"] + 1
    assert state["tournament"]["status"] == "BRACKET_SETUP"
    assert state["bracket"]["locked"] and state["permissions"]["canRecoverBracket"]
    assert (
        not state["permissions"]["canManageBracket"] and not state["permissions"]["canRevertDraft"]
    )
    matches = state["bracket"]["matches"]
    assert [m["matchKey"] for m in matches] == (
        ["FINAL"] if count == 10 else ["SEMIFINAL_A", "SEMIFINAL_B", "FINAL"]
    )
    assert [m["bestOf"] for m in matches] == ([5] if count == 10 else [1, 1, 5])
    for m in matches:
        assert m["status"] == "LINEUP_PENDING" and m["revision"] == 0
        assert not m["hardFearless"] and not m["lineupsLocked"]
        assert m["leftScore"] == m["rightScore"] == 0 and m["winnerTeamId"] is None
    if count == 20:
        assert matches[-1]["leftTeamId"] is matches[-1]["rightTeamId"] is None
    assert (await runtime_client.get(url)).json()["data"] == state
    assert (await command(runtime_client, h, before_lock, "LOCK_BRACKET", key="lock")).json()[
        "data"
    ] == state
    # Lineup rows alone are recoverable and must survive as history.
    async with owner.begin() as c:
        await c.execute(
            text("""
            INSERT INTO auction_match_lineups
                (guild_id,tournament_id,match_id,team_id,lane,team_member_id,user_id)
            SELECT guild_id,tournament_id,:match,team_id,'TOP',id,user_id FROM auction_team_members
            WHERE guild_id=:g AND tournament_id=:id AND team_id=:team AND roster_slot=1
        """),
            {
                "match": uuid.UUID(matches[0]["id"]),
                "g": GUILD_A,
                "id": uuid.UUID(identifier),
                "team": uuid.UUID(matches[0]["leftTeamId"]),
            },
        )
        await c.execute(
            text(
                "UPDATE auction_matches SET "
                "status='READY',lineups_locked_at=now(),lineups_locked_by=:user "
                "WHERE left_team_id IS NOT NULL AND right_team_id IS NOT NULL"
            ),
            {"user": USER_ID},
        )
        await c.execute(text("UPDATE auction_tournaments SET status='IN_GAME'"))
        notification_count = await c.scalar(text("SELECT count(*) FROM bot_notification_outbox"))
    state = await run(runtime_client, h, state, "UNLOCK_BRACKET")
    assert state["tournament"]["status"] == "BRACKET_SETUP"
    assert not state["bracket"]["locked"] and state["bracket"]["matches"] == []
    assert state["bracket"]["slots"] == before_lock["bracket"]["slots"]
    assert state["permissions"]["canManageBracket"]
    # Historical cancelled matches and lineups must not block a fresh lock.
    state = await run(runtime_client, h, state, "LOCK_BRACKET")
    assert {m["id"] for m in state["bracket"]["matches"]}.isdisjoint(m["id"] for m in matches)
    async with owner.connect() as c:
        old = (
            await c.execute(
                text(
                    "SELECT status,lineups_locked_at,winner_team_id,revision FROM "
                    "auction_matches WHERE status='CANCELLED'"
                )
            )
        ).all()
        assert old == [("CANCELLED", None, None, 1)] * len(matches)
        assert await c.scalar(text("SELECT count(*) FROM auction_match_lineups")) == 1
        assert (
            await c.scalar(text("SELECT count(*) FROM bot_notification_outbox"))
            == notification_count
        )
        assert await c.scalar(text("SELECT count(*) FROM mmr_ledger_entries")) == 0
        audit_count = await c.scalar(
            text("SELECT count(*) FROM admin_audit_log WHERE action LIKE 'AUCTION_BRACKET_%'")
        )
        events = (
            await c.execute(
                text(
                    "SELECT topic,aggregate_revision FROM outbox_events WHERE "
                    "event_type='AUCTION_TOURNAMENT_BRACKET_UPDATED' ORDER BY aggregate_revision"
                )
            )
        ).all()
        assert audit_count == len(events) == count // 5 + 3
        assert events[-1] == (
            f"guild:{GUILD_A}:auction-tournament:{identifier}",
            state["tournament"]["revision"],
        )
        stored = (
            await c.execute(
                text(
                    "SELECT request_hash,status_code,response_body FROM idempotency_records "
                    "WHERE idempotency_key='lock'"
                )
            )
        ).one()
        canonical = json.dumps(
            [
                GUILD_A,
                {"kind": "LOCK_BRACKET", "expectedRevision": before_lock["tournament"]["revision"]},
            ],
            separators=(",", ":"),
        )
        assert stored.request_hash == hashlib.sha256(canonical.encode()).digest()
        assert stored.status_code == 200 and stored.response_body["bracket"]["matches"] == matches


async def test_move_displaces_target_clear_empty_and_draft_revert(owner, runtime_client):
    h, state = await assigned(owner, runtime_client)
    team = state["teams"][0]["id"]
    state = await run(runtime_client, h, state, "ASSIGN_TEAM", slotKey="A_1", teamId=team)
    assert [s["teamId"] for s in state["bracket"]["slots"]] == [None, team, None, None]
    state = await run(runtime_client, h, state, "CLEAR_SLOT", slotKey="A_1")
    revision = state["tournament"]["revision"]
    state = await run(runtime_client, h, state, "CLEAR_SLOT", slotKey="A_1")
    assert state["tournament"]["revision"] == revision + 1
    state = await run(runtime_client, h, state, "ASSIGN_TEAM", slotKey="A_0", teamId=team)
    reverted = await draft(
        runtime_client,
        h,
        state["tournament"]["id"],
        {"kind": "REVERT_LAST", "expectedRevision": state["tournament"]["revision"]},
    )
    assert reverted["tournament"]["status"] == "BIDDING"
    assert_failure(
        await runtime_client.get(f"{PATH}/{state['tournament']['id']}/bracket"),
        409,
        "auction_tournament_bracket_unavailable",
        "경매 드래프트가 완료된 후 브래킷을 확인할 수 있습니다.",
    )
    async with owner.connect() as c:
        assert (
            await c.scalar(
                text("SELECT count(*) FROM auction_bracket_slots WHERE team_id IS NOT NULL")
            )
            == 0
        )
        assert await c.scalar(text("SELECT status FROM auction_recruitments")) == "CONVERTED"


@pytest.mark.parametrize(
    "kind,extra,code,message",
    [
        (
            "LOCK_BRACKET",
            {},
            "auction_tournament_bracket_slots_incomplete",
            "경매 형식에 필요한 모든 브래킷 슬롯에 팀을 배치해야 합니다.",
        ),
        (
            "UNLOCK_BRACKET",
            {},
            "auction_tournament_bracket_recovery_unavailable",
            "게임 입력 또는 결과 기록이 있어 브래킷 확정을 해제할 수 없습니다.",
        ),
        (
            "CLEAR_SLOT",
            {"slotKey": "B_0"},
            "auction_tournament_bracket_slot_unavailable",
            "현재 경매 형식에서는 선택한 브래킷 슬롯을 사용할 수 없습니다.",
        ),
        (
            "CLEAR_SLOT",
            {"slotKey": "unknown"},
            "auction_tournament_bracket_slot_unavailable",
            "현재 경매 형식에서는 선택한 브래킷 슬롯을 사용할 수 없습니다.",
        ),
        (
            "ASSIGN_TEAM",
            {"slotKey": "A_0", "teamId": str(uuid.uuid4())},
            "auction_tournament_bracket_team_not_found",
            "경매 내전에서 선택한 팀을 찾을 수 없습니다.",
        ),
    ],
)
async def test_bracket_command_denials(owner, runtime_client, kind, extra, code, message):
    h, state = await ready(owner, runtime_client)
    assert_failure(
        await command(runtime_client, h, state, kind, **extra),
        422 if kind in {"ASSIGN_TEAM", "CLEAR_SLOT"} else 409,
        code,
        message,
    )
    assert (await runtime_client.get(f"{PATH}/{state['tournament']['id']}/bracket")).json()[
        "data"
    ] == state


async def test_incomplete_roster_and_locked_edit_denials(owner, runtime_client):
    h, state = await assigned(owner, runtime_client)
    async with owner.begin() as c:
        removed = await c.scalar(
            text(
                "UPDATE auction_team_members SET released_at=now() WHERE id=(SELECT id FROM "
                "auction_team_members WHERE roster_slot=5 LIMIT 1) RETURNING id"
            )
        )
    assert_failure(
        await command(runtime_client, h, state, "LOCK_BRACKET"),
        409,
        "auction_tournament_bracket_roster_incomplete",
        "모든 팀의 활성 roster가 5명이어야 브래킷을 확정할 수 있습니다.",
    )
    async with owner.begin() as c:
        await c.execute(
            text("UPDATE auction_team_members SET released_at=NULL WHERE id=:id"), {"id": removed}
        )
    state = await run(runtime_client, h, state, "LOCK_BRACKET")
    assert_failure(
        await command(runtime_client, h, state, "CLEAR_SLOT", slotKey="A_0"),
        409,
        "auction_tournament_bracket_locked",
        "브래킷이 확정되었거나 경기 기록이 있어 변경할 수 없습니다.",
    )


@pytest.mark.parametrize("status", ["DRAFT", "SUBMITTED", "SUPERSEDED"])
async def test_any_game_input_blocks_recovery(owner, runtime_client, status):
    h, state = await assigned(owner, runtime_client)
    state = await run(runtime_client, h, state, "LOCK_BRACKET")
    async with owner.begin() as c:
        await c.execute(
            text("""
            INSERT INTO game_inputs
                (guild_id,id,auction_match_id,set_number,side_preset,winning_side,status,updated_at)
            VALUES (:guild,:id,:match,1,'DEFAULT','BLUE',:status,now())
        """),
            {
                "guild": GUILD_A,
                "id": uuid.uuid7(),
                "match": uuid.UUID(state["bracket"]["matches"][0]["id"]),
                "status": status,
            },
        )
    before = (await runtime_client.get(f"{PATH}/{state['tournament']['id']}/bracket")).json()[
        "data"
    ]
    assert not before["permissions"]["canRecoverBracket"]
    assert_failure(
        await command(runtime_client, h, state, "UNLOCK_BRACKET"),
        409,
        "auction_tournament_bracket_recovery_unavailable",
        "게임 입력 또는 결과 기록이 있어 브래킷 확정을 해제할 수 없습니다.",
    )
    assert (await runtime_client.get(f"{PATH}/{state['tournament']['id']}/bracket")).json()[
        "data"
    ] == before


async def test_concurrent_lock_single_match_and_replay_auth(owner, runtime_client):
    h, state = await assigned(owner, runtime_client)
    responses = await asyncio.gather(
        *[command(runtime_client, h, state, "LOCK_BRACKET", key=f"race-{i}") for i in range(2)]
    )
    assert sorted(r.status_code for r in responses) == [200, 409]
    assert_failure(
        next(r for r in responses if r.status_code == 409),
        409,
        "auction_tournament_revision_conflict",
        "다른 운영자가 경매 브래킷을 변경했습니다. 최신 상태를 확인해 주세요.",
    )
    winner = next(i for i, r in enumerate(responses) if r.status_code == 200)
    key = f"race-{winner}"
    assert (await command(runtime_client, h, state, "LOCK_BRACKET", key=key)).json()[
        "data"
    ] == responses[winner].json()["data"]
    assert_failure(
        await command(runtime_client, h, state, "CLEAR_SLOT", key=key, slotKey="A_0"),
        409,
        "idempotency_conflict",
        "같은 Idempotency-Key가 다른 경매 브래킷 요청에 사용되었습니다.",
    )
    async with owner.begin() as c:
        assert await c.scalar(text("SELECT count(*) FROM auction_matches")) == 1
        await c.execute(text("DELETE FROM guild_member_discord_roles"))
    assert_failure(
        await command(runtime_client, h, state, "LOCK_BRACKET", key=key),
        403,
        "operator_required",
        "경매 브래킷을 변경하려면 현재 길드 운영자 권한이 필요합니다.",
    )
    read = await runtime_client.get(f"{PATH}/{state['tournament']['id']}/bracket")
    assert read.status_code == 200 and not any(read.json()["data"]["permissions"].values())
    async with owner.begin() as c:
        await c.execute(
            text("UPDATE guild_members SET status='INACTIVE' WHERE user_id=:user"),
            {"user": USER_ID},
        )
    assert_failure(
        await command(runtime_client, h, state, "LOCK_BRACKET", key=key),
        403,
        "guild_membership_required",
        "해당 길드의 활성 멤버만 경매 브래킷을 확인하거나 변경할 수 있습니다.",
    )


@pytest.mark.parametrize(
    "table,kind", [("admin_audit_log", "LOCK_BRACKET"), ("outbox_events", "UNLOCK_BRACKET")]
)
async def test_effect_failure_rolls_back_matches_slots_and_revision(
    owner, runtime_client, table, kind
):
    h, state = await assigned(owner, runtime_client)
    if kind == "UNLOCK_BRACKET":
        state = await run(runtime_client, h, state, "LOCK_BRACKET")
    async with owner.begin() as c:
        counts = {
            t: await c.scalar(text(f"SELECT count(*) FROM {t}"))
            for t in ("idempotency_records", "admin_audit_log", "outbox_events", "auction_matches")
        }
        await c.execute(text(f"REVOKE INSERT ON {table} FROM fastapi_test_runtime"))
    try:
        assert_failure(
            await command(runtime_client, h, state, kind, key="failure"),
            500,
            "internal_error",
            "요청을 처리하는 중 내부 오류가 발생했습니다.",
        )
        assert (await runtime_client.get(f"{PATH}/{state['tournament']['id']}/bracket")).json()[
            "data"
        ] == state
        async with owner.connect() as c:
            for t, n in counts.items():
                assert await c.scalar(text(f"SELECT count(*) FROM {t}")) == n
    finally:
        async with owner.begin() as c:
            await c.execute(text(f"GRANT INSERT ON {table} TO fastapi_test_runtime"))
    await run(runtime_client, h, state, kind, key="failure")


@pytest.mark.parametrize(
    "body",
    [
        {"kind": "LOCK_BRACKET", "expectedRevision": -1},
        {"kind": "LOCK_BRACKET", "expectedRevision": True},
        {"kind": "LOCK_BRACKET", "expectedRevision": 0, "extra": 1},
        {"kind": "ASSIGN_TEAM", "expectedRevision": 0, "slotKey": "A_0", "teamId": "bad"},
        {"kind": "CLEAR_SLOT", "expectedRevision": 0, "slotKey": 42},
        {"kind": "OTHER", "expectedRevision": 0},
    ],
)
async def test_strict_bracket_input(owner, runtime_client, body):
    h, source = await prepared(owner, runtime_client)
    state = await draft(runtime_client, h, None, source)
    response = await runtime_client.post(
        f"{PATH}/{state['tournament']['id']}/bracket/commands", json=body, headers=h
    )
    assert_failure(
        response,
        422,
        "invalid_auction_tournament_bracket_command_request",
        "경매 브래킷 명령 형식이 올바르지 않습니다.",
    )


async def test_bracket_auth_tenant_not_found_and_state(owner, runtime_client):
    h, source = await prepared(owner, runtime_client)
    state = await draft(runtime_client, h, None, source)
    url = f"{PATH}/{state['tournament']['id']}/bracket"
    assert_failure(
        await runtime_client.get(url),
        409,
        "auction_tournament_bracket_unavailable",
        "경매 드래프트가 완료된 후 브래킷을 확인할 수 있습니다.",
    )
    assert_failure(
        await command(runtime_client, h, state, "LOCK_BRACKET"),
        409,
        "auction_tournament_bracket_not_editable",
        "브래킷 설정 단계에서만 이 작업을 수행할 수 있습니다.",
    )
    assert_failure(
        await command(runtime_client, {}, state, "LOCK_BRACKET"),
        403,
        "csrf_validation_failed",
        "요청 출처를 확인할 수 없습니다.",
    )
    for target in (url.replace(GUILD_A, "999999"), f"{PATH}/{uuid.uuid7()}/bracket"):
        assert_failure(
            await runtime_client.get(target),
            404,
            "not_found",
            "요청한 경매 내전을 찾을 수 없습니다.",
        )
    runtime_client.cookies.clear()
    assert (
        await runtime_client.get(url, headers={"authorization": f"Bearer {BOT}"})
    ).status_code == 401
    assert (
        await command(runtime_client, {"authorization": f"Bearer {BOT}"}, state, "LOCK_BRACKET")
    ).status_code == 401


async def test_confirmed_scores_and_terminal_history(owner, runtime_client):
    h, state = await assigned(owner, runtime_client)
    state = await run(runtime_client, h, state, "LOCK_BRACKET")
    match = state["bracket"]["matches"][0]
    url = f"{PATH}/{state['tournament']['id']}/bracket"
    async with owner.begin() as c:
        for number, (side, winner, voided) in enumerate(
            [("DEFAULT", "BLUE", False), ("SWAPPED", "BLUE", False), ("DEFAULT", "RED", True)], 1
        ):
            p = {
                "guild": GUILD_A,
                "input": uuid.uuid7(),
                "game": uuid.uuid7(),
                "match": uuid.UUID(match["id"]),
                "number": number,
                "side": side,
                "winner": winner,
                "user": USER_ID,
            }
            await c.execute(
                text("""
                INSERT INTO game_inputs (guild_id,id,auction_match_id,set_number,
                    side_preset,winning_side,status,updated_at)
                VALUES (:guild,:input,:match,:number,:side,:winner,'SUBMITTED',now())
            """),
                p,
            )
            await c.execute(
                text("""
                INSERT INTO games (guild_id,id,season_id,auction_match_id,source_input_id,
                    game_number,side_preset,winning_side,status,mmr_status,played_at,
                    confirmed_by,confirmed_by_type,confirmed_at)
                SELECT :guild,:game,season_id,:match,:input,:number,:side,:winner,
                    'CONFIRMED','LEGACY_NOT_APPLIED',now(),:user,'USER',now()
                FROM auction_matches WHERE guild_id=:guild AND id=:match
            """),
                p,
            )
            if voided:
                await c.execute(
                    text("""
                    UPDATE games SET status='VOIDED',voided_by=:user,voided_by_type='USER',
                        voided_at=now(),void_reason='test history'
                    WHERE guild_id=:guild AND id=:game
                """),
                    p,
                )
        await c.execute(text("UPDATE auction_tournaments SET status='IN_GAME'"))
    snapshot = (await runtime_client.get(url)).json()["data"]
    assert snapshot["bracket"]["matches"][0]["leftScore"] == 1
    assert snapshot["bracket"]["matches"][0]["rightScore"] == 1
    assert not snapshot["permissions"]["canRecoverBracket"]
    # Terminal history remains readable, with all mutation capabilities disabled.
    async with owner.begin() as c:
        await c.execute(
            text("""
            UPDATE auction_tournaments SET status='COMPLETED',ended_at=now(),champion_team_id=:team
        """),
            {"team": uuid.UUID(match["leftTeamId"])},
        )
    completed = await runtime_client.get(url)
    assert completed.status_code == 200 and not any(
        completed.json()["data"]["permissions"].values()
    )
    async with owner.begin() as c:
        await c.execute(
            text("UPDATE auction_tournaments SET status='CANCELLED',champion_team_id=NULL")
        )
        await c.execute(text("UPDATE auction_matches SET status='CANCELLED',ended_at=now()"))
    cancelled = await runtime_client.get(url)
    assert cancelled.status_code == 200 and cancelled.json()["data"]["bracket"]["matches"] == []
    assert not any(cancelled.json()["data"]["permissions"].values())


async def test_missing_slot_backfill_and_canonical_team_id_replay(owner, runtime_client):
    h, state = await ready(owner, runtime_client)
    async with owner.begin() as c:
        await c.execute(text("DELETE FROM auction_bracket_slots WHERE slot_key='B_1'"))
    url = f"{PATH}/{state['tournament']['id']}/bracket"
    assert (await runtime_client.get(url)).json()["data"] == state
    team = state["teams"][0]["id"]
    result = await run(
        runtime_client,
        h,
        state,
        "ASSIGN_TEAM",
        slotKey="A_0",
        teamId=uuid.UUID(team).hex.upper(),
        key="canonical-team",
    )
    replay = await command(
        runtime_client, h, state, "ASSIGN_TEAM", slotKey="A_0", teamId=team, key="canonical-team"
    )
    assert replay.status_code == 200 and replay.json()["data"] == result
    async with owner.connect() as c:
        assert await c.scalar(text("SELECT count(*) FROM auction_bracket_slots")) == 4
        canonical = json.dumps(
            [
                GUILD_A,
                {
                    "kind": "ASSIGN_TEAM",
                    "expectedRevision": state["tournament"]["revision"],
                    "slotKey": "A_0",
                    "teamId": team,
                },
            ],
            separators=(",", ":"),
        )
        assert (
            await c.scalar(
                text(
                    "SELECT request_hash FROM idempotency_records "
                    "WHERE idempotency_key='canonical-team'"
                )
            )
            == hashlib.sha256(canonical.encode()).digest()
        )
