import asyncio

import pytest
from sqlalchemy import text
from test_auth_crypto import rust_auth
from test_context_postgres import GUILD_A, ROLE_A, USER_ID
from test_foundation import assert_failure
from test_recruitment_create_postgres import BOT
from test_recruitment_lifecycle_postgres import call, fixture, oracle

pytestmark = pytest.mark.postgres


def move(member: str = "1001", slot: str | None = "TEAM_2_TOP") -> dict:
    return {"type": "MOVE_PARTICIPANT", "participantId": member, "slot": slot}


async def mutate(client, path, headers, revision, operation=None, key=None):
    return await client.request(
        "PATCH" if operation is not None else "POST",
        path + "/entry-draft" + ("" if operation is not None else "/reset"),
        json={"expectedRevision": revision, **({"operation": operation} if operation else {})},
        headers={**headers, "idempotency-key": key or f"entry-{revision}"},
    )


async def reference(path, action, revision=0, key=None, **extra):
    return await asyncio.to_thread(
        rust_auth,
        {
            "operation": "recruitment_lifecycle",
            "guild_id": GUILD_A,
            "user_id": USER_ID,
            "recruitment_id": path.split("/")[-1],
            "action": action,
            "key": key or f"entry-{revision}",
            "expected_revision": revision,
            **extra,
        },
    )


async def test_move_swap_unassign_displace_and_reset_match_rust(owner, runtime_client):
    path, headers = await fixture(owner, runtime_client)
    assert (await call(runtime_client, path, headers, "close")).status_code == 200
    # Occupied to occupied swaps; null unassigns; unassigned to occupied displaces.
    for revision, operation in enumerate((move(), move(slot=None), move(slot="TEAM_1_TOP"))):
        response = await mutate(runtime_client, path, headers, revision, operation)
        assert response.status_code == 200, response.text
        snapshot = response.json()["data"]
        assert snapshot == await reference(path, "apply", revision, entry_operation=operation)
        assert snapshot == await oracle(path, "entry")
        assert snapshot["draft"]["revision"] == revision + 1
        assert snapshot["recruitment"]["revision"] == 1
        assignments = {
            a["participantId"]: (a["team"], a["lane"]) for a in snapshot["draft"]["assignments"]
        }
        if revision == 0:
            assert assignments["1001"] == ("TEAM_2", "TOP")
            assert assignments["1006"] == ("TEAM_1", "TOP")
        elif revision == 1:
            assert "1001" not in assignments
        else:
            assert assignments["1001"] == ("TEAM_1", "TOP") and "1006" not in assignments
    # Execute Rust first, then replay its exact request in Python.
    operation = move("1002", "TEAM_1_TOP")
    expected = await reference(path, "apply", 3, entry_operation=operation)
    assert (await mutate(runtime_client, path, headers, 3, operation)).json()["data"] == expected
    expected = await reference(path, "reset", 4)
    assert (await mutate(runtime_client, path, headers, 4)).json()["data"] == expected
    assert expected["draft"]["assignments"] == []
    # Resetting an empty draft still bumps the revision, as Rust does.
    response = await mutate(runtime_client, path, headers, 5)
    assert response.status_code == 200
    assert response.json()["data"] == await reference(path, "reset", 5)
    async with owner.connect() as connection:
        assert await connection.scalar(text("SELECT count(*) FROM bot_notification_outbox")) == 2
        assert await connection.scalar(text("SELECT count(*) FROM outbox_events")) == 8
        assert await connection.scalar(text("SELECT count(*) FROM admin_audit_log")) == 8


@pytest.mark.parametrize(
    "operation,code,message",
    [
        (move(slot="TEAM_3_TOP"), "invalid_entry_slot", "엔트리 배치 위치가 올바르지 않습니다."),
        (move("9999"), "not_participant", "현재 모집 참가자가 아닙니다."),
    ],
)
async def test_invalid_move_rolls_back(owner, runtime_client, operation, code, message):
    path, headers = await fixture(owner, runtime_client)
    await call(runtime_client, path, headers, "close")
    response = await mutate(runtime_client, path, headers, 0, operation)
    assert_failure(response, 422, code, message)
    snapshot = await oracle(path, "entry")
    assert snapshot["draft"]["revision"] == 0
    assert len(snapshot["draft"]["assignments"]) == 10


async def test_entry_concurrency_revision_idempotency_and_permission(owner, runtime_client):
    path, headers = await fixture(owner, runtime_client)
    await call(runtime_client, path, headers, "close")
    responses = await asyncio.gather(
        *[mutate(runtime_client, path, headers, 0, move(), f"race-{i}") for i in range(2)]
    )
    assert sorted(r.status_code for r in responses) == [200, 409]
    failed = next(r for r in responses if r.status_code == 409)
    assert_failure(
        failed,
        409,
        "entry_draft_revision_conflict",
        "다른 운영자가 엔트리 편성을 변경했습니다. 최신 상태를 확인해 주세요.",
    )
    winner = next(i for i, r in enumerate(responses) if r.status_code == 200)
    response = await mutate(runtime_client, path, headers, 0, move(slot=None), f"race-{winner}")
    assert_failure(
        response, 409, "idempotency_conflict", "같은 Idempotency-Key가 다른 요청에 사용되었습니다."
    )
    async with owner.begin() as connection:
        await connection.execute(text("DELETE FROM guild_member_discord_roles"))
    response = await mutate(runtime_client, path, headers, 0, move(), f"race-{winner}")
    assert_failure(response, 403, "operator_required", "길드 운영자 권한이 필요합니다.")


@pytest.mark.parametrize(
    "body",
    [
        {"expectedRevision": 0, "operation": {"type": "OTHER"}},
        {"expectedRevision": 0, "operation": {**move(), "extra": True}},
        {"expectedRevision": True, "operation": move()},
        {"expectedRevision": 0, "operation": move("bad-id")},
    ],
)
async def test_entry_typed_json_rejects_invalid_body(owner, runtime_client, body):
    path, headers = await fixture(owner, runtime_client, full=False)
    response = await runtime_client.patch(path + "/entry-draft", json=body, headers=headers)
    assert response.status_code == 422
    assert response.json()["error"]["code"] == "unprocessable_entity"


async def test_entry_effect_failure_rolls_back_assignment_and_revision(owner, runtime_client):
    path, headers = await fixture(owner, runtime_client)
    await call(runtime_client, path, headers, "close")
    before = await oracle(path, "entry")
    async with owner.begin() as connection:
        await connection.execute(text("REVOKE INSERT ON outbox_events FROM fastapi_test_runtime"))
    try:
        response = await mutate(runtime_client, path, headers, 0, move())
        assert_failure(
            response, 500, "internal_error", "요청을 처리하는 중 내부 오류가 발생했습니다."
        )
        assert await oracle(path, "entry") == before
    finally:
        async with owner.begin() as connection:
            await connection.execute(text("GRANT INSERT ON outbox_events TO fastapi_test_runtime"))


async def test_preferred_lanes_bot_only_and_bidirectional_replay(owner, runtime_client):
    path, _ = await fixture(owner, runtime_client)
    url = path + "/members/1001/preferred-lanes"
    actor = {"id": "1001", "globalDisplayName": "Player", "displayName": "Player", "roleIds": []}
    body = {"actor": actor, "preferredLanes": ["MID", "TOP"], "ignored": True}
    response = await runtime_client.put(url, json=body)
    assert response.status_code == 401  # A valid browser session does not authorize this route.
    headers = {"authorization": f"Bearer {BOT}", "idempotency-key": "1000001"}
    response = await runtime_client.put(url, json=body, headers=headers)
    assert response.status_code == 200, response.text
    assert response.json()["data"] == await reference(
        path, "lanes", key="1000001", user_id="1001", bot_actor=actor, lanes=["MID", "TOP"]
    )
    expected = await reference(
        path, "lanes", key="1000002", user_id="1001", bot_actor=actor, lanes=["BOTTOM", "SUPPORT"]
    )
    body["preferredLanes"] = ["BOTTOM", "SUPPORT"]
    response = await runtime_client.put(
        url, json=body, headers={**headers, "idempotency-key": "1000002"}
    )
    assert response.json()["data"] == expected
    player = next(p for p in expected["participants"] if p["id"] == "1001")
    assert player["preferredLanes"] == ["BOTTOM", "SUPPORT"]
    async with owner.connect() as connection:
        assert await connection.scalar(text("SELECT revision FROM recruitment_entry_drafts")) == 2
        assert await connection.scalar(text("SELECT count(*) FROM bot_notification_outbox")) == 1
        assert await connection.scalar(text("SELECT count(*) FROM outbox_events")) == 3


@pytest.mark.parametrize(
    "lanes,status,code",
    [
        ([], 422, "invalid_preferred_lanes"),
        (["TOP", "TOP"], 422, "invalid_preferred_lanes"),
        (["FLEX"], 422, "unprocessable_entity"),
    ],
)
async def test_preferred_lane_validation(owner, runtime_client, lanes, status, code):
    path, _ = await fixture(owner, runtime_client)
    response = await runtime_client.put(
        path + "/members/1001/preferred-lanes",
        json={
            "actor": {"id": "1001", "globalDisplayName": "P", "displayName": "P", "roleIds": []},
            "preferredLanes": lanes,
        },
        headers={"authorization": f"Bearer {BOT}", "idempotency-key": "1000001"},
    )
    assert response.status_code == status
    assert response.json()["error"]["code"] == code


async def test_preferred_lanes_self_operator_and_state_rules(owner, runtime_client):
    path, headers = await fixture(owner, runtime_client)
    actor = {"id": USER_ID, "globalDisplayName": "P", "displayName": "P", "roleIds": []}
    bot_headers = {"authorization": f"Bearer {BOT}", "idempotency-key": "1000001"}

    async def put(member="1001"):
        return await runtime_client.put(
            path + f"/members/{member}/preferred-lanes",
            json={"actor": actor, "preferredLanes": ["TOP"]},
            headers=bot_headers,
        )

    assert_failure(await put(), 403, "operator_required", "길드 운영자 권한이 필요합니다.")
    actor["roleIds"] = [ROLE_A]
    assert_failure(await put("9999"), 422, "not_participant", "현재 모집 참가자가 아닙니다.")
    assert (await put()).status_code == 200
    assert (await call(runtime_client, path, headers, "close")).status_code == 200
    bot_headers["idempotency-key"] = "1000002"
    assert_failure(
        await put(),
        409,
        "invalid_recruitment_state",
        "현재 모집 상태에서는 이 작업을 수행할 수 없습니다.",
    )
