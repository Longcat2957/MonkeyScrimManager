import asyncio
import uuid
from datetime import UTC, datetime, timedelta

import pytest
from sqlalchemy import text

from tests.support.assertions import assert_failure
from tests.support.guilds.context import GUILD_A, USER_ID
from tests.support.recruitments.conversion import BODY, convert, setup
from tests.support.recruitments.creation import BOT
from tests.support.recruitments.delivery import series, setup_series
from tests.support.recruitments.entries import reference

pytestmark = pytest.mark.postgres


@pytest.mark.parametrize("assigned,rust_first", [(10, False), (4, False), (0, False), (4, True)])
async def test_conversion_rust_replay_and_preserved_roster(
    owner, runtime_client, assigned, rust_first
):
    path, headers = await setup(owner, runtime_client, assigned)
    expected = None
    if rust_first:
        expected = await reference(path, "convert", 1, "convert-1", entry_operation=BODY)
    response = await convert(runtime_client, path, headers)
    assert response.status_code == 201, response.text
    data = response.json()["data"]
    if expected is None:
        expected = await reference(path, "convert", 1, "convert-1", entry_operation=BODY)
    assert data == expected
    record = data["recruitment"]
    assert record["status"] == "CLOSED" and record["revision"] == 1
    assert record["format"] == "TEN_PERSON" and record["participantCount"] == 10
    assert "2026-09-13 20:00 (KST)" in record["title"]
    assert [r["id"] for r in data["participants"]] == [str(i) for i in range(1001, 1011)]
    auction_path = f"/api/guilds/{GUILD_A}/auction-recruitments/{record['id']}"
    assert (await runtime_client.get(auction_path)).json()["data"] == data
    assert (await runtime_client.get(auction_path + "/setup-draft")).status_code == 200
    async with owner.connect() as c:
        source = (await c.execute(text("SELECT * FROM recruitments"))).mappings().one()
        assert source["status"] == "CONVERTED" and source["revision"] == 2
        auction = (await c.execute(text("SELECT * FROM auction_recruitments"))).mappings().one()
        assert auction["season_id"] == source["season_id"]
        assert auction["final_best_of"] == source["best_of"]
        assert auction["hard_fearless"] == source["hard_fearless"]
        assert auction["semifinal_best_of"] is None
        assert await c.scalar(text("SELECT revision FROM recruitment_entry_drafts")) == 0
        assert (
            await c.scalar(text("SELECT count(*) FROM recruitment_entry_assignments")) == assigned
        )
        roster = (
            (
                await c.execute(
                    text("SELECT * FROM auction_recruitment_participants ORDER BY joined_at")
                )
            )
            .mappings()
            .all()
        )
        for index, row in enumerate(roster):
            assert row["joined_at"] == roster[0]["joined_at"] + timedelta(milliseconds=index)
            assert row["snapshot_origin"] == ("CAPTURED" if index < assigned else "UNAVAILABLE")
            assert row["mmr_at_join"] == (
                (1732.5 if index == 0 else 1500) if index < assigned else None
            )
        assert await c.scalar(text("SELECT count(*) FROM user_lane_mmr")) == max(1, assigned)
        assert await c.scalar(text("SELECT revision FROM user_lane_mmr WHERE user_id='1001'")) == 3
        assert await c.scalar(text("SELECT count(*) FROM mmr_ledger_entries")) == 0
        teams = (
            await c.execute(
                text(
                    "SELECT team_name,captain_user_id,initial_points "
                    "FROM auction_setup_teams ORDER BY team_index"
                )
            )
        ).all()
        assert teams == [("팀 1", None, 1000), ("팀 2", None, 1000)]
        event = (
            await c.execute(
                text(
                    "SELECT topic,aggregate_revision FROM outbox_events "
                    "WHERE event_type='AUCTION_RECRUITMENT_CREATED'"
                )
            )
        ).one()
        assert event == (f"guild:{GUILD_A}:auction-recruitment:{record['id']}", 1)
        payload = await c.scalar(
            text(
                "SELECT payload FROM bot_notification_outbox "
                "WHERE notification_type='AUCTION_RECRUITMENT_MESSAGE_CREATE'"
            )
        )
        assert payload == {
            "schemaVersion": 2,
            "auctionRecruitmentId": record["id"],
            "snapshot": data,
        }
        assert await c.scalar(
            text(
                "SELECT before_summary FROM admin_audit_log "
                "WHERE action='AUCTION_RECRUITMENT_CREATED_FROM_ENTRY_DRAFT'"
            )
        ) == {
            "sourceRecruitmentId": path.split("/")[-1],
            "sourceRecruitmentRevision": 1,
            "participantCount": 10,
            "declaredLaneCount": assigned,
        }
    assert (await convert(runtime_client, path, headers)).json()["data"] == data
    assert_failure(
        await convert(runtime_client, path, headers, {**BODY, "expectedRevision": 2}),
        409,
        "idempotency_conflict",
        "같은 Idempotency-Key가 다른 요청에 사용되었습니다.",
    )
    assert_failure(
        await convert(runtime_client, path, headers, {**BODY, "expectedRevision": 2}, "new"),
        409,
        "entry_draft_auction_source_not_ready",
        "마감된 일반 엔트리 모집만 경매 모집으로 전환할 수 있습니다.",
    )


@pytest.mark.parametrize(
    "change,status,code,message",
    [
        (
            "UPDATE recruitments SET revision=2",
            409,
            "recruitment_revision_conflict",
            "모집 상태가 변경되었습니다. 최신 상태를 확인해 주세요.",
        ),
        (
            "UPDATE recruitments SET status='OPEN'",
            409,
            "entry_draft_auction_source_not_ready",
            "마감된 일반 엔트리 모집만 경매 모집으로 전환할 수 있습니다.",
        ),
        (
            "UPDATE recruitment_participants SET status='WITHDRAWN',withdrawn_at=now() "
            "WHERE user_id='1010'",
            422,
            "entry_draft_incomplete",
            "참가자 10명을 채운 뒤 경매 모집으로 전환할 수 있습니다.",
        ),
        (
            "UPDATE guild_config SET recruitment_channel_id=NULL",
            409,
            "recruitment_channel_not_configured",
            "Discord 모집 채널을 먼저 설정해야 경매 모집으로 전환할 수 있습니다.",
        ),
    ],
)
async def test_conversion_business_failures(owner, runtime_client, change, status, code, message):
    path, headers = await setup(owner, runtime_client)
    async with owner.begin() as c:
        await c.execute(text(change))
    response = await convert(runtime_client, path, headers)
    assert_failure(response, status, code, message)
    if code == "entry_draft_incomplete":
        assert response.json()["error"]["details"] == {
            "joinedCount": 9,
            "assignedCount": 9,
            "targetCount": 10,
        }
    async with owner.connect() as c:
        assert await c.scalar(text("SELECT count(*) FROM auction_recruitments")) == 0


async def test_active_series_guard(owner, runtime_client):
    path, headers = await setup_series(owner, runtime_client)
    assert (await series(runtime_client, path, headers)).status_code == 201
    async with owner.begin() as c:
        await c.execute(
            text("UPDATE recruitments SET status='CLOSED',revision=1,converted_at=NULL")
        )
    assert_failure(
        await convert(runtime_client, path, headers),
        409,
        "entry_draft_already_converted_to_series",
        "이미 일반 시리즈로 전환된 모집은 경매 모집으로 전환할 수 없습니다.",
    )


@pytest.mark.parametrize("inactive", [False, True])
async def test_replay_requires_current_operator_membership(owner, runtime_client, inactive):
    path, headers = await setup(owner, runtime_client)
    assert (await convert(runtime_client, path, headers)).status_code == 201
    async with owner.begin() as c:
        await c.execute(
            text(
                "UPDATE guild_members SET status='INACTIVE' WHERE user_id=:user"
                if inactive
                else "DELETE FROM guild_member_discord_roles WHERE user_id=:user"
            ),
            {"user": USER_ID},
        )
    assert_failure(
        await convert(runtime_client, path, headers),
        403,
        "guild_membership_required" if inactive else "operator_required",
        "해당 길드의 활성 멤버만 엔트리를 경매 모집으로 전환할 수 있습니다."
        if inactive
        else "엔트리를 경매 모집으로 전환하려면 현재 길드 운영자 권한이 필요합니다.",
    )


async def test_concurrent_conversion_creates_once(owner, runtime_client):
    path, headers = await setup(owner, runtime_client)
    responses = await asyncio.gather(
        *[convert(runtime_client, path, headers, key=f"race-{i}") for i in range(2)]
    )
    assert sorted(r.status_code for r in responses) == [201, 409]
    assert_failure(
        next(r for r in responses if r.status_code == 409),
        409,
        "recruitment_revision_conflict",
        "모집 상태가 변경되었습니다. 최신 상태를 확인해 주세요.",
    )
    async with owner.connect() as c:
        assert await c.scalar(text("SELECT count(*) FROM auction_recruitments")) == 1
        assert (
            await c.scalar(
                text(
                    "SELECT next_value FROM guild_counters WHERE counter_key='AUCTION_RECRUITMENT'"
                )
            )
            == 2
        )


@pytest.mark.parametrize("table", ["admin_audit_log", "outbox_events", "bot_notification_outbox"])
async def test_effect_failure_rolls_back_every_write(owner, runtime_client, table):
    path, headers = await setup(owner, runtime_client)
    async with owner.begin() as c:
        before = {
            t: await c.scalar(text(f"SELECT count(*) FROM {t}"))
            for t in (
                "admin_audit_log",
                "outbox_events",
                "bot_notification_outbox",
                "idempotency_records",
            )
        }
        await c.execute(text(f"REVOKE INSERT ON {table} FROM fastapi_test_runtime"))
    try:
        assert_failure(
            await convert(runtime_client, path, headers),
            500,
            "internal_error",
            "요청을 처리하는 중 내부 오류가 발생했습니다.",
        )
        async with owner.connect() as c:
            assert (await c.execute(text("SELECT status,revision FROM recruitments"))).one() == (
                "CLOSED",
                1,
            )
            for target in (
                "auction_recruitments",
                "auction_setup_drafts",
                "auction_setup_teams",
                "auction_recruitment_participants",
            ):
                assert await c.scalar(text(f"SELECT count(*) FROM {target}")) == 0
            assert await c.scalar(text("SELECT count(*) FROM user_lane_mmr")) == 1
            assert (
                await c.scalar(
                    text(
                        "SELECT next_value FROM guild_counters "
                        "WHERE counter_key='AUCTION_RECRUITMENT'"
                    )
                )
                == 1
            )
            for target, count in before.items():
                assert await c.scalar(text(f"SELECT count(*) FROM {target}")) == count
    finally:
        async with owner.begin() as c:
            await c.execute(text(f"GRANT INSERT ON {table} TO fastapi_test_runtime"))
    assert (await convert(runtime_client, path, headers)).status_code == 201


@pytest.mark.parametrize(
    "body,code,message",
    [
        (
            {**BODY, "expectedRevision": -1},
            "invalid_entry_auction_conversion_request",
            "엔트리 경매 전환 요청 형식이 올바르지 않습니다.",
        ),
        (
            {**BODY, "expectedStartAt": "2026-09-20T20:00:00"},
            "invalid_expected_start_at",
            "예상 시작 시간은 시간대가 포함된 ISO 8601 형식이어야 합니다.",
        ),
        (
            {**BODY, "expectedStartAt": 123},
            "invalid_expected_start_at",
            "예상 시작 시간은 시간대가 포함된 ISO 8601 형식이어야 합니다.",
        ),
        (
            {**BODY, "expectedRevision": True},
            "unprocessable_entity",
            "요청 JSON의 필드 또는 값이 API 계약과 일치하지 않습니다.",
        ),
        (
            {**BODY, "unknown": True},
            "unprocessable_entity",
            "요청 JSON의 필드 또는 값이 API 계약과 일치하지 않습니다.",
        ),
    ],
)
async def test_conversion_input_errors(owner, runtime_client, body, code, message):
    path, headers = await setup(owner, runtime_client)
    assert_failure(await convert(runtime_client, path, headers, body), 422, code, message)


@pytest.mark.parametrize("start", [None, " "])
async def test_default_start_time(owner, runtime_client, start):
    path, headers = await setup(owner, runtime_client, assigned=0)
    before = datetime.now(UTC) + timedelta(minutes=30)
    response = await convert(
        runtime_client,
        path,
        headers,
        {"expectedRevision": 1, **({"expectedStartAt": start} if start else {})},
    )
    assert response.status_code == 201, response.text
    after = datetime.now(UTC) + timedelta(minutes=30)
    title = response.json()["data"]["recruitment"]["title"]
    date = datetime.strptime(title.split("예상 시작 ")[1], "%Y-%m-%d %H:%M (KST)")
    utc_minute = date.replace(tzinfo=UTC) - timedelta(hours=9)
    assert before.replace(second=0, microsecond=0) <= utc_minute <= after


async def test_session_csrf_tenant_and_missing_source(owner, runtime_client):
    path, headers = await setup(owner, runtime_client)
    response = await convert(runtime_client, path, {}, key="no-csrf")
    assert_failure(response, 403, "csrf_validation_failed", "요청 출처를 확인할 수 없습니다.")
    missing = path.rsplit("/", 1)[0] + "/" + str(uuid.uuid7())
    assert_failure(
        await convert(runtime_client, missing, headers),
        404,
        "not_found",
        "요청한 대상을 찾을 수 없습니다.",
    )
    other = path.replace(f"/guilds/{GUILD_A}/", "/guilds/999999/")
    assert_failure(
        await convert(runtime_client, other, headers),
        404,
        "not_found",
        "요청한 대상을 찾을 수 없습니다.",
    )
    runtime_client.cookies.clear()
    response = await convert(runtime_client, path, {"authorization": f"Bearer {BOT}"})
    assert response.status_code == 401
