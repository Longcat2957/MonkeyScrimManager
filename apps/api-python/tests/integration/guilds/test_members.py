import asyncio
import hashlib
import json
import uuid

import pytest
from sqlalchemy import text

from tests.support.assertions import assert_failure
from tests.support.guilds.context import GUILD_A, GUILD_B, USER_ID
from tests.support.guilds.members import (
    DEFAULT_MMR,
    MEMBER_ID,
    MEMBERS,
    MMR,
    MMR_ROUTE,
    STATUS,
    adjust,
    adjustment,
    effects,
    member,
    setup,
    snapshot,
)

pytestmark = pytest.mark.postgres


async def test_members_projection_privacy_dummy_sorting_and_read_without_state(
    owner, runtime_client
):
    headers = await setup(owner, runtime_client)
    dummy = f"dmy_{uuid.uuid7()}"
    inactive = "100000000000000003"
    async with owner.begin() as c:
        for user, kind, name, status, lane in (
            (dummy, "DUMMY", "alpha", "ACTIVE", "MID"),
            (inactive, "DISCORD", "A", "INACTIVE", None),
        ):
            await c.execute(
                text("""
                    INSERT INTO users
                        (user_id, identity_kind, display_name, created_at, updated_at)
                    VALUES (:user, :kind, 'Global name', now(), now())
                """),
                {"user": user, "kind": kind},
            )
            await c.execute(
                text("""
                    INSERT INTO guild_members
                        (guild_id, user_id, display_name, status, main_lane, first_seen_at)
                    VALUES (:guild, :user, :name, :status, :lane, now())
                """),
                {"guild": GUILD_A, "user": user, "name": name, "status": status, "lane": lane},
            )
        await c.execute(
            text("""
                INSERT INTO riot_accounts
                    (puuid, user_id, platform, routing_region, game_name, tag_line,
                     normalized_game_name, normalized_tag_line, is_main, created_at, updated_at)
                VALUES ('private-member-puuid', :member, 'KR', 'ASIA', 'SecretMain', 'KR1',
                        'secretmain', 'kr1', true, now(), now())
            """),
            {"member": MEMBER_ID},
        )
        await c.execute(
            text("""
                INSERT INTO user_privacy_settings
                    (user_id, public_profile_enabled, show_riot_ids,
                     allow_external_record_search, updated_at)
                VALUES (:member, false, false, false, now())
            """),
            {"member": MEMBER_ID},
        )
        await c.execute(
            text("""
                UPDATE guild_config SET updated_by=:actor, updated_at='2026-04-05T06:07:08Z'
                WHERE guild_id=:guild
            """),
            {"guild": GUILD_A, "actor": USER_ID},
        )
    before = await effects(owner)
    data = await snapshot(runtime_client, headers)
    assert data["revision"] == 0
    assert data["updatedAt"] == "2026-04-05T06:07:08Z"
    assert data["updatedBy"] == USER_ID
    assert [row["id"] for row in data["members"]] == [MEMBER_ID, dummy, USER_ID, inactive]
    assert member(data) == {
        "id": MEMBER_ID,
        "identityKind": "DISCORD",
        "discordName": "Alpha",
        "riotId": "SecretMain#KR1",
        "role": "MEMBER",
        "mainLane": None,
        "laneMmr": DEFAULT_MMR,
        "registered": True,
        "status": "ACTIVE",
        "lastActiveAt": "2026-02-03T04:05:06Z",
    }
    assert member(data, USER_ID)["role"] == "OPERATOR"
    dummy_member = member(data, dummy)
    assert dummy_member["identityKind"] == "DUMMY" and dummy_member["role"] == "MEMBER"
    assert dummy_member["mainLane"] == "MID" and not dummy_member["registered"]
    assert dummy_member["lastActiveAt"] is None and dummy_member["riotId"] is None
    assert all(row["laneMmr"] == DEFAULT_MMR for row in data["members"])
    head = await runtime_client.head(MEMBERS, headers=headers)
    assert head.status_code == 200 and head.content == b""
    assert await effects(owner) == before
    assert before["state"] == before["lanes"] == 0


async def test_members_without_active_season_read_defaults_and_reject_adjustment(
    owner, runtime_client
):
    headers = await setup(owner, runtime_client)
    async with owner.begin() as c:
        await c.execute(text("UPDATE seasons SET status='CLOSED', ended_at=now()"))
    before = await snapshot(runtime_client, headers)
    assert all(row["laneMmr"] == DEFAULT_MMR for row in before["members"])
    counts = await effects(owner)
    assert_failure(
        await adjust(runtime_client, headers),
        409,
        "guild_setup_required",
        "길드의 활성 시즌을 먼저 설정해야 합니다.",
    )
    assert await snapshot(runtime_client, headers) == before
    assert await effects(owner) == counts


async def test_member_mmr_ledger_audit_replay_conflict_and_revoked_operator(owner, runtime_client):
    headers = await setup(owner, runtime_client)
    response = await adjust(runtime_client, headers)
    assert response.status_code == 200, response.text
    first = response.json()["data"]
    assert first["revision"] == 1 and first["updatedBy"] == USER_ID
    assert member(first)["laneMmr"] == {**DEFAULT_MMR, "MID": 1525.5}
    request_id = response.headers["x-request-id"]
    async with owner.connect() as c:
        ledger = (
            (
                await c.execute(
                    text("""
                    SELECT user_id, lane, lane_revision, source_type, mmr_before, mmr_after, delta,
                           games_played_delta, wins_delta, actor_type, actor_id, note, request_id
                    FROM mmr_ledger_entries
                """)
                )
            )
            .mappings()
            .one()
        )
        assert dict(ledger) == {
            "user_id": MEMBER_ID,
            "lane": "MID",
            "lane_revision": 1,
            "source_type": "MANUAL_ADJUSTMENT",
            "mmr_before": 1500.0,
            "mmr_after": 1525.5,
            "delta": 25.5,
            "games_played_delta": 0,
            "wins_delta": 0,
            "actor_type": "USER",
            "actor_id": USER_ID,
            "note": "수동 조정",
            "request_id": request_id,
        }
        audit = (
            (
                await c.execute(
                    text("""
                    SELECT action, target_type, target_id, before_summary, after_summary,
                           note, request_id FROM admin_audit_log
                """)
                )
            )
            .mappings()
            .one()
        )
        assert dict(audit) == {
            "action": "MEMBER_MMR_ADJUSTED",
            "target_type": "GUILD_MEMBER_MMR",
            "target_id": MEMBER_ID,
            "before_summary": {"lane": "MID", "mmr": 1500.0, "laneRevision": 0},
            "after_summary": {"lane": "MID", "mmr": 1525.5, "delta": 25.5, "laneRevision": 1},
            "note": "수동 조정",
            "request_id": request_id,
        }
    second = await adjust(runtime_client, headers, adjustment(1, lane="TOP", delta=-10), key="top")
    assert second.status_code == 200, second.text
    assert second.json()["data"]["revision"] == 2
    replay = await adjust(runtime_client, headers)
    assert replay.status_code == 200 and replay.json()["data"] == first
    for body in (adjustment(delta=10), adjustment(note="수동 조정")):
        assert_failure(
            await adjust(runtime_client, headers, body),
            409,
            "idempotency_conflict",
            "같은 Idempotency-Key가 다른 요청에 사용되었습니다.",
        )
    assert await effects(owner) == {
        "state": 1,
        "lanes": 2,
        "ledger": 2,
        "audit": 2,
        "idempotency": 2,
        "outbox": 0,
    }
    async with owner.begin() as c:
        await c.execute(text("DELETE FROM guild_member_discord_roles"))
    assert_failure(
        await adjust(runtime_client, headers),
        403,
        "operator_required",
        "길드 운영자 권한이 필요합니다.",
    )


@pytest.mark.parametrize(
    "delta,canonical",
    [
        (10, "10.0"),
        (0.00001, "0.00001"),
        (1e-6, "1e-6"),
        (30.985340601942358, "30.98534060194236"),
    ],
)
async def test_member_mmr_hash_preserves_rust_float_and_original_note(
    owner, runtime_client, delta, canonical
):
    headers = await setup(owner, runtime_client)
    response = await adjust(runtime_client, headers, adjustment(delta=delta, note="  이유  "))
    assert response.status_code == 200, response.text
    canonical_body = (
        f'["{GUILD_A}","{MEMBER_ID}",{{"lane":"MID","delta":{canonical},'
        '"note":"  이유  ","expectedRevision":0}]'
    ).encode()
    async with owner.connect() as c:
        actual = await c.scalar(
            text("SELECT request_hash FROM idempotency_records WHERE route_template=:route"),
            {"route": MMR_ROUTE},
        )
    assert actual == hashlib.sha256(canonical_body).digest()


async def test_member_mmr_replays_seeded_rust_float_hash_without_ledger(owner, runtime_client):
    headers = await setup(owner, runtime_client)
    stored_snapshot = await snapshot(runtime_client, headers)
    # Frozen serde_json's default parser rounds this token before canonical serialization.
    rust_canonical_body = (
        f'["{GUILD_A}","{MEMBER_ID}",{{"lane":"MID","delta":30.98534060194236,'
        '"note":"  이유  ","expectedRevision":0}]'
    ).encode()
    async with owner.begin() as c:
        await c.execute(
            text("""
                INSERT INTO idempotency_records
                    (id, guild_id, scope_key, principal_type, principal_id, method,
                     route_template, idempotency_key, request_hash, state, status_code,
                     response_body, created_at, completed_at, expires_at)
                VALUES (:id, :guild, :guild, 'USER', :actor, 'PATCH', :route, 'rust-float',
                        :hash, 'COMPLETED', 200, CAST(:snapshot AS jsonb), now(), now(),
                        now() + interval '1 hour')
            """),
            {
                "id": uuid.uuid7(),
                "guild": GUILD_A,
                "actor": USER_ID,
                "route": MMR_ROUTE,
                "hash": hashlib.sha256(rust_canonical_body).digest(),
                "snapshot": json.dumps(stored_snapshot),
            },
        )
    before = await effects(owner)
    response = await runtime_client.patch(
        MMR,
        content=(
            '{"lane":"MID","delta":30.985340601942358,"note":"  이유  ","expectedRevision":0}'
        ).encode(),
        headers={**headers, "content-type": "application/json", "idempotency-key": "rust-float"},
    )
    assert response.status_code == 200, response.text
    assert response.json()["data"] == stored_snapshot
    assert await effects(owner) == before
    assert before["ledger"] == before["audit"] == before["state"] == before["lanes"] == 0


async def test_member_mmr_preserves_game_counters_and_rolls_back_float_overflow(
    owner, runtime_client
):
    headers = await setup(owner, runtime_client)
    async with owner.begin() as c:
        await c.execute(
            text("""
                INSERT INTO user_lane_mmr
                    (guild_id, season_id, user_id, lane, mmr, games_played, wins,
                     revision, updated_at)
                SELECT guild_id, id, :member, 'MID', 1600, 4, 2, 0, now()
                FROM seasons WHERE guild_id=:guild AND status='ACTIVE'
            """),
            {"guild": GUILD_A, "member": MEMBER_ID},
        )
    response = await adjust(runtime_client, headers)
    assert response.status_code == 200, response.text
    assert member(response.json()["data"])["laneMmr"]["MID"] == 1625.5
    async with owner.begin() as c:
        result = await c.execute(
            text("SELECT mmr, games_played, wins, revision FROM user_lane_mmr")
        )
        assert result.one() == (1625.5, 4, 2, 1)
        await c.execute(text("UPDATE user_lane_mmr SET mmr=1e308"))
    before = await snapshot(runtime_client, headers)
    counts = await effects(owner)
    assert_failure(
        await adjust(runtime_client, headers, adjustment(1, delta=1e308), key="overflow"),
        422,
        "invalid_mmr_adjustment",
        "MMR 증감값과 조정 사유를 확인해 주세요.",
    )
    assert await snapshot(runtime_client, headers) == before
    assert await effects(owner) == counts


async def test_member_status_self_denial_noop_inactive_mmr_and_restore(owner, runtime_client):
    headers = await setup(owner, runtime_client)
    before = await snapshot(runtime_client, headers)
    assert_failure(
        await runtime_client.patch(
            f"{MEMBERS}/{USER_ID}/status",
            json={"status": "INACTIVE", "expectedRevision": 0},
            headers=headers,
        ),
        409,
        "self_deactivation_forbidden",
        "자기 자신은 비활성화할 수 없습니다.",
    )
    assert (await effects(owner))["state"] == 0
    for revision, status in ((0, "ACTIVE"), (1, "INACTIVE")):
        response = await runtime_client.patch(
            STATUS, json={"status": status, "expectedRevision": revision}, headers=headers
        )
        assert response.status_code == 200, response.text
        assert response.json()["data"]["revision"] == revision + 1
        assert member(response.json()["data"])["status"] == status
    async with owner.connect() as c:
        assert await c.scalar(
            text("SELECT inactivated_at IS NOT NULL FROM guild_members WHERE user_id=:member"),
            {"member": MEMBER_ID},
        )
    response = await adjust(runtime_client, headers, adjustment(2))
    assert response.status_code == 200, response.text
    assert member(response.json()["data"])["status"] == "INACTIVE"
    response = await runtime_client.patch(
        STATUS, json={"status": "ACTIVE", "expectedRevision": 3}, headers=headers
    )
    assert response.status_code == 200, response.text
    latest = response.json()["data"]
    assert latest["revision"] == 4
    assert member(latest) == {**member(before), "laneMmr": {**DEFAULT_MMR, "MID": 1525.5}}
    async with owner.connect() as c:
        assert (
            await c.scalar(
                text("SELECT inactivated_at FROM guild_members WHERE user_id=:member"),
                {"member": MEMBER_ID},
            )
            is None
        )
        logs = (
            await c.execute(
                text("""
                    SELECT before_summary, after_summary, target_type FROM admin_audit_log
                    WHERE action='MEMBER_STATUS_CHANGED' ORDER BY created_at, id
                """)
            )
        ).all()
        assert logs == [
            ({"status": "ACTIVE"}, {"status": "ACTIVE"}, "GUILD_MEMBER"),
            ({"status": "ACTIVE"}, {"status": "INACTIVE"}, "GUILD_MEMBER"),
            ({"status": "INACTIVE"}, {"status": "ACTIVE"}, "GUILD_MEMBER"),
        ]


async def test_members_concurrent_commands_share_one_revision_lock(owner, runtime_client):
    headers = await setup(owner, runtime_client)
    responses = await asyncio.gather(
        adjust(runtime_client, headers),
        runtime_client.patch(
            STATUS, json={"status": "INACTIVE", "expectedRevision": 0}, headers=headers
        ),
    )
    assert sorted(response.status_code for response in responses) == [200, 409]
    conflict = next(response for response in responses if response.status_code == 409)
    assert_failure(conflict, 409, "revision_conflict", "최신 상태를 다시 확인해야 합니다.")
    latest = await snapshot(runtime_client, headers)
    assert conflict.json()["error"]["details"] == {
        "expectedRevision": 0,
        "currentRevision": 1,
        "updatedAt": latest["updatedAt"],
        "updatedBy": USER_ID,
    }
    counts = await effects(owner)
    assert counts["audit"] == counts["state"] == 1
    assert counts["ledger"] == counts["lanes"] == counts["idempotency"] <= 1


@pytest.mark.parametrize("action", ["mmr", "status"])
async def test_member_audit_failure_rolls_back_every_mutation(owner, runtime_client, action):
    headers = await setup(owner, runtime_client)
    before = await snapshot(runtime_client, headers)
    counts = await effects(owner)
    body = adjustment() if action == "mmr" else {"status": "INACTIVE", "expectedRevision": 0}
    path = MMR if action == "mmr" else STATUS
    async with owner.begin() as c:
        await c.execute(text("REVOKE INSERT ON admin_audit_log FROM fastapi_test_runtime"))
    try:
        response = await runtime_client.patch(
            path, json=body, headers={**headers, "idempotency-key": "rollback"}
        )
        assert_failure(
            response, 500, "internal_error", "요청을 처리하는 중 내부 오류가 발생했습니다."
        )
    finally:
        async with owner.begin() as c:
            await c.execute(text("GRANT INSERT ON admin_audit_log TO fastapi_test_runtime"))
    assert await snapshot(runtime_client, headers) == before
    assert await effects(owner) == counts
    retry = await runtime_client.patch(
        path, json=body, headers={**headers, "idempotency-key": "rollback"}
    )
    assert retry.status_code == 200, retry.text


@pytest.mark.parametrize("corruption", ["snapshot", "member", "status_code"])
async def test_member_replay_validates_snapshot_independent_of_http_status(
    owner, runtime_client, corruption
):
    headers = await setup(owner, runtime_client)
    response = await adjust(runtime_client, headers)
    assert response.status_code == 200, response.text
    data = response.json()["data"]
    counts = await effects(owner)
    async with owner.begin() as c:
        if corruption == "status_code":
            await c.execute(text("UPDATE idempotency_records SET status_code=201"))
        else:
            malformed = {} if corruption == "snapshot" else data
            if corruption == "member":
                malformed["members"][0]["status"] = "UNKNOWN"
            await c.execute(
                text("UPDATE idempotency_records SET response_body=CAST(:body AS jsonb)"),
                {"body": json.dumps(malformed)},
            )
    replay = await adjust(runtime_client, headers)
    if corruption == "status_code":
        # The Rust member repository replays its typed snapshot, ignoring stored HTTP status.
        assert replay.status_code == 200 and replay.json()["data"] == data
    else:
        assert_failure(
            replay,
            500,
            "internal_error",
            "요청을 처리하는 중 내부 오류가 발생했습니다.",
        )
    assert await effects(owner) == counts


@pytest.mark.parametrize("denial", ["operator", "inactive_actor", "inactive_guild", "tenant"])
async def test_members_recheck_operator_and_hide_inaccessible_guilds(owner, runtime_client, denial):
    headers = await setup(owner, runtime_client)
    path = MEMBERS
    async with owner.begin() as c:
        if denial == "operator":
            await c.execute(text("DELETE FROM guild_member_discord_roles"))
        elif denial == "inactive_actor":
            await c.execute(
                text("UPDATE guild_members SET status='INACTIVE' WHERE user_id=:actor"),
                {"actor": USER_ID},
            )
        elif denial == "inactive_guild":
            await c.execute(text("UPDATE guilds SET status='DISABLED'"))
        else:
            path = MEMBERS.replace(GUILD_A, GUILD_B)
    for method, endpoint, body in (
        ("GET", path, None),
        ("PATCH", f"{path}/{MEMBER_ID}/mmr", adjustment()),
        ("PATCH", f"{path}/{MEMBER_ID}/status", {"status": "INACTIVE", "expectedRevision": 0}),
    ):
        response = await runtime_client.request(
            method, endpoint, json=body, headers={**headers, "idempotency-key": "denied"}
        )
        if denial == "operator":
            assert_failure(response, 403, "operator_required", "길드 운영자 권한이 필요합니다.")
        else:
            assert_failure(response, 404, "not_found", "요청한 길드 멤버를 찾을 수 없습니다.")
    assert (await effects(owner))["state"] == 0


async def test_member_session_csrf_key_and_missing_target_errors(owner, runtime_client):
    headers = await setup(owner, runtime_client)
    for method, path, body in (
        ("GET", MEMBERS, None),
        ("PATCH", MMR, adjustment()),
        ("PATCH", STATUS, {"status": "INACTIVE", "expectedRevision": 0}),
    ):
        assert_failure(
            await runtime_client.request(method, path, json=body, headers={"cookie": ""}),
            401,
            "unauthorized",
            "인증이 필요합니다.",
        )
        if method == "PATCH":
            assert_failure(
                await runtime_client.patch(path, json=body),
                403,
                "csrf_validation_failed",
                "요청 출처를 확인할 수 없습니다.",
            )
            assert_failure(
                await runtime_client.patch(
                    path.replace(MEMBER_ID, "9999"),
                    json=body,
                    headers={**headers, "idempotency-key": "missing-target"},
                ),
                404,
                "not_found",
                "요청한 길드 멤버를 찾을 수 없습니다.",
            )
    for extra in (
        [],
        [("idempotency-key", "")],
        [("idempotency-key", "has space")],
        [("idempotency-key", "a" * 129)],
        [("idempotency-key", "first"), ("idempotency-key", "second")],
    ):
        response = await runtime_client.patch(
            MMR, json=adjustment(), headers=[*headers.items(), *extra]
        )
        assert_failure(
            response, 400, "invalid_idempotency_key", "Idempotency-Key가 올바르지 않습니다."
        )
        assert response.json()["error"]["details"] == {"maxLength": 128}
    assert (await effects(owner))["state"] == 0


@pytest.mark.parametrize(
    "changes,code,message",
    [
        ({"delta": 0}, "invalid_mmr_adjustment", "MMR 증감값과 조정 사유를 확인해 주세요."),
        ({"note": "  "}, "invalid_mmr_adjustment", "MMR 증감값과 조정 사유를 확인해 주세요."),
        ({"note": "가" * 501}, "invalid_mmr_adjustment", "MMR 증감값과 조정 사유를 확인해 주세요."),
        (
            {"expectedRevision": -1},
            "invalid_expected_revision",
            "expectedRevision은 0 이상이어야 합니다.",
        ),
    ],
)
async def test_member_adjustment_validation_has_safe_business_reason(
    owner, runtime_client, changes, code, message
):
    headers = await setup(owner, runtime_client)
    assert_failure(await adjust(runtime_client, headers, adjustment(**changes)), 422, code, message)
    assert (await effects(owner))["state"] == 0


@pytest.mark.parametrize(
    "path,body",
    [
        (MMR, adjustment(delta=True)),
        (MMR, adjustment(delta="10")),
        (MMR, adjustment(lane="FLEX")),
        (MMR, adjustment(unexpected=True)),
        (STATUS, {"status": "ACTIVE", "expectedRevision": True}),
        (STATUS, {"status": "UNKNOWN", "expectedRevision": 0}),
    ],
)
async def test_member_commands_reject_invalid_json_fields(owner, runtime_client, path, body):
    headers = await setup(owner, runtime_client)
    assert_failure(
        await runtime_client.patch(
            path, json=body, headers={**headers, "idempotency-key": "invalid"}
        ),
        422,
        "unprocessable_entity",
        "요청 JSON의 필드 또는 값이 API 계약과 일치하지 않습니다.",
    )
    assert (await effects(owner))["state"] == 0


@pytest.mark.parametrize("action", ["mmr", "status"])
@pytest.mark.parametrize(
    "revision,status,code,message",
    [
        (
            "-0",
            422,
            "unprocessable_entity",
            "요청 JSON의 필드 또는 값이 API 계약과 일치하지 않습니다.",
        ),
        ("1e309", 400, "bad_request", "요청 형식이 올바르지 않습니다."),
    ],
)
async def test_member_commands_preserve_legacy_number_rejections(
    owner, runtime_client, action, revision, status, code, message
):
    headers = await setup(owner, runtime_client)
    fields = '"lane":"MID","delta":1,"note":"reason"' if action == "mmr" else '"status":"ACTIVE"'
    raw = "{" + fields + ',"expectedRevision":' + revision + "}"
    before = await effects(owner)
    response = await runtime_client.patch(
        MMR if action == "mmr" else STATUS,
        content=raw,
        headers={**headers, "content-type": "application/json", "idempotency-key": "raw-number"},
    )
    assert_failure(response, status, code, message)
    assert await effects(owner) == before
