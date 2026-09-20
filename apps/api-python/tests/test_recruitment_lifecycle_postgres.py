import asyncio
import uuid

import httpx
import pytest
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncEngine
from test_auth_crypto import rust_auth
from test_context_postgres import GUILD_A, ROLE_A, USER_ID
from test_foundation import assert_failure
from test_recruitment_create_postgres import BOT
from test_recruitment_members_postgres import setup_members

pytestmark = pytest.mark.postgres


async def fixture(
    owner: AsyncEngine, client: httpx.AsyncClient, *, full: bool = True
) -> tuple[str, dict[str, str]]:
    member_path, headers = await setup_members(owner, client)
    path = member_path.removesuffix("/members")
    if full:
        record_id = uuid.UUID(path.split("/")[-1])
        async with owner.begin() as connection:
            await connection.execute(
                text("""
                INSERT INTO users (user_id, identity_kind, display_name, created_at, updated_at)
                SELECT (1000+n)::text, 'DISCORD', '참가자', now(), now()
                FROM generate_series(1,10) n
            """)
            )
            await connection.execute(
                text("""
                INSERT INTO guild_members (guild_id, user_id, display_name, status, first_seen_at)
                SELECT :guild, (1000+n)::text, '참가자', 'ACTIVE', now()
                FROM generate_series(1,10) n
            """),
                {"guild": GUILD_A},
            )
            await connection.execute(
                text("""
                INSERT INTO recruitment_participants
                    (guild_id, recruitment_id, user_id, status, joined_at)
                SELECT :guild, :id, (1000+n)::text, 'JOINED', now() FROM generate_series(1,10) n
            """),
                {"guild": GUILD_A, "id": record_id},
            )
            await connection.execute(
                text("""
                INSERT INTO recruitment_entry_assignments
                    (guild_id, recruitment_id, user_id, team, lane)
                SELECT :guild, :id, (1000+n)::text,
                    CASE WHEN n <= 5 THEN 'TEAM_1' ELSE 'TEAM_2' END,
                    (ARRAY['TOP','JUNGLE','MID','BOTTOM','SUPPORT'])[(n-1)%5+1]
                FROM generate_series(1,10) n
            """),
                {"guild": GUILD_A, "id": record_id},
            )
    return path, headers


async def call(
    client: httpx.AsyncClient,
    path: str,
    headers: dict[str, str],
    action: str,
    revision: int = 0,
    key: str | None = None,
) -> httpx.Response:
    return await client.request(
        "DELETE" if action == "cancel" else "POST",
        path + ("" if action == "cancel" else "/" + action),
        json={} if action == "close" else {"expectedRevision": revision},
        headers={**headers, "idempotency-key": key or action},
    )


async def oracle(path: str, action: str, revision: int = 0, key: str | None = None) -> dict:
    return await asyncio.to_thread(
        rust_auth,
        {
            "operation": "recruitment_lifecycle",
            "guild_id": GUILD_A,
            "user_id": USER_ID,
            "recruitment_id": path.split("/")[-1],
            "action": action,
            "key": key or action,
            "expected_revision": revision,
        },
    )


async def test_lifecycle_matches_rust_in_both_directions_and_preserves_assignments(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    path, headers = await fixture(owner, runtime_client)
    # Snapshot SQL, derived permissions and the full roster are compared against Rust.
    entry = await runtime_client.get(path + "/entry-draft")
    assert entry.status_code == 200 and entry.json()["data"] == await oracle(path, "entry")
    closed = await call(runtime_client, path, headers, "close")
    assert closed.status_code == 200
    assert closed.json()["data"] == await oracle(path, "close")
    assert closed.json()["data"]["recruitment"]["revision"] == 1
    entry = (await runtime_client.get(path + "/entry-draft")).json()["data"]
    assert entry == await oracle(path, "entry")
    assert entry["draft"]["revision"] == 0
    assert entry["capabilities"]["canEdit"] and entry["capabilities"]["canCreateSeries"]
    cancelled = await oracle(path, "cancel", 1)
    replay = await call(runtime_client, path, headers, "cancel", 1)
    assert replay.status_code == 200 and replay.json()["data"] == cancelled
    assert (await runtime_client.get(path)).json()["data"] == cancelled
    assert_failure(
        await runtime_client.get(path + "/entry-draft"),
        404,
        "not_found",
        "요청한 모집을 찾을 수 없습니다.",
    )
    restored = await call(runtime_client, path, headers, "restore", 2)
    assert restored.status_code == 200
    assert restored.json()["data"] == await oracle(path, "restore", 2)
    assert restored.json()["data"]["recruitment"]["status"] == "CLOSED"
    assert restored.json()["data"]["recruitment"]["messageId"] is None
    # Bind fixture message so reopen exercises the UPDATE outbox branch as well.
    async with owner.begin() as connection:
        await connection.execute(text("UPDATE recruitments SET message_id = '789'"))
    reopened = await oracle(path, "reopen", 3)
    replay = await call(runtime_client, path, headers, "reopen", 3)
    assert replay.status_code == 200 and replay.json()["data"] == reopened
    assert reopened["recruitment"]["status"] == "OPEN"
    assert reopened["recruitment"]["revision"] == 4
    assert reopened["draft"]["revision"] == 1 and len(reopened["draft"]["assignments"]) == 10
    async with owner.connect() as connection:
        assert await connection.scalar(text("SELECT count(*) FROM outbox_events")) == 6
        assert await connection.scalar(text("SELECT count(*) FROM admin_audit_log")) == 5
        kinds = (
            (
                await connection.execute(
                    text(
                        "SELECT notification_type FROM bot_notification_outbox ORDER BY created_at"
                    )
                )
            )
            .scalars()
            .all()
        )
        assert kinds == [
            "RECRUITMENT_MESSAGE_CREATE",
            "RECRUITMENT_MESSAGE_UPDATE",
            "RECRUITMENT_MESSAGE_DELETE",
            "RECRUITMENT_MESSAGE_CREATE",
            "RECRUITMENT_MESSAGE_UPDATE",
        ]


async def test_python_reopen_is_readable_by_rust_and_requires_membership_before_replay(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    path, headers = await fixture(owner, runtime_client)
    assert (await call(runtime_client, path, headers, "close")).status_code == 200
    reopened = await call(runtime_client, path, headers, "reopen", 1)
    assert reopened.status_code == 200 and reopened.json()["data"] == await oracle(
        path, "reopen", 1
    )
    async with owner.begin() as connection:
        await connection.execute(
            text("UPDATE guild_members SET status = 'INACTIVE' WHERE user_id = :user"),
            {"user": USER_ID},
        )
    replay = await call(runtime_client, path, headers, "reopen", 1)
    assert_failure(
        replay,
        403,
        "guild_membership_required",
        "해당 길드의 활성 멤버만 엔트리 편성을 변경할 수 있습니다.",
    )
    # The close command preserves Axum's opposite ordering: replay comes before membership.
    assert (await call(runtime_client, path, headers, "close")).status_code == 200


async def test_creator_can_cancel_restore_open_without_operator_and_with_bearer_header(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    path, headers = await fixture(owner, runtime_client, full=False)
    async with owner.begin() as connection:
        await connection.execute(text("DELETE FROM guild_member_discord_roles"))
    headers["authorization"] = "Bearer ignored-by-session-only-route"
    cancelled = await call(runtime_client, path, headers, "cancel")
    assert cancelled.status_code == 200
    restored = await call(runtime_client, path, headers, "restore", 1)
    assert (
        restored.status_code == 200 and restored.json()["data"]["recruitment"]["status"] == "OPEN"
    )
    assert restored.json()["data"]["recruitment"]["channelId"] == "123"
    assert restored.json()["data"]["recruitment"]["messageId"] is None


async def test_concurrent_cancel_revision_one_winner_and_no_duplicate_effects(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    path, headers = await fixture(owner, runtime_client, full=False)
    results = await asyncio.gather(
        *[call(runtime_client, path, headers, "cancel", 0, f"cancel-{i}") for i in range(2)]
    )
    assert sorted(result.status_code for result in results) == [200, 409]
    error = next(result for result in results if result.status_code == 409)
    assert_failure(
        error,
        409,
        "recruitment_revision_conflict",
        "모집 상태가 변경되었습니다. 최신 상태를 확인해 주세요.",
    )
    assert error.json()["error"]["details"] == {"expectedRevision": 0, "actualRevision": 1}
    async with owner.connect() as connection:
        for table in (
            "admin_audit_log",
            "outbox_events",
            "bot_notification_outbox",
            "idempotency_records",
        ):
            assert await connection.scalar(text(f"SELECT count(*) FROM {table}")) == 2


async def test_bot_close_requires_operator_and_does_not_enqueue_browser_message(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    path, _ = await fixture(owner, runtime_client)
    actor = {"id": USER_ID, "globalDisplayName": "Creator", "displayName": "Creator", "roleIds": []}
    headers = {"authorization": f"Bearer {BOT}", "idempotency-key": "1234567"}
    denied = await runtime_client.post(path + "/close", json={"actor": actor}, headers=headers)
    assert_failure(denied, 403, "operator_required", "길드 운영자 권한이 필요합니다.")
    actor["roleIds"] = [ROLE_A]
    closed = await runtime_client.post(path + "/close", json={"actor": actor}, headers=headers)
    assert closed.status_code == 200
    assert (
        await runtime_client.post(path + "/close", json={"actor": actor}, headers=headers)
    ).json()["data"] == closed.json()["data"]
    async with owner.connect() as connection:
        assert await connection.scalar(text("SELECT count(*) FROM bot_notification_outbox")) == 1


@pytest.mark.parametrize(
    "action,revision,code,message",
    [
        ("close", 0, "recruitment_not_full", "정원을 채운 뒤 엔트리 편성을 시작할 수 있습니다."),
        ("restore", 0, "recruitment_restore_unavailable", "취소된 모집만 복구할 수 있습니다."),
        (
            "cancel",
            -1,
            "invalid_recruitment_lifecycle_request",
            "모집 취소 또는 복구 요청 형식이 올바르지 않습니다.",
        ),
        ("reopen", -1, "invalid_entry_draft_request", "엔트리 편성 요청 형식이 올바르지 않습니다."),
        (
            "reopen",
            0,
            "entry_draft_not_editable",
            "현재 모집 상태에서는 엔트리 편성을 수정할 수 없습니다.",
        ),
        (
            "reopen",
            1,
            "entry_draft_revision_conflict",
            "다른 운영자가 엔트리 편성을 변경했습니다. 최신 상태를 확인해 주세요.",
        ),
    ],
)
async def test_lifecycle_failures_preserve_all_state(
    owner: AsyncEngine,
    runtime_client: httpx.AsyncClient,
    action: str,
    revision: int,
    code: str,
    message: str,
) -> None:
    path, headers = await fixture(owner, runtime_client, full=False)
    result = await call(runtime_client, path, headers, action, revision)
    assert_failure(result, 422 if action == "close" or revision < 0 else 409, code, message)
    async with owner.connect() as connection:
        assert await connection.scalar(text("SELECT revision FROM recruitments")) == 0
        assert await connection.scalar(text("SELECT count(*) FROM outbox_events")) == 1
        assert await connection.scalar(text("SELECT count(*) FROM idempotency_records")) == 1


async def test_reopen_late_failure_rolls_back_both_revisions(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    path, headers = await fixture(owner, runtime_client)
    assert (await call(runtime_client, path, headers, "close")).status_code == 200
    async with owner.begin() as connection:
        await connection.execute(
            text("REVOKE INSERT ON bot_notification_outbox FROM fastapi_test_runtime")
        )
    try:
        failed = await call(runtime_client, path, headers, "reopen", 1)
        assert_failure(
            failed, 500, "internal_error", "요청을 처리하는 중 내부 오류가 발생했습니다."
        )
        async with owner.connect() as connection:
            assert await connection.scalar(text("SELECT status FROM recruitments")) == "CLOSED"
            assert await connection.scalar(text("SELECT revision FROM recruitments")) == 1
            assert (
                await connection.scalar(text("SELECT revision FROM recruitment_entry_drafts")) == 0
            )
            assert await connection.scalar(text("SELECT count(*) FROM outbox_events")) == 2
    finally:
        async with owner.begin() as connection:
            await connection.execute(
                text("GRANT INSERT ON bot_notification_outbox TO fastapi_test_runtime")
            )


async def test_entry_projection_matches_rust_for_lane_mmr_rank_and_champions(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    path, _ = await fixture(owner, runtime_client)
    async with owner.begin() as connection:
        for statement in (
            "UPDATE guild_members SET main_lane = 'MID' WHERE user_id = '1001'",
            """INSERT INTO recruitment_preferred_lanes
                (guild_id, recruitment_id, user_id, lane, priority)
                SELECT guild_id, id, '1001', 'JUNGLE', 1 FROM recruitments""",
            """INSERT INTO user_lane_mmr
                (guild_id, season_id, user_id, lane, mmr, games_played, wins, revision, updated_at)
                SELECT guild_id, season_id, '1001', lane, rating, 10, 7, 0, now()
                FROM recruitments CROSS JOIN
                    (VALUES ('TOP', 1700.5), ('MID', 1600.0), ('JUNGLE', 1550.0)) AS m(lane,rating)
            """,
            """INSERT INTO user_champion_preferences (user_id, lane, position, champion_id)
                VALUES ('1001', 'MID', 1, 'Ahri'), ('1001', 'TOP', 2, 'Gnar'),
                       ('1001', 'TOP', 1, 'Ahri')""",
            """INSERT INTO riot_accounts (puuid, user_id, platform, routing_region, game_name,
                tag_line, normalized_game_name, normalized_tag_line, profile_icon_id,
                is_main, created_at, updated_at, origin)
                VALUES ('entry-projection', '1001', 'KR', 'ASIA', 'Tiered Player', 'KR1',
                        'tiered player', 'kr1', 29, true, now(), now(), 'RIOT')""",
            """INSERT INTO riot_records_snapshots (platform, puuid, routing_region, game_name,
                tag_line, normalized_game_name, normalized_tag_line, schema_version, snapshot,
                refreshed_at, created_at, updated_at)
                VALUES ('KR', 'entry-projection', 'ASIA', 'Tiered Player', 'KR1',
                        'tiered player', 'kr1', 1,
                        '{"ranks":[{"queueType":"RANKED_FLEX_SR","tier":"GOLD","rank":"II"},
                        {"queueType":"RANKED_SOLO_5x5","tier":" platinum ","rank":"III"}]}',
                        now(), now(), now())""",
        ):
            await connection.execute(text(statement))
    # Assignment, main lane, preferred lane, then 1500 default: preserve that precedence.
    for mmr, mutation in (
        (1700.5, "DELETE FROM recruitment_entry_assignments WHERE user_id = '1001'"),
        (1600.0, "UPDATE guild_members SET main_lane = NULL WHERE user_id = '1001'"),
        (1550.0, "DELETE FROM recruitment_preferred_lanes WHERE user_id = '1001'"),
        (1500.0, None),
    ):
        response = await runtime_client.get(path + "/entry-draft")
        assert response.status_code == 200
        snapshot = response.json()["data"]
        assert snapshot == await oracle(path, "entry")
        player = next(p for p in snapshot["draft"]["participants"] if p["id"] == "1001")
        assert player["mmr"] == mmr
        assert player["games"] == (0 if mmr == 1500 else 10)
        assert player["riotTier"] == "P3" and player["profileIconId"] == 29
        assert player["championIds"] == ["Ahri", "Gnar", "Ahri"]
        if mutation:
            async with owner.begin() as connection:
                await connection.execute(text(mutation))
    async with owner.begin() as connection:
        await connection.execute(
            text("""
            UPDATE riot_records_snapshots SET snapshot = jsonb_set(
                snapshot, '{ranks,1,tier}', '"ıron"'::jsonb)
        """)
        )
    snapshot = (await runtime_client.get(path + "/entry-draft")).json()["data"]
    assert snapshot == await oracle(path, "entry")
    assert (
        next(p for p in snapshot["draft"]["participants"] if p["id"] == "1001")["riotTier"] is None
    )
