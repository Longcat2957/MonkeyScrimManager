import asyncio
import uuid

import httpx
import pytest
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncEngine

from tests.support.assertions import assert_failure
from tests.support.auctions.recruitments import (
    BODY,
    LANES,
    PATH,
    ROSTER,
    call,
    filled,
    rust,
    setup,
    teams,
)
from tests.support.guilds.context import GUILD_A, ROLE_A, USER_ID
from tests.support.recruitments.creation import BOT, START

pytestmark = pytest.mark.postgres


@pytest.mark.parametrize("rust_first", [False, True])
async def test_auction_entire_workflow_rust_replay_both_directions(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, rust_first: bool
) -> None:
    h = await setup(owner, runtime_client)

    async def both(action, identifier, body):
        key = str(uuid.uuid7())
        if rust_first:
            expected = await rust(action, identifier, body, key)
            actual = await call(runtime_client, h, action, identifier, body, key)
        else:
            expected = await call(runtime_client, h, action, identifier, body, key)
            actual = await rust(action, identifier, body, key)
        assert actual == expected
        return actual

    created = await both("create", None, BODY)
    record = created["recruitment"]
    identifier = record["id"]
    assert record["title"] == "10인 경매 내전 모집 #1 · 예상 시작 2026-09-13 20:00 (KST)"
    assert record["rules"] == {"hardFearless": True, "semifinalBestOf": None, "finalBestOf": 3}
    async with owner.begin() as c:
        await c.execute(
            text("UPDATE auction_recruitments SET channel_id='123456789',message_id='987654321'")
        )
    for i in range(10):
        await both("join", identifier, {"memberId": ROSTER[i], "declaredLane": LANES[i % 5]})
    await both("leave", identifier, {"memberId": ROSTER[0]})
    await both("join", identifier, {"memberId": ROSTER[0], "declaredLane": "TOP"})
    closed = await both("close", identifier, {})
    assert closed["recruitment"]["revision"] == 13
    draft = (await runtime_client.get(f"{PATH}/{identifier}/setup-draft")).json()["data"]
    assert draft == await rust("draft", identifier, {}, "read")
    assert draft["draft"]["revision"] == 12 and not draft["permissions"]["canCreateTournament"]
    saved = await both("setup", identifier, {"expectedRevision": 12, "teams": teams()})
    assert saved["permissions"] == {"canEdit": True, "canCreateTournament": True}
    assert saved["draft"]["teams"][0]["teamName"] == "팀장 1"
    await both("dm", identifier, {})
    assert await both("dm", identifier, {}) == {
        "queuedRecipientCount": 0,
        "alreadyQueuedRecipientCount": 10,
    }
    await both("reopen", identifier, {"expectedRevision": 13})
    await both("close", identifier, {})
    assert (await runtime_client.get(f"{PATH}/{identifier}/setup-draft")).json()["data"] == saved
    cancelled = await both("cancel", identifier, {"expectedRevision": 15})
    assert (await runtime_client.get(f"{PATH}/{identifier}")).json()["data"] == cancelled
    assert cancelled == await rust("detail", identifier, {}, "read")
    assert (await runtime_client.get(PATH + "?state=actionable")).json()["data"][
        "recruitments"
    ] == []
    restored = await both("restore", identifier, {"expectedRevision": 16})
    assert (
        restored["recruitment"]["status"] == "CLOSED"
        and restored["recruitment"]["messageId"] is None
    )
    assert (await runtime_client.get(f"{PATH}/{identifier}/setup-draft")).json()["data"] == saved
    async with owner.connect() as c:
        assert await c.scalar(text("SELECT count(*) FROM mmr_ledger_entries")) == 0
        notifications = (
            await c.execute(text("SELECT notification_type,payload FROM bot_notification_outbox"))
        ).all()
        assert all(
            payload["schemaVersion"] == 2 for kind, payload in notifications if "MESSAGE" in kind
        )
        assert {kind for kind, _ in notifications} == {
            "AUCTION_RECRUITMENT_MESSAGE_CREATE",
            "AUCTION_RECRUITMENT_MESSAGE_UPDATE",
            "AUCTION_RECRUITMENT_MESSAGE_DELETE",
            "AUCTION_RECRUITMENT_PARTICIPANT_CONFIRMATION_DM_CREATE",
        }
        assert await c.scalar(
            text("SELECT bool_and(topic LIKE :pattern) FROM outbox_events"),
            {"pattern": "guild:%:auction-recruitment:%"},
        )


@pytest.mark.parametrize("rust_first", [False, True])
async def test_auction_bot_hash_self_join_default_lane_and_notifications(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, rust_first: bool
) -> None:
    h = await setup(owner, runtime_client)
    actor = {
        "id": USER_ID,
        "globalDisplayName": "운영자",
        "displayName": "운영자",
        "avatarHash": None,
        "roleIds": [ROLE_A],
    }
    key_number = 5000

    async def both(action, identifier, body):
        nonlocal key_number
        key_number += 1
        key = str(key_number)
        if rust_first:
            expected = await rust(action, identifier, body, key, True)
            actual = await call(runtime_client, h, action, identifier, body, key, True)
        else:
            expected = await call(runtime_client, h, action, identifier, body, key, True)
            actual = await rust(action, identifier, body, key, True)
        assert actual == expected
        return actual

    created = await both("create", None, {**BODY, "actor": actor})
    identifier = created["recruitment"]["id"]
    async with owner.begin() as c:
        await c.execute(
            text("UPDATE guild_members SET main_lane='SUPPORT' WHERE user_id=:u"), {"u": USER_ID}
        )
        await c.execute(
            text("UPDATE auction_recruitments SET channel_id='123456789',message_id='987654321'")
        )
    joined = await both("join", identifier, {"actor": actor})
    assert joined["participants"][0]["declaredLane"] == "SUPPORT"
    await both("leave", identifier, {"actor": actor, "memberId": USER_ID})
    participant = {**actor, "id": ROSTER[0], "roleIds": [], "displayName": "다른 참가자"}
    await both("join", identifier, {"actor": actor, "member": participant, "declaredLane": "MID"})
    async with owner.connect() as c:
        assert await c.scalar(text("SELECT count(*) FROM bot_notification_outbox")) == 4


async def test_auction_twenty_person_and_captain_withdrawal_preserves_other_settings(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    h, identifier = await filled(owner, runtime_client, count=20)
    closed = await call(runtime_client, h, "close", identifier)
    assert closed["recruitment"]["rules"]["semifinalBestOf"] == 3
    assert closed["recruitment"]["teamCount"] == 4
    saved = await call(
        runtime_client, h, "setup", identifier, {"expectedRevision": 20, "teams": teams(4)}
    )
    await call(runtime_client, h, "reopen", identifier, {"expectedRevision": 21})
    await call(runtime_client, h, "leave", identifier, {"memberId": ROSTER[0]})
    await call(
        runtime_client, h, "join", identifier, {"memberId": ROSTER[0], "declaredLane": "JUNGLE"}
    )
    await call(runtime_client, h, "close", identifier)
    changed = (await runtime_client.get(f"{PATH}/{identifier}/setup-draft")).json()["data"]
    assert changed["draft"]["revision"] == 23
    assert changed["draft"]["teams"][0] == {**saved["draft"]["teams"][0], "captainUserId": None}
    assert changed["draft"]["teams"][1:] == saved["draft"]["teams"][1:]
    assert not changed["permissions"]["canCreateTournament"]
    async with owner.connect() as c:
        row = (
            await c.execute(
                text(
                    "SELECT declared_lane,mmr_at_join,snapshot_origin "
                    "FROM auction_recruitment_participants WHERE user_id='1000'"
                )
            )
        ).one()
        assert tuple(row) == ("JUNGLE", 1500, "CAPTURED")


@pytest.mark.parametrize(
    "body,code,details",
    [
        (
            {"format": "TEN_PERSON", "finalBestOf": 2},
            "invalid_auction_recruitment_best_of",
            {"stage": "final", "bestOf": 2},
        ),
        (
            {"format": "TEN_PERSON", "semifinalBestOf": 3},
            "auction_semifinal_best_of_unsupported",
            None,
        ),
        (
            {"format": "TWENTY_PERSON", "semifinalBestOf": 2},
            "invalid_auction_recruitment_best_of",
            {"stage": "semifinal", "bestOf": 2},
        ),
        ({"format": "THIRTY_PERSON"}, "invalid_auction_recruitment_create_request", None),
        (
            {"format": "TEN_PERSON", "finalBestOf": True},
            "invalid_auction_recruitment_create_request",
            None,
        ),
    ],
)
async def test_auction_create_validation(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, body, code, details
) -> None:
    h = await setup(owner, runtime_client)
    response = await runtime_client.post(PATH, json={"expectedStartAt": START, **body}, headers=h)
    assert response.status_code == 422
    problem = response.json()["error"]
    assert problem["code"] == code and problem["message"]
    assert problem.get("details") == details
    async with owner.connect() as c:
        assert await c.scalar(text("SELECT count(*) FROM auction_recruitments")) == 0
        assert await c.scalar(text("SELECT count(*) FROM idempotency_records")) == 0


@pytest.mark.parametrize(
    "change,code,message",
    [
        ("count", "invalid_auction_setup_team_count", "경매 형식에 맞는 팀 수를 입력해야 합니다."),
        (
            "index",
            "invalid_auction_setup_team_index",
            "팀 순서는 1번부터 연속해서 입력해야 합니다.",
        ),
        (
            "blank",
            "invalid_auction_setup_team_name",
            "팀 이름은 비어 있지 않은 64바이트 이하의 값이어야 합니다.",
        ),
        (
            "bytes",
            "invalid_auction_setup_team_name",
            "팀 이름은 비어 있지 않은 64바이트 이하의 값이어야 합니다.",
        ),
        ("points", "invalid_auction_setup_initial_points", "초기 포인트는 1 이상이어야 합니다."),
        (
            "duplicate",
            "duplicate_auction_setup_captain",
            "같은 참가자를 여러 팀의 팀장으로 지정할 수 없습니다.",
        ),
        (
            "outside",
            "auction_setup_captain_not_joined",
            "팀장은 현재 모집에 참가한 길드 멤버여야 합니다.",
        ),
    ],
)
async def test_auction_setup_validation_rolls_back(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, change, code, message
) -> None:
    h, identifier = await filled(owner, runtime_client)
    await call(runtime_client, h, "close", identifier)
    inputs = teams()
    if change == "count":
        inputs.pop()
    elif change == "index":
        inputs[0]["teamIndex"] = 2
    elif change in {"blank", "bytes"}:
        inputs[0]["teamName"] = "\u3000" if change == "blank" else "한" * 22
    elif change == "points":
        inputs[0]["initialPoints"] = 0
    else:
        inputs[1]["captainUserId"] = ROSTER[0] if change == "duplicate" else ROSTER[15]
    response = await runtime_client.patch(
        f"{PATH}/{identifier}/setup-draft",
        json={"expectedRevision": 10, "teams": inputs},
        headers=h,
    )
    assert_failure(response, 422, code, message)
    draft = (await runtime_client.get(f"{PATH}/{identifier}/setup-draft")).json()["data"]["draft"]
    assert draft["revision"] == 10 and draft["teams"][0]["captainUserId"] is None


async def test_auction_permission_replay_order_and_cancelled_detail(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    h, identifier = await filled(owner, runtime_client)
    closed = await call(runtime_client, h, "close", identifier, key="closed")
    await call(
        runtime_client,
        h,
        "setup",
        identifier,
        {"expectedRevision": 10, "teams": teams()},
        key="saved",
    )
    async with owner.begin() as c:
        await c.execute(text("DELETE FROM guild_member_discord_roles"))
    assert await call(runtime_client, h, "close", identifier, key="closed") == closed
    response = await runtime_client.patch(
        f"{PATH}/{identifier}/setup-draft",
        json={"expectedRevision": 10, "teams": teams()},
        headers={**h, "idempotency-key": "saved"},
    )
    assert_failure(
        response,
        403,
        "operator_required",
        "경매 팀장 설정을 변경하려면 현재 길드 운영자 권한이 필요합니다.",
    )
    cancelled = await call(runtime_client, h, "cancel", identifier, {"expectedRevision": 11})
    assert cancelled["permissions"]["canRestore"] and not cancelled["permissions"]["canCancel"]
    assert (await runtime_client.get(f"{PATH}/{identifier}")).json()["data"] == cancelled
    assert (await runtime_client.get(f"{PATH}/{identifier}/setup-draft")).status_code == 404
    await call(runtime_client, h, "restore", identifier, {"expectedRevision": 12})


async def test_auction_last_slot_and_revision_concurrency(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    h, identifier = await filled(owner, runtime_client)
    await call(runtime_client, h, "leave", identifier, {"memberId": ROSTER[9]})
    responses = await asyncio.gather(
        *[
            runtime_client.post(
                f"{PATH}/{identifier}/members",
                json={"memberId": member, "declaredLane": "TOP"},
                headers={**h, "idempotency-key": member},
            )
            for member in ROSTER[9:11]
        ]
    )
    assert sorted(r.status_code for r in responses) == [200, 422]
    assert_failure(
        next(r for r in responses if r.status_code == 422),
        422,
        "recruitment_full",
        "모집 정원이 가득 찼습니다.",
    )
    await call(runtime_client, h, "close", identifier)
    responses = await asyncio.gather(
        *[
            runtime_client.patch(
                f"{PATH}/{identifier}/setup-draft",
                json={"expectedRevision": 12, "teams": teams()},
                headers={**h, "idempotency-key": f"draft-{i}"},
            )
            for i in range(2)
        ]
    )
    assert sorted(r.status_code for r in responses) == [200, 409]
    assert_failure(
        next(r for r in responses if r.status_code == 409),
        409,
        "auction_setup_draft_revision_conflict",
        "경매 팀장 설정이 변경되었습니다. 최신 상태를 확인해 주세요.",
    )


async def test_auction_late_outbox_failure_rolls_back_every_artifact(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    h = await setup(owner, runtime_client)
    async with owner.begin() as c:
        await c.execute(
            text(
                "ALTER TABLE bot_notification_outbox "
                "ADD CONSTRAINT auction_test_failure CHECK (notification_type <> "
                "'AUCTION_RECRUITMENT_MESSAGE_CREATE')"
            )
        )
    try:
        response = await runtime_client.post(PATH, json=BODY, headers=h)
        assert_failure(
            response, 500, "internal_error", "요청을 처리하는 중 내부 오류가 발생했습니다."
        )
        async with owner.connect() as c:
            for table in (
                "auction_recruitments",
                "auction_setup_drafts",
                "auction_setup_teams",
                "idempotency_records",
                "admin_audit_log",
                "outbox_events",
            ):
                assert await c.scalar(text(f"SELECT count(*) FROM {table}")) == 0
            assert (
                await c.scalar(
                    text(
                        "SELECT next_value FROM guild_counters "
                        "WHERE counter_key='AUCTION_RECRUITMENT'"
                    )
                )
                == 1
            )
    finally:
        async with owner.begin() as c:
            await c.execute(
                text("ALTER TABLE bot_notification_outbox DROP CONSTRAINT auction_test_failure")
            )
    await call(runtime_client, h, "create", body=BODY, key=h["idempotency-key"])


async def test_auction_listing_pagination_cancel_restore_open(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    h = await setup(owner, runtime_client)
    identifiers = [
        (await call(runtime_client, h, "create", body=BODY))["recruitment"]["id"] for _ in range(3)
    ]
    response = await runtime_client.get(PATH + "?state=actionable&limit=2")
    data = response.json()["data"]
    assert [r["id"] for r in data["recruitments"]] == identifiers[::-1][:2]
    next_page = (
        await runtime_client.get(PATH + "?state=actionable&limit=2&cursor=" + data["nextCursor"])
    ).json()["data"]
    assert [r["id"] for r in next_page["recruitments"]] == [identifiers[0]] and next_page[
        "nextCursor"
    ] is None
    await call(runtime_client, h, "cancel", identifiers[0], {"expectedRevision": 0})
    restored = await call(runtime_client, h, "restore", identifiers[0], {"expectedRevision": 1})
    assert restored["recruitment"]["status"] == "OPEN"
    assert (await runtime_client.head(f"{PATH}/{identifiers[0]}")).status_code == 200
    async with owner.begin() as c:
        await c.execute(
            text("UPDATE guild_members SET status='INACTIVE' WHERE user_id=:u"), {"u": USER_ID}
        )
    for suffix in (
        "?state=actionable",
        "/" + identifiers[0],
        "/" + identifiers[0] + "/setup-draft",
    ):
        assert (await runtime_client.get(PATH + suffix)).status_code == 404


@pytest.mark.parametrize(
    "action,body,status,code,message",
    [
        (
            "close",
            {},
            422,
            "auction_recruitment_not_full",
            "정원을 채운 뒤 모집을 마감할 수 있습니다.",
        ),
        (
            "reopen",
            {"expectedRevision": 0},
            409,
            "auction_recruitment_reopen_unavailable",
            "팀장 선택 단계의 경매 모집만 모집 단계로 되돌릴 수 있습니다.",
        ),
        (
            "restore",
            {"expectedRevision": 0},
            409,
            "auction_recruitment_restore_unavailable",
            "취소된 경매 모집만 복구할 수 있습니다.",
        ),
        (
            "reopen",
            {"expectedRevision": 1},
            409,
            "recruitment_revision_conflict",
            "경매 모집 상태가 변경되었습니다. 최신 상태를 확인해 주세요.",
        ),
        (
            "participant-confirmation-dms",
            {},
            409,
            "participant_confirmation_dm_unavailable",
            "마감된 모집에서만 참가 확정 DM을 발송할 수 있습니다.",
        ),
        (
            "setup-draft",
            {"expectedRevision": 0, "teams": teams()},
            404,
            "auction_setup_draft_not_found",
            "요청한 경매 팀장 설정을 찾을 수 없습니다.",
        ),
    ],
)
async def test_auction_lifecycle_invalid_states(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, action, body, status, code, message
) -> None:
    h = await setup(owner, runtime_client)
    identifier = (await call(runtime_client, h, "create", body=BODY))["recruitment"]["id"]
    response = await runtime_client.request(
        "PATCH" if action == "setup-draft" else "POST",
        f"{PATH}/{identifier}/{action}",
        json=body,
        headers=h,
    )
    assert_failure(response, status, code, message)
    if action == "close":
        assert response.json()["error"]["details"] == {"joinedCount": 0, "targetCount": 10}
    async with owner.connect() as c:
        assert await c.scalar(text("SELECT revision FROM auction_recruitments")) == 0
        assert await c.scalar(text("SELECT count(*) FROM idempotency_records")) == 1


async def test_auction_fixed_mmr_rejoin_and_missing_member_errors(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    h = await setup(owner, runtime_client)
    identifier = (await call(runtime_client, h, "create", body=BODY))["recruitment"]["id"]
    body = {"memberId": ROSTER[0], "declaredLane": "TOP"}
    await call(runtime_client, h, "join", identifier, body)
    async with owner.begin() as c:
        await c.execute(text("UPDATE user_lane_mmr SET mmr=1777 WHERE user_id='1000'"))
        assert (
            await c.scalar(text("SELECT mmr_at_join FROM auction_recruitment_participants")) == 1500
        )
    duplicate = await runtime_client.post(f"{PATH}/{identifier}/members", json=body, headers=h)
    assert_failure(duplicate, 422, "already_participant", "이미 참가 중인 사용자입니다.")
    await call(runtime_client, h, "leave", identifier, {"memberId": ROSTER[0]})
    absent = await runtime_client.request(
        "DELETE", f"{PATH}/{identifier}/members/{ROSTER[0]}", json={}, headers=h
    )
    assert_failure(absent, 422, "not_participant", "현재 모집 참가자가 아닙니다.")
    await call(runtime_client, h, "join", identifier, body)
    async with owner.connect() as c:
        assert (
            await c.scalar(text("SELECT mmr_at_join FROM auction_recruitment_participants")) == 1777
        )
    missing = await runtime_client.post(
        f"{PATH}/{identifier}/members", json={**body, "memberId": "9999"}, headers=h
    )
    assert_failure(
        missing, 404, "guild_member_not_found", "길드에 소속된 사용자를 찾을 수 없습니다."
    )
    async with owner.begin() as c:
        await c.execute(text("UPDATE guild_members SET status='INACTIVE' WHERE user_id='1001'"))
    inactive = await runtime_client.post(
        f"{PATH}/{identifier}/members", json={**body, "memberId": "1001"}, headers=h
    )
    assert_failure(
        inactive, 409, "guild_member_inactive", "비활성화된 길드 멤버는 모집에 추가할 수 없습니다."
    )


async def test_auction_bot_missing_lane_and_operator_priority(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    h = await setup(owner, runtime_client)
    identifier = (await call(runtime_client, h, "create", body=BODY))["recruitment"]["id"]
    actor = {"id": USER_ID, "globalDisplayName": "사용자", "displayName": "사용자", "roleIds": []}
    bot_headers = {"authorization": f"Bearer {BOT}", "idempotency-key": "12345"}
    missing = await runtime_client.post(
        f"{PATH}/{identifier}/members", json={"actor": actor}, headers=bot_headers
    )
    assert_failure(
        missing,
        422,
        "auction_declared_lane_required",
        "주 라인을 먼저 설정하거나 모집 메시지에서 참가 라인을 선택해 주세요.",
    )
    denied = await runtime_client.post(
        f"{PATH}/{uuid.uuid7()}/members",
        json={"actor": actor, "member": {**actor, "id": "1000"}, "declaredLane": "MID"},
        headers=bot_headers,
    )
    assert_failure(denied, 403, "operator_required", "길드 운영자 권한이 필요합니다.")
    # Failed actor synchronization must also roll back the original operator role.
    async with owner.connect() as c:
        assert await c.scalar(text("SELECT count(*) FROM guild_member_discord_roles")) == 1


async def test_auction_dm_dummy_skip_payload_conflict_rollback(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    h, identifier = await filled(owner, runtime_client)
    dummy = "dmy_" + str(uuid.uuid7())
    async with owner.begin() as c:
        await c.execute(
            text(
                "INSERT INTO users (user_id,identity_kind,display_name,created_at,updated_at) "
                "VALUES (:u,'DUMMY','더미',now(),now())"
            ),
            {"u": dummy},
        )
        await c.execute(
            text(
                "INSERT INTO guild_members (guild_id,user_id,display_name,status,first_seen_at) "
                "VALUES (:g,:u,'더미','ACTIVE',now())"
            ),
            {"g": GUILD_A, "u": dummy},
        )
    await call(runtime_client, h, "leave", identifier, {"memberId": "1000"})
    await call(runtime_client, h, "join", identifier, {"memberId": dummy, "declaredLane": "MID"})
    await call(runtime_client, h, "close", identifier)
    assert await call(runtime_client, h, "dm", identifier) == {
        "queuedRecipientCount": 9,
        "alreadyQueuedRecipientCount": 0,
    }
    async with owner.begin() as c:
        await c.execute(
            text(
                "UPDATE bot_notification_outbox SET payload='{}' "
                "WHERE notification_type='AUCTION_RECRUITMENT_PARTICIPANT_CONFIRMATION_DM_CREATE' "
                "AND channel_id='1001'"
            )
        )
    response = await runtime_client.post(
        f"{PATH}/{identifier}/participant-confirmation-dms", json={}, headers=h
    )
    assert_failure(response, 500, "internal_error", "요청을 처리하는 중 내부 오류가 발생했습니다.")
    async with owner.connect() as c:
        assert (
            await c.scalar(
                text(
                    "SELECT count(*) FROM admin_audit_log "
                    "WHERE action='AUCTION_RECRUITMENT_PARTICIPANT_CONFIRMATION_DMS_REQUESTED'"
                )
            )
            == 1
        )


@pytest.mark.parametrize(
    "fault,code,message",
    [
        (
            "channel",
            "recruitment_channel_not_configured",
            "Discord 모집 채널을 먼저 설정해야 내전을 만들 수 있습니다.",
        ),
        (
            "season",
            "no_active_season",
            "길드에 진행 중인 시즌이 없어 경매 모집을 만들 수 없습니다.",
        ),
        ("guild", "guild_inactive", "현재 길드에서는 command를 실행할 수 없습니다."),
    ],
)
async def test_auction_creation_dependencies(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, fault, code, message
) -> None:
    h = await setup(owner, runtime_client)
    async with owner.begin() as c:
        await c.execute(
            text(
                {
                    "channel": "UPDATE guild_config SET recruitment_channel_id=NULL",
                    "season": "UPDATE seasons SET status='CLOSED',ended_at=now()",
                    "guild": "UPDATE guilds SET status='DISABLED'",
                }[fault]
            )
        )
    response = await runtime_client.post(PATH, json=BODY, headers=h)
    assert_failure(response, 409, code, message)


async def test_auction_converted_detail_links_and_actionable_exclusion(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    h = await setup(owner, runtime_client)
    identifier = (await call(runtime_client, h, "create", body=BODY))["recruitment"]["id"]
    tournament = uuid.uuid7()
    async with owner.begin() as c:
        await c.execute(
            text("""INSERT INTO auction_tournaments
            (guild_id,id,season_id,source_recruitment_id,display_number,title,format,status,
             revision,started_at,created_by,updated_at)
            SELECT guild_id,:t,season_id,id,1,title,format,'BIDDING',0,now(),created_by,now()
            FROM auction_recruitments"""),
            {"t": tournament},
        )
    assert (await runtime_client.get(PATH + "?state=actionable")).json()["data"][
        "recruitments"
    ] == []
    async with owner.begin() as c:
        await c.execute(text("UPDATE auction_tournaments SET status='CANCELLED',ended_at=now()"))
    assert (
        len((await runtime_client.get(PATH + "?state=actionable")).json()["data"]["recruitments"])
        == 1
    )
    async with owner.begin() as c:
        await c.execute(
            text(
                "UPDATE auction_recruitments SET status='CONVERTED',"
                "converted_at=now(),closed_at=now()"
            )
        )
    result = (await runtime_client.get(f"{PATH}/{identifier}")).json()["data"]
    assert result["recruitment"]["href"] == f"/g/{GUILD_A}/auction/tournaments/{tournament}/draft"
    assert result == await rust("detail", identifier, {}, "read-converted")
    assert not any(result["permissions"].values()) and result["links"]["draft"] is None
    assert (await runtime_client.get(PATH + "?state=actionable")).json()["data"][
        "recruitments"
    ] == []


async def test_auction_join_audit_failure_rolls_back_mmr_and_revisions(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    h = await setup(owner, runtime_client)
    identifier = (await call(runtime_client, h, "create", body=BODY))["recruitment"]["id"]
    async with owner.begin() as c:
        await c.execute(
            text(
                "ALTER TABLE admin_audit_log ADD CONSTRAINT auction_join_test_failure "
                "CHECK (action <> 'AUCTION_RECRUITMENT_MEMBER_ADDED')"
            )
        )
    try:
        response = await runtime_client.post(
            f"{PATH}/{identifier}/members",
            json={"memberId": ROSTER[0], "declaredLane": "TOP"},
            headers=h,
        )
        assert_failure(
            response, 500, "internal_error", "요청을 처리하는 중 내부 오류가 발생했습니다."
        )
        async with owner.connect() as c:
            assert await c.scalar(text("SELECT revision FROM auction_recruitments")) == 0
            assert await c.scalar(text("SELECT revision FROM auction_setup_drafts")) == 0
            for table in ("auction_recruitment_participants", "user_lane_mmr"):
                assert await c.scalar(text(f"SELECT count(*) FROM {table}")) == 0
            assert await c.scalar(text("SELECT count(*) FROM idempotency_records")) == 1
    finally:
        async with owner.begin() as c:
            await c.execute(
                text("ALTER TABLE admin_audit_log DROP CONSTRAINT auction_join_test_failure")
            )
