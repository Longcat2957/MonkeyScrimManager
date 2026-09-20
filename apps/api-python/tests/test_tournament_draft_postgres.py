import asyncio
import subprocess
import uuid

import httpx
import pytest
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncEngine
from test_auction_recruitment_postgres import call, filled, teams
from test_auth_crypto import rust_auth
from test_context_postgres import GUILD_A, USER_ID
from test_foundation import assert_failure

pytestmark = pytest.mark.postgres
PATH = f"/api/guilds/{GUILD_A}/auction-tournaments"


async def prepared(owner: AsyncEngine, client: httpx.AsyncClient, count: int = 10):
    headers, source = await filled(owner, client, count=count)
    await call(client, headers, "close", source)
    saved = await call(
        client, headers, "setup", source, {"expectedRevision": count, "teams": teams(count // 5)}
    )
    async with owner.begin() as c:
        await c.execute(
            text("INSERT INTO guild_counters VALUES (:g,'AUCTION_TOURNAMENT',1)"), {"g": GUILD_A}
        )
        await c.execute(
            text("UPDATE auction_recruitments SET channel_id='123456789',message_id='987654321'")
        )
    return headers, {"sourceRecruitmentId": source, "expectedRevision": saved["draft"]["revision"]}


async def rust(action, identifier, body, key):
    return await asyncio.to_thread(
        rust_auth,
        {
            "operation": "auction_tournament",
            "guild_id": GUILD_A,
            "user_id": USER_ID,
            "tournament_id": identifier,
            "action": action,
            "body": body,
            "key": key,
        },
    )


async def request(client, headers, identifier, body, key=None):
    result = await client.post(
        PATH + (f"/{identifier}/commands" if identifier else ""),
        json=body,
        headers={**headers, "idempotency-key": key or str(uuid.uuid7())},
    )
    assert result.status_code == (200 if identifier else 201), result.text
    return result.json()["data"]


@pytest.mark.parametrize("rust_first", [False, True])
async def test_tournament_all_nine_commands_rust_replay(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, rust_first: bool
):
    h, body = await prepared(owner, runtime_client)

    async def both(identifier, body):
        key = str(uuid.uuid7())
        action = "command" if identifier else "create"
        if rust_first:
            expected = await rust(action, identifier, body, key)
            actual = await request(runtime_client, h, identifier, body, key)
        else:
            expected = await request(runtime_client, h, identifier, body, key)
            actual = await rust(action, identifier, body, key)
        assert actual == expected
        return actual

    snapshot = await both(None, body)
    identifier = snapshot["tournament"]["id"]
    assert snapshot["tournament"]["title"] == "10인 경매 내전 #1"
    assert len(snapshot["candidates"]) == 8 and all(
        len(t["members"]) == 1 for t in snapshot["teams"]
    )
    team = snapshot["teams"][0]["id"]

    async def command(kind, **extra):
        nonlocal snapshot
        snapshot = await both(
            identifier,
            {"kind": kind, "expectedRevision": snapshot["tournament"]["revision"], **extra},
        )
        assert (await runtime_client.get(f"{PATH}/{identifier}")).json()["data"] == snapshot
        assert await rust("detail", identifier, {}, "read") == snapshot
        return snapshot

    initial = [r["userId"] for r in snapshot["candidates"]]
    await command("RESHUFFLE_CANDIDATES")
    assert initial != [r["userId"] for r in snapshot["candidates"]]
    await command("DRAW_NEXT")
    user = snapshot["draft"]["currentTarget"]["userId"]
    assert snapshot["draft"]["currentTarget"]["drawCount"] == 1
    await command("MARK_UNSOLD", memberId=user)
    await command("RESTORE_UNSOLD", memberId=user)
    assert snapshot["draft"]["currentTarget"]["drawCount"] == 1
    await command("AWARD_BID", memberId=user, teamId=team, points=125)
    assert snapshot["teams"][0]["currentPoints"] == 875
    await command("REVERT_LAST")
    assert (
        snapshot["teams"][0]["currentPoints"] == 1000
        and snapshot["draft"]["currentTarget"]["userId"] == user
    )
    assert not snapshot["permissions"]["canRevertLast"]
    await command("ASSIGN_CURRENT", memberId=user, teamId=team)
    await command("DRAW_NEXT")
    user = snapshot["draft"]["currentTarget"]["userId"]
    await command("MARK_UNSOLD", memberId=user)
    await command("ASSIGN_UNSOLD", memberId=user, teamId=team)
    await command("REVERT_LAST")
    assert any(r["userId"] == user for r in snapshot["draft"]["unsold"])
    await command("RESTORE_UNSOLD", memberId=user)
    # The reference reset needs ledger DELETE, denied by the actual runtime grant policy.
    # Grant only inside this isolated test to compare the underlying reset algorithm.
    async with owner.begin() as c:
        await c.execute(
            text("GRANT DELETE ON auction_point_ledger,auction_actions TO fastapi_test_runtime")
        )
    try:
        await command("RESET_TO_INITIAL")
    finally:
        async with owner.begin() as c:
            await c.execute(
                text(
                    "REVOKE DELETE ON auction_point_ledger,auction_actions "
                    "FROM fastapi_test_runtime"
                )
            )
    assert snapshot["tournament"]["status"] == "BIDDING"
    assert all(
        len(t["members"]) == 1
        and t["currentPoints"] == t["initialPoints"]
        and t["pointRevision"] == 0
        for t in snapshot["teams"]
    )
    assert all(r["state"] == "QUEUED" and r["drawCount"] == 0 for r in snapshot["candidates"])
    assert snapshot["draft"]["recentAction"] is None
    async with owner.connect() as c:
        for table in ("auction_actions", "auction_lots"):
            assert await c.scalar(text(f"SELECT count(*) FROM {table}")) == 0
        assert await c.scalar(text("SELECT count(*) FROM auction_point_ledger")) == 2
        assert await c.scalar(text("SELECT count(*) FROM mmr_ledger_entries")) == 0
        assert await c.scalar(text("SELECT revision FROM auction_setup_drafts")) == 11


@pytest.mark.parametrize(
    "kind",
    ["DRAW_NEXT", "MARK_UNSOLD", "AWARD_BID", "ASSIGN_CURRENT", "ASSIGN_UNSOLD", "RESTORE_UNSOLD"],
)
async def test_tournament_revert_each_action(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, kind
):
    h, body = await prepared(owner, runtime_client)
    snapshot = await request(runtime_client, h, None, body)
    identifier = snapshot["tournament"]["id"]

    async def command(kind, **extra):
        nonlocal snapshot
        snapshot = await request(
            runtime_client,
            h,
            identifier,
            {"kind": kind, "expectedRevision": snapshot["tournament"]["revision"], **extra},
        )

    await command("DRAW_NEXT")
    user = snapshot["draft"]["currentTarget"]["userId"]
    team = snapshot["teams"][0]["id"]
    if kind in {"MARK_UNSOLD", "ASSIGN_UNSOLD", "RESTORE_UNSOLD"}:
        await command("MARK_UNSOLD", memberId=user)
    if kind in {"ASSIGN_CURRENT", "ASSIGN_UNSOLD", "AWARD_BID"}:
        await command(
            kind, memberId=user, teamId=team, **({"points": 0} if kind == "AWARD_BID" else {})
        )
    elif kind == "RESTORE_UNSOLD":
        await command(kind, memberId=user)
    await command("REVERT_LAST")
    candidate = next(r for r in snapshot["candidates"] if r["userId"] == user)
    assert candidate["state"] == {
        "DRAW_NEXT": "QUEUED",
        "ASSIGN_UNSOLD": "UNSOLD",
        "RESTORE_UNSOLD": "UNSOLD",
    }.get(kind, "ACTIVE")
    assert candidate["drawCount"] == 1
    assert await rust("detail", identifier, {}, "read") == snapshot
    if kind == "ASSIGN_UNSOLD":
        await command("RESTORE_UNSOLD", memberId=user)
        assert snapshot["draft"]["currentTarget"]["userId"] == user


@pytest.mark.parametrize("count", [10, 20])
async def test_tournament_fill_to_bracket_and_revert_preserves_captains(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, count
):
    h, body = await prepared(owner, runtime_client, count)
    snapshot = await request(runtime_client, h, None, body)
    identifier = snapshot["tournament"]["id"]
    for team in snapshot["teams"]:
        for _ in range(4):
            snapshot = await request(
                runtime_client,
                h,
                identifier,
                {"kind": "DRAW_NEXT", "expectedRevision": snapshot["tournament"]["revision"]},
            )
            snapshot = await request(
                runtime_client,
                h,
                identifier,
                {
                    "kind": "AWARD_BID",
                    "expectedRevision": snapshot["tournament"]["revision"],
                    "memberId": snapshot["draft"]["currentTarget"]["userId"],
                    "teamId": team["id"],
                    "points": 10,
                },
            )
    assert (
        snapshot["tournament"]["status"] == "BRACKET_SETUP"
        and snapshot["permissions"]["canRevertLast"]
    )
    assert not snapshot["permissions"]["canManageDraft"]
    response = await runtime_client.get(PATH + "?state=active")
    assert response.json()["data"]["tournaments"][0]["href"].endswith("/bracket")
    snapshot = await request(
        runtime_client,
        h,
        identifier,
        {"kind": "REVERT_LAST", "expectedRevision": snapshot["tournament"]["revision"]},
    )
    assert (
        snapshot["tournament"]["status"] == "BIDDING"
        and snapshot["draft"]["currentTarget"] is not None
    )
    assert await rust("detail", identifier, {}, "read") == snapshot


async def test_tournament_optimistic_concurrency_and_permission_before_replay(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
):
    h, body = await prepared(owner, runtime_client)
    created = await request(runtime_client, h, None, body, "create")
    identifier = created["tournament"]["id"]
    responses = await asyncio.gather(
        *[
            runtime_client.post(
                f"{PATH}/{identifier}/commands",
                json={"kind": "DRAW_NEXT", "expectedRevision": 0},
                headers={**h, "idempotency-key": str(i)},
            )
            for i in range(2)
        ]
    )
    assert sorted(r.status_code for r in responses) == [200, 409]
    denied = next(r for r in responses if r.status_code == 409)
    assert_failure(
        denied,
        409,
        "auction_tournament_revision_conflict",
        "다른 운영자가 경매 드래프트를 변경했습니다. 최신 상태를 확인해 주세요.",
    )
    async with owner.begin() as c:
        await c.execute(text("DELETE FROM guild_member_discord_roles"))
    response = await runtime_client.post(
        PATH, json=body, headers={**h, "idempotency-key": "create"}
    )
    assert_failure(
        response,
        403,
        "operator_required",
        "경매 내전을 시작하려면 현재 길드 운영자 권한이 필요합니다.",
    )
    detail = (await runtime_client.get(f"{PATH}/{identifier}")).json()["data"]
    assert not any(detail["permissions"].values())


async def test_tournament_reset_preserves_rust_runtime_grant_failure(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
):
    h, body = await prepared(owner, runtime_client)
    original = await request(runtime_client, h, None, body)
    identifier = original["tournament"]["id"]
    reset = {"kind": "RESET_TO_INITIAL", "expectedRevision": 0}
    response = await runtime_client.post(f"{PATH}/{identifier}/commands", json=reset, headers=h)
    assert_failure(response, 500, "internal_error", "요청을 처리하는 중 내부 오류가 발생했습니다.")
    with pytest.raises(subprocess.CalledProcessError) as failure:
        await rust("command", identifier, reset, "rust-reset")
    assert "42501" in failure.value.stderr and "auction_point_ledger" in failure.value.stderr
    assert (await runtime_client.get(f"{PATH}/{identifier}")).json()["data"] == original


@pytest.mark.parametrize(
    "body,status,code,message",
    [
        (
            {"kind": "REVERT_LAST"},
            409,
            "auction_draft_no_revertable_action",
            "되돌릴 수 있는 마지막 경매 작업이 없습니다.",
        ),
        (
            {"kind": "MARK_UNSOLD", "memberId": "1002"},
            409,
            "auction_draft_current_target_required",
            "현재 추첨된 후보가 있어야 이 작업을 수행할 수 있습니다.",
        ),
        (
            {"kind": "RESTORE_UNSOLD", "memberId": "9999"},
            422,
            "auction_draft_candidate_not_found",
            "경매 후보 풀에서 해당 참가자를 찾을 수 없습니다.",
        ),
        (
            {"kind": "RESTORE_UNSOLD", "memberId": "1002"},
            409,
            "auction_draft_candidate_state_mismatch",
            "선택한 후보의 현재 상태에서는 이 작업을 수행할 수 없습니다.",
        ),
        (
            {"kind": "DRAW_NEXT", "expectedRevision": 3},
            409,
            "auction_tournament_revision_conflict",
            "다른 운영자가 경매 드래프트를 변경했습니다. 최신 상태를 확인해 주세요.",
        ),
        (
            {"kind": "DRAW_NEXT", "expectedRevision": -1},
            422,
            "invalid_auction_tournament_draft_command_request",
            "경매 드래프트 명령 형식이 올바르지 않습니다.",
        ),
        (
            {"kind": "DRAW_NEXT", "extra": True},
            422,
            "invalid_auction_tournament_draft_command_request",
            "경매 드래프트 명령 형식이 올바르지 않습니다.",
        ),
        (
            {"kind": "WRONG"},
            422,
            "invalid_auction_tournament_draft_command_request",
            "경매 드래프트 명령 형식이 올바르지 않습니다.",
        ),
    ],
)
async def test_tournament_command_denials(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, body, status, code, message
):
    h, source = await prepared(owner, runtime_client)
    original = await request(runtime_client, h, None, source)
    identifier = original["tournament"]["id"]
    response = await runtime_client.post(
        f"{PATH}/{identifier}/commands", json={"expectedRevision": 0, **body}, headers=h
    )
    assert_failure(response, status, code, message)
    assert (await runtime_client.get(f"{PATH}/{identifier}")).json()["data"] == original


@pytest.mark.parametrize(
    "fault", ["negative", "over", "wrong_target", "wrong_team", "active_draw", "active_restore"]
)
async def test_tournament_active_target_errors(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, fault
):
    h, body = await prepared(owner, runtime_client)
    created = await request(runtime_client, h, None, body)
    identifier = created["tournament"]["id"]
    active = await request(
        runtime_client, h, identifier, {"kind": "DRAW_NEXT", "expectedRevision": 0}
    )
    body = {
        "kind": "AWARD_BID",
        "expectedRevision": 1,
        "memberId": active["draft"]["currentTarget"]["userId"],
        "teamId": active["teams"][0]["id"],
        "points": 1,
    }
    if fault in {"negative", "over"}:
        body["points"] = -1 if fault == "negative" else 1001
        status, code, message = (
            422,
            "invalid_auction_draft_bid_points",
            "낙찰 포인트는 팀의 현재 포인트 범위에서 입력해야 합니다.",
        )
    elif fault == "wrong_target":
        body["memberId"] = "9999"
        status, code, message = (
            409,
            "auction_draft_current_target_mismatch",
            "선택한 후보가 현재 추첨 대상과 다릅니다. 최신 상태를 확인해 주세요.",
        )
    elif fault == "wrong_team":
        body["teamId"] = str(uuid.uuid7())
        status, code, message = (
            422,
            "auction_draft_team_not_found",
            "경매 내전에서 해당 팀을 찾을 수 없습니다.",
        )
    else:
        body = {
            "kind": "DRAW_NEXT" if fault == "active_draw" else "RESTORE_UNSOLD",
            "expectedRevision": 1,
        }
        if fault == "active_restore":
            body["memberId"] = "1002"
        status, code, message = (
            409,
            "auction_draft_current_target_exists",
            "현재 추첨된 후보를 먼저 처리해야 합니다.",
        )
    response = await runtime_client.post(f"{PATH}/{identifier}/commands", json=body, headers=h)
    assert_failure(response, status, code, message)
    assert (await runtime_client.get(f"{PATH}/{identifier}")).json()["data"] == active


async def test_tournament_award_audit_failure_rolls_back_points_and_roster(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
):
    h, body = await prepared(owner, runtime_client)
    created = await request(runtime_client, h, None, body)
    identifier = created["tournament"]["id"]
    original = await request(
        runtime_client, h, identifier, {"kind": "DRAW_NEXT", "expectedRevision": 0}
    )
    async with owner.begin() as c:
        await c.execute(
            text(
                "ALTER TABLE admin_audit_log "
                "ADD CONSTRAINT tournament_test_failure CHECK (action<>'AUCTION_DRAFT_AWARD_BID')"
            )
        )
    try:
        response = await runtime_client.post(
            f"{PATH}/{identifier}/commands",
            json={
                "kind": "AWARD_BID",
                "expectedRevision": 1,
                "memberId": original["draft"]["currentTarget"]["userId"],
                "teamId": original["teams"][0]["id"],
                "points": 500,
            },
            headers=h,
        )
        assert_failure(
            response, 500, "internal_error", "요청을 처리하는 중 내부 오류가 발생했습니다."
        )
        assert (await runtime_client.get(f"{PATH}/{identifier}")).json()["data"] == original
        async with owner.connect() as c:
            assert (
                await c.scalar(
                    text("SELECT count(*) FROM auction_point_ledger WHERE reason='BID_DEBIT'")
                )
                == 0
            )
            assert await c.scalar(text("SELECT count(*) FROM auction_actions")) == 1
    finally:
        async with owner.begin() as c:
            await c.execute(
                text("ALTER TABLE admin_audit_log DROP CONSTRAINT tournament_test_failure")
            )


async def test_tournament_create_race_and_outbox_rollback(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
):
    h, body = await prepared(owner, runtime_client)
    async with owner.begin() as c:
        await c.execute(
            text(
                "ALTER TABLE outbox_events "
                "ADD CONSTRAINT tournament_create_failure CHECK "
                "(event_type<>'AUCTION_TOURNAMENT_CREATED')"
            )
        )
    try:
        response = await runtime_client.post(PATH, json=body, headers=h)
        assert_failure(
            response, 500, "internal_error", "요청을 처리하는 중 내부 오류가 발생했습니다."
        )
        async with owner.connect() as c:
            for table in (
                "auction_tournaments",
                "auction_teams",
                "auction_team_members",
                "auction_point_ledger",
                "auction_bid_candidates",
                "auction_bracket_slots",
            ):
                assert await c.scalar(text(f"SELECT count(*) FROM {table}")) == 0
            assert await c.scalar(text("SELECT status FROM auction_recruitments")) == "CLOSED"
            assert (
                await c.scalar(
                    text(
                        "SELECT next_value FROM guild_counters "
                        "WHERE counter_key='AUCTION_TOURNAMENT'"
                    )
                )
                == 1
            )
    finally:
        async with owner.begin() as c:
            await c.execute(
                text("ALTER TABLE outbox_events DROP CONSTRAINT tournament_create_failure")
            )
    responses = await asyncio.gather(
        *[
            runtime_client.post(PATH, json=body, headers={**h, "idempotency-key": str(i)})
            for i in range(2)
        ]
    )
    assert sorted(r.status_code for r in responses) == [201, 409]
    assert_failure(
        next(r for r in responses if r.status_code == 409),
        409,
        "auction_tournament_source_not_ready",
        "모집 마감 후에만 경매 내전을 시작할 수 있습니다.",
    )


async def test_tournament_lists_bot_session_season_and_limit(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
):
    from test_recruitment_create_postgres import BOT

    h, body = await prepared(owner, runtime_client)
    first = await request(runtime_client, h, None, body)
    identifier = first["tournament"]["id"]
    async with owner.begin() as c:
        season = await c.scalar(text("SELECT id FROM seasons"))
        # Historical cancelled tournament permits the same recruitment as a source.
        await c.execute(
            text("""INSERT INTO auction_tournaments
            (guild_id,id,season_id,source_recruitment_id,display_number,title,format,status,revision,
             started_at,ended_at,created_by,updated_at)
            SELECT guild_id,:id,season_id,source_recruitment_id,2,'지난 경매',format,'CANCELLED',0,
                started_at,now(),created_by,now() FROM auction_tournaments"""),
            {"id": uuid.uuid7()},
        )
    for headers in (h, {"authorization": f"Bearer {BOT}"}):
        result = await runtime_client.get(
            PATH + f"?state=active&seasonId={season}&limit=1", headers=headers
        )
        assert (
            result.status_code == 200
            and result.json()["data"]["tournaments"][0]["id"] == identifier
        )
        assert result.json()["data"]["tournaments"][0]["activeMemberCount"] == 2
        completed = await runtime_client.get(PATH + "?state=completed", headers=headers)
        assert completed.json()["data"]["tournaments"][0]["href"].endswith("/result")
    missing = await runtime_client.get(PATH + f"?state=active&seasonId={uuid.uuid7()}")
    assert missing.json()["data"]["tournaments"] == []
    for query in (
        "state=other",
        "state=active&limit=0",
        "state=active&cursor=bad",
        "state=active&seasonId=bad",
    ):
        response = await runtime_client.get(PATH + "?" + query)
        assert response.status_code == 400 and response.json()["error"]["message"]


async def test_tournament_match_artifact_locks_commands(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
):
    h, body = await prepared(owner, runtime_client)
    created = await request(runtime_client, h, None, body)
    identifier = created["tournament"]["id"]
    async with owner.begin() as c:
        await c.execute(
            text("""INSERT INTO auction_matches
            (guild_id,tournament_id,id,season_id,match_key,best_of,status,revision,updated_at)
            SELECT guild_id,id,:match,season_id,'FINAL',1,'LINEUP_PENDING',0,now()
            FROM auction_tournaments WHERE id=:tournament"""),
            {"match": uuid.uuid7(), "tournament": uuid.UUID(identifier)},
        )
    original = (await runtime_client.get(f"{PATH}/{identifier}")).json()["data"]
    assert not original["permissions"]["canManageDraft"]
    assert not original["permissions"]["canRevertLast"]
    assert await rust("detail", identifier, {}, "read") == original
    for kind in ("DRAW_NEXT", "RESHUFFLE_CANDIDATES", "REVERT_LAST", "RESET_TO_INITIAL"):
        response = await runtime_client.post(
            f"{PATH}/{identifier}/commands",
            json={"kind": kind, "expectedRevision": 0},
            headers=h,
        )
        assert_failure(
            response,
            409,
            "auction_tournament_draft_locked",
            "대진 또는 경기 기록이 있어 경매 드래프트를 변경할 수 없습니다.",
        )
    assert (await runtime_client.get(f"{PATH}/{identifier}")).json()["data"] == original


async def test_tournament_current_target_profile_and_mmr_snapshot(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
):
    import json

    h, body = await prepared(owner, runtime_client)
    created = await request(runtime_client, h, None, body)
    identifier = created["tournament"]["id"]
    target = created["candidates"][0]
    user = target["userId"]
    records = {
        "account": {"puuid": "profile-test", "gameName": "Player", "tagLine": "KR1"},
        "platform": "KR",
        "routingRegion": "ASIA",
        "ranks": [
            {
                "queueType": "RANKED_SOLO_5x5",
                "tier": "DIAMOND",
                "rank": "II",
                "leaguePoints": 10,
                "wins": 2,
                "losses": 1,
            }
        ],
        "championAggregates": [],
        "recentMatches": [],
        "sample": {
            "recentMatches": {"requested": 20, "returned": 0},
            "championAggregates": {
                "seasonStartUnixSeconds": 1,
                "discoveredMatches": 0,
                "analyzedMatches": 0,
                "truncated": False,
            },
        },
        "refreshedAt": "test-nonempty",
        "cache": {"state": "fresh"},
    }
    async with owner.begin() as c:
        await c.execute(
            text("UPDATE users SET avatar_hash='a_0123abcd' WHERE user_id=:u"), {"u": user}
        )
        await c.execute(
            text("UPDATE user_lane_mmr SET mmr=1888,games_played=4,wins=3 WHERE user_id=:u"),
            {"u": user},
        )
        await c.execute(
            text("""INSERT INTO user_champion_preferences
            (user_id,lane,position,champion_id) VALUES (:u,:lane,1,'Ahri')"""),
            {"u": user, "lane": target["declaredLane"]},
        )
        await c.execute(
            text("""INSERT INTO riot_accounts
            (puuid,user_id,platform,routing_region,game_name,tag_line,normalized_game_name,
             normalized_tag_line,profile_icon_id,is_main,created_at,updated_at,origin)
            VALUES ('profile-test',:u,'KR','ASIA','Player','KR1','player','kr1',
                    29,true,now(),now(),'RIOT')"""),
            {"u": user},
        )
        await c.execute(
            text("""INSERT INTO riot_records_snapshots
            (platform,puuid,routing_region,game_name,tag_line,normalized_game_name,
             normalized_tag_line,schema_version,snapshot,refreshed_at,created_at,updated_at)
            VALUES ('KR','profile-test','ASIA','Player','KR1','player','kr1',1,
                CAST(:snapshot AS jsonb),now(),now(),now())"""),
            {"snapshot": json.dumps(records)},
        )
    drawn = await request(
        runtime_client, h, identifier, {"kind": "DRAW_NEXT", "expectedRevision": 0}
    )
    assert drawn == await rust("detail", identifier, {}, "profile")
    profile = drawn["draft"]["currentTargetProfile"]
    assert profile["summary"] == {"games": 4, "wins": 3, "losses": 1}
    assert (
        profile["riot"]["games"] == 0
        and profile["riotTier"] == "D2"
        and profile["profileIconId"] == 29
    )
    assert profile["championIds"] == ["Ahri"] and profile["avatarUrl"].endswith(
        "/a_0123abcd.webp?size=128"
    )
    assert drawn["draft"]["currentTarget"]["mmrAtStart"] == 1500
    awarded = await request(
        runtime_client,
        h,
        identifier,
        {
            "kind": "AWARD_BID",
            "expectedRevision": 1,
            "memberId": user,
            "teamId": drawn["teams"][0]["id"],
            "points": 0,
        },
    )
    acquired = next(r for r in awarded["teams"][0]["members"] if r["userId"] == user)
    assert acquired["mmrAtAcquisition"] == 1500
    await request(runtime_client, h, identifier, {"kind": "REVERT_LAST", "expectedRevision": 2})
    for bad in (
        {},
        {
            **records,
            "sample": {**records["sample"], "recentMatches": {"requested": 20, "returned": 1}},
        },
    ):
        async with owner.begin() as c:
            await c.execute(
                text("UPDATE riot_records_snapshots SET snapshot=CAST(:s AS jsonb)"),
                {"s": json.dumps(bad)},
            )
        detail = (await runtime_client.get(f"{PATH}/{identifier}")).json()["data"]
        assert detail == await rust("detail", identifier, {}, "bad-profile")
        assert detail["draft"]["currentTargetProfile"]["riot"] is None
