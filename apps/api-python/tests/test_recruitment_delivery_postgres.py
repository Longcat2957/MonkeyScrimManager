import asyncio
import uuid

import pytest
from sqlalchemy import text
from test_context_postgres import GUILD_A, ROLE_A, USER_ID
from test_entry_mutation_postgres import reference
from test_foundation import assert_failure
from test_recruitment_create_postgres import BOT
from test_recruitment_lifecycle_postgres import call, fixture

pytestmark = pytest.mark.postgres
ACTOR = {"id": USER_ID, "globalDisplayName": "Creator", "displayName": "Creator", "roleIds": []}
BIND = {"channelId": "789", "messageId": "987"}


async def bind(client, path, actor=None, key="10000001", body=None):
    return await client.put(
        path + "/message",
        json={"actor": actor or ACTOR, **(body or BIND)},
        headers={"authorization": f"Bearer {BOT}", "idempotency-key": key},
    )


async def dm(client, path, headers, key="dm-1"):
    return await client.post(
        path + "/participant-confirmation-dms", json={}, headers={**headers, "idempotency-key": key}
    )


async def series(client, path, headers, key="series-1", revision=1):
    return await client.post(
        f"/api/guilds/{GUILD_A}/series",
        json={"sourceRecruitmentId": path.split("/")[-1], "expectedRevision": revision},
        headers={**headers, "idempotency-key": key},
    )


async def setup_series(owner, client, *, entry_channel=True):
    path, headers = await fixture(owner, client)
    await call(client, path, headers, "close")
    async with owner.begin() as c:
        await c.execute(
            text(
                "INSERT INTO guild_counters (guild_id, counter_key, next_value) "
                "VALUES (:guild, 'SERIES', 1)"
            ),
            {"guild": GUILD_A},
        )
        if entry_channel:
            await c.execute(text("UPDATE guild_config SET entry_channel_id = '999'"))
        await c.execute(
            text("""
            INSERT INTO user_lane_mmr
                (guild_id, season_id, user_id, lane, mmr, games_played, wins, revision, updated_at)
            SELECT guild_id, season_id, '1001', 'TOP', 1732.5, 12, 8, 3, now() FROM recruitments
        """)
        )
    return path, headers


async def test_bind_rust_bidirectional_and_replay_before_membership(owner, runtime_client):
    path, _ = await fixture(owner, runtime_client, full=False)
    response = await bind(runtime_client, path)
    assert response.status_code == 200, response.text
    assert response.json()["data"] == await reference(
        path, "bind", key="10000001", bot_actor=ACTOR, entry_operation=BIND
    )
    expected = await reference(
        path,
        "bind",
        key="10000002",
        bot_actor=ACTOR,
        entry_operation={"channelId": "555", "messageId": "666"},
    )
    response = await bind(
        runtime_client, path, key="10000002", body={"channelId": "555", "messageId": "666"}
    )
    assert response.json()["data"] == expected
    assert expected["recruitment"]["revision"] == 2
    async with owner.begin() as c:
        assert await c.scalar(text("SELECT revision FROM recruitment_entry_drafts")) == 0
        assert await c.scalar(text("SELECT count(*) FROM bot_notification_outbox")) == 1
        assert await c.scalar(text("SELECT count(*) FROM outbox_events")) == 3
        await c.execute(
            text("UPDATE guild_members SET status = 'INACTIVE' WHERE user_id = :id"),
            {"id": USER_ID},
        )
    assert (await bind(runtime_client, path)).status_code == 200


async def test_bind_requires_creator_or_operator_and_bindable_state(owner, runtime_client):
    path, headers = await fixture(owner, runtime_client)
    actor = {**ACTOR, "id": "1001"}
    assert_failure(
        await bind(runtime_client, path, actor),
        403,
        "operator_required",
        "길드 운영자 권한이 필요합니다.",
    )
    await call(runtime_client, path, headers, "cancel")
    assert_failure(
        await bind(runtime_client, path),
        409,
        "invalid_recruitment_state",
        "현재 모집 상태에서는 이 작업을 수행할 수 없습니다.",
    )
    response = await runtime_client.put(path + "/message", json={"actor": ACTOR, **BIND})
    assert response.status_code == 401


async def test_dm_dedup_revision_rust_replay_and_dummy_skip(owner, runtime_client):
    path, headers = await fixture(owner, runtime_client)
    await call(runtime_client, path, headers, "close")
    dummy = "dmy_" + str(uuid.uuid7())
    async with owner.begin() as c:
        await c.execute(
            text(
                "INSERT INTO users (user_id, identity_kind, display_name, created_at, updated_at) "
                "VALUES (:id, 'DUMMY', 'Dummy', now(), now())"
            ),
            {"id": dummy},
        )
        await c.execute(
            text(
                "INSERT INTO guild_members "
                "(guild_id, user_id, display_name, status, first_seen_at) "
                "VALUES (:guild, :id, 'Dummy', 'ACTIVE', now())"
            ),
            {"id": dummy, "guild": GUILD_A},
        )
        await c.execute(
            text(
                "INSERT INTO recruitment_participants "
                "(guild_id, recruitment_id, user_id, status, joined_at) "
                "SELECT guild_id, id, :dummy, 'JOINED', now() FROM recruitments"
            ),
            {"dummy": dummy},
        )
    response = await dm(runtime_client, path, headers)
    assert response.status_code == 200, response.text
    assert response.json()["data"] == {"queuedRecipientCount": 10, "alreadyQueuedRecipientCount": 0}
    assert response.json()["data"] == await reference(path, "dm", key="dm-1")
    expected = await reference(path, "dm", key="dm-2")
    assert expected == {"queuedRecipientCount": 0, "alreadyQueuedRecipientCount": 10}
    assert (await dm(runtime_client, path, headers, "dm-2")).json()["data"] == expected
    # New keys deduplicate recipients; a new recruitment revision permits another dispatch.
    response = await dm(runtime_client, path, headers, "dm-3")
    assert response.json()["data"] == expected
    await bind(runtime_client, path, {**ACTOR, "roleIds": [ROLE_A]})
    response = await dm(runtime_client, path, headers, "dm-4")
    assert response.status_code == 200, response.text
    assert response.json()["data"]["queuedRecipientCount"] == 10
    async with owner.connect() as c:
        payloads = (
            (
                await c.execute(
                    text(
                        "SELECT payload FROM bot_notification_outbox "
                        "WHERE notification_type = 'RECRUITMENT_PARTICIPANT_CONFIRMATION_DM_CREATE'"
                    )
                )
            )
            .scalars()
            .all()
        )
        assert len(payloads) == 20
        assert all(
            p["schemaVersion"] == 1
            and p["guildId"] == GUILD_A
            and p["recruitmentHref"] == f"/g/{GUILD_A}/recruitments/{path.split('/')[-1]}/entry"
            and p["recipientUserId"] != dummy
            for p in payloads
        )
        assert await c.scalar(text("SELECT count(*) FROM outbox_events")) == 3


async def test_dm_denial_messages_and_missing_recruitment(owner, runtime_client):
    path, headers = await fixture(owner, runtime_client, full=False)
    assert_failure(
        await dm(runtime_client, path, headers),
        409,
        "participant_confirmation_dm_unavailable",
        "마감된 모집에서만 참가 확정 DM을 발송할 수 있습니다.",
    )
    missing = path.rsplit("/", 1)[0] + "/" + str(uuid.uuid7())
    assert_failure(
        await dm(runtime_client, missing, headers),
        404,
        "recruitment_not_found",
        "요청한 모집을 찾을 수 없습니다.",
    )
    async with owner.begin() as c:
        await c.execute(text("DELETE FROM guild_member_discord_roles"))
    assert_failure(
        await dm(runtime_client, path, headers),
        403,
        "operator_required",
        "참가 확정 DM 발송에는 현재 길드 운영자 권한이 필요합니다.",
    )


@pytest.mark.parametrize("rust_first,entry_channel", [(False, True), (True, True), (False, False)])
async def test_series_create_matches_rust_and_captures_mmr(
    owner, runtime_client, rust_first, entry_channel
):
    path, headers = await setup_series(owner, runtime_client, entry_channel=entry_channel)
    expected = await reference(path, "series", 1, "series-1") if rust_first else None
    response = await series(runtime_client, path, headers)
    assert response.status_code == 201, response.text
    if expected is None:
        expected = await reference(path, "series", 1, "series-1")
    assert response.json()["data"] == expected
    series_id = expected["series"]["id"]
    assert expected["series"]["href"] == f"/g/{GUILD_A}/series/{series_id}/pick-ban"
    async with owner.connect() as c:
        created = (await c.execute(text("SELECT * FROM series"))).mappings().one()
        assert created["best_of"] == 3 and created["hard_fearless"] is True
        assert created["title"] == "일반 내전 #1" and created["status"] == "IN_PROGRESS"
        assert created["revision"] == 0
        assert await c.scalar(text("SELECT count(*) FROM series_participants")) == 10
        assert await c.scalar(text("SELECT count(*) FROM user_lane_mmr")) == 10
        assert (
            await c.scalar(
                text("SELECT mmr_at_lock FROM series_participants WHERE user_id = '1001'")
            )
            == 1732.5
        )
        assert (
            await c.scalar(text("SELECT revision FROM user_lane_mmr WHERE user_id = '1001'")) == 3
        )
        assert await c.scalar(text("SELECT status FROM recruitments")) == "CONVERTED"
        assert await c.scalar(text("SELECT revision FROM recruitments")) == 2
        assert await c.scalar(text("SELECT revision FROM recruitment_entry_drafts")) == 0
        payload = await c.scalar(
            text(
                "SELECT payload FROM bot_notification_outbox "
                "WHERE notification_type = 'ENTRY_CONFIRMATION_CREATE'"
            )
        )
        if entry_channel:
            assert payload["seriesId"] == series_id and payload["schemaVersion"] == 1
            assert payload["snapshot"]["teams"][0]["players"][0] == {
                "id": "1001",
                "displayName": "참가자",
                "lane": "TOP",
            }
            assert payload["snapshot"]["links"]["pickBan"] == expected["series"]["href"]
        else:
            assert payload is None
        assert await c.scalar(text("SELECT count(*) FROM outbox_events")) == 4
        assert await c.scalar(text("SELECT count(*) FROM admin_audit_log")) == 3
    assert (await runtime_client.get(path)).status_code == 404


async def test_series_race_and_permission_before_replay(owner, runtime_client):
    path, headers = await setup_series(owner, runtime_client)
    responses = await asyncio.gather(
        *[series(runtime_client, path, headers, f"s-{i}") for i in range(2)]
    )
    assert sorted(r.status_code for r in responses) == [201, 409]
    failure = next(r for r in responses if r.status_code == 409)
    assert_failure(
        failure,
        409,
        "recruitment_revision_conflict",
        "모집 상태가 변경되었습니다. 최신 상태를 확인해 주세요.",
    )
    winner = next(i for i, r in enumerate(responses) if r.status_code == 201)
    async with owner.begin() as c:
        assert await c.scalar(text("SELECT count(*) FROM series")) == 1
        assert (
            await c.scalar(
                text("SELECT next_value FROM guild_counters WHERE counter_key = 'SERIES'")
            )
            == 2
        )
        await c.execute(text("DELETE FROM guild_member_discord_roles"))
    assert_failure(
        await series(runtime_client, path, headers, f"s-{winner}"),
        403,
        "operator_required",
        "길드 운영자 권한이 필요합니다.",
    )


@pytest.mark.parametrize("action", ["dm", "series", "bind"])
async def test_late_failure_rolls_back_delivery_and_series(owner, runtime_client, action):
    path, headers = await setup_series(owner, runtime_client)
    async with owner.begin() as c:
        await c.execute(text("REVOKE INSERT ON admin_audit_log FROM fastapi_test_runtime"))
    try:
        response = await (
            dm(runtime_client, path, headers)
            if action == "dm"
            else series(runtime_client, path, headers)
            if action == "series"
            else bind(runtime_client, path)
        )
        assert_failure(
            response, 500, "internal_error", "요청을 처리하는 중 내부 오류가 발생했습니다."
        )
        async with owner.connect() as c:
            assert await c.scalar(text("SELECT status FROM recruitments")) == "CLOSED"
            assert await c.scalar(text("SELECT revision FROM recruitments")) == 1
            assert await c.scalar(text("SELECT count(*) FROM series")) == 0
            assert await c.scalar(text("SELECT count(*) FROM user_lane_mmr")) == 1
            assert (
                await c.scalar(
                    text("SELECT next_value FROM guild_counters WHERE counter_key = 'SERIES'")
                )
                == 1
            )
            assert await c.scalar(text("SELECT count(*) FROM bot_notification_outbox")) == 2
            assert await c.scalar(text("SELECT count(*) FROM idempotency_records")) == 2
    finally:
        async with owner.begin() as c:
            await c.execute(text("GRANT INSERT ON admin_audit_log TO fastapi_test_runtime"))


async def test_series_incomplete_and_wrong_state_preserve_state(owner, runtime_client):
    path, headers = await fixture(owner, runtime_client, full=False)
    assert_failure(
        await series(runtime_client, path, headers, revision=0),
        409,
        "series_source_not_ready",
        "모집 마감 후 엔트리 편성이 완료된 상태에서만 시리즈를 시작할 수 있습니다.",
    )
    async with owner.begin() as c:
        await c.execute(text("UPDATE recruitments SET status = 'CLOSED', closed_at = now()"))
    response = await series(runtime_client, path, headers, revision=0)
    assert_failure(
        response,
        422,
        "entry_draft_incomplete",
        "참가자 10명을 양 팀의 모든 라인에 배치한 뒤 시리즈를 시작할 수 있습니다.",
    )
    assert response.json()["error"]["details"] == {
        "joinedCount": 0,
        "assignedCount": 0,
        "targetCount": 10,
    }


async def test_dm_dedup_payload_conflict_rolls_back_request(owner, runtime_client):
    path, headers = await fixture(owner, runtime_client)
    await call(runtime_client, path, headers, "close")
    assert (await dm(runtime_client, path, headers)).status_code == 200
    async with owner.begin() as c:
        await c.execute(
            text(
                "UPDATE bot_notification_outbox SET payload = "
                "jsonb_set(payload, '{recruitmentTitle}', '\"different title\"'::jsonb) "
                "WHERE notification_type = 'RECRUITMENT_PARTICIPANT_CONFIRMATION_DM_CREATE'"
            )
        )
    response = await dm(runtime_client, path, headers, "dm-conflict")
    assert_failure(response, 500, "internal_error", "요청을 처리하는 중 내부 오류가 발생했습니다.")
    async with owner.connect() as c:
        assert await c.scalar(text("SELECT count(*) FROM bot_notification_outbox")) == 12
        assert await c.scalar(text("SELECT count(*) FROM idempotency_records")) == 3
        assert await c.scalar(text("SELECT count(*) FROM admin_audit_log")) == 3
