import asyncio
import uuid

import pytest
from sqlalchemy import text
from test_auth_crypto import rust_auth
from test_context_postgres import GUILD_A, ROLE_A, USER_ID
from test_foundation import assert_failure
from test_recruitment_create_postgres import BODY, BOT, PATH
from test_recruitment_lifecycle_postgres import fixture
from test_series_flow_postgres import command, game
from test_series_flow_postgres import setup as series_setup

pytestmark = pytest.mark.postgres
SETTINGS = f"/api/guilds/{GUILD_A}/settings"
SEASONS = f"/api/guilds/{GUILD_A}/seasons"
CATALOG = f"/internal/guilds/{GUILD_A}/discord-catalog"
CATALOG_BODY = {
    "roles": [
        {"id": ROLE_A, "name": " Operators ", "position": 1, "managed": False},
        {"id": "9001", "name": " Zeta ", "position": 3, "managed": False},
        {"id": "9002", "name": "Managed", "position": 10, "managed": True},
    ],
    "channels": [
        {"id": "8001", "name": " recruitment ", "position": 2, "kind": "TEXT"},
        {
            "id": "8002",
            "name": " results ",
            "parentName": " Group ",
            "position": 1,
            "kind": "ANNOUNCEMENT",
        },
    ],
}


async def oracle(action, body=None, key=None):
    return await asyncio.to_thread(
        rust_auth,
        {
            "operation": "guild_admin",
            "guild_id": GUILD_A,
            "user_id": USER_ID,
            "action": action,
            "body": body,
            "key": key,
        },
    )


async def setup(owner, client):
    _, headers = await fixture(owner, client, full=False)
    async with owner.begin() as c:
        await c.execute(
            text("UPDATE guild_config SET display_name='Admin guild' WHERE guild_id=:guild"),
            {"guild": GUILD_A},
        )
    response = await client.get(SETTINGS, headers=headers)
    assert response.status_code == 200, response.text
    return headers, response.json()["data"]


async def catalog(client, body=None):
    return await client.put(
        CATALOG,
        json=CATALOG_BODY if body is None else body,
        headers={"authorization": f"Bearer {BOT}"},
    )


def settings_body(snapshot, **changes):
    return {**snapshot["settings"], "expectedRevision": snapshot["revision"], **changes}


async def rotate(client, headers, current, *, name="새 시즌", key="season-1"):
    return await client.post(
        SEASONS,
        json={"name": name, "expectedCurrentSeasonId": current},
        headers={**headers, "idempotency-key": key},
    )


async def test_catalog_settings_rust_both_directions_and_sorting(owner, runtime_client):
    headers, initial = await setup(owner, runtime_client)
    assert initial == await oracle("settings")
    assert (await catalog(runtime_client)).status_code == 200
    response = await runtime_client.get(SETTINGS, headers=headers)
    snapshot = response.json()["data"]
    assert snapshot == await oracle("settings")
    assert [r["id"] for r in snapshot["roles"]] == ["9001", ROLE_A]
    assert snapshot["channels"] == [
        {"id": "8002", "name": "results", "parentName": "Group"},
        {"id": "8001", "name": "recruitment", "parentName": None},
    ]
    assert snapshot["revision"] == initial["revision"]
    body = settings_body(
        snapshot,
        displayName="  변경된 길드  ",
        recruitmentChannelId="8001",
        resultChannelId="8002",
        isExternallyPublic=True,
    )
    response = await runtime_client.put(SETTINGS, json=body, headers=headers)
    assert response.status_code == 200, response.text
    assert response.json()["data"] == await oracle("settings")
    current = response.json()["data"]
    assert current["settings"]["displayName"] == "변경된 길드"
    changed = await oracle("update", settings_body(current, entryChannelId="8002"))
    assert (await runtime_client.get(SETTINGS, headers=headers)).json()["data"] == changed
    await oracle("catalog", {"roles": [], "channels": []})
    # Existing selections survive catalog removal. A no-op update still increments revision.
    response = await runtime_client.put(SETTINGS, json=settings_body(changed), headers=headers)
    assert response.status_code == 200
    assert response.json()["data"]["revision"] == changed["revision"] + 1
    assert response.json()["data"]["roles"] == []
    assert response.json()["data"] == await oracle("settings")
    async with owner.connect() as c:
        logs = (
            await c.execute(
                text(
                    "SELECT before_summary, after_summary FROM admin_audit_log "
                    "WHERE action='GUILD_SETTINGS_UPDATED' ORDER BY created_at, id"
                )
            )
        ).all()
        assert len(logs) == 3
        assert logs[0][1]["displayName"] == "변경된 길드"


@pytest.mark.parametrize(
    "field,value",
    [
        ("operatorRoleId", "9002"),
        ("operatorRoleId", "9999"),
        ("recruitmentChannelId", "9999"),
        ("entryChannelId", "9999"),
        ("resultChannelId", "9999"),
        ("logChannelId", "9999"),
    ],
)
async def test_settings_rejects_changed_unavailable_selection(owner, runtime_client, field, value):
    headers, before = await setup(owner, runtime_client)
    await catalog(runtime_client)
    response = await runtime_client.put(
        SETTINGS, json=settings_body(before, **{field: value}), headers=headers
    )
    assert_failure(
        response,
        422,
        "invalid_settings_selection",
        "현재 Discord 서버에서 선택할 수 없는 설정입니다.",
    )
    assert response.json()["error"]["details"] == {"field": field}
    assert (await runtime_client.get(SETTINGS, headers=headers)).json()["data"][
        "revision"
    ] == before["revision"]


async def test_settings_operator_change_and_concurrent_revision(owner, runtime_client):
    headers, before = await setup(owner, runtime_client)
    await catalog(runtime_client)
    results = await asyncio.gather(
        *[
            runtime_client.put(
                SETTINGS, json=settings_body(before, displayName=name), headers=headers
            )
            for name in ("first", "second")
        ]
    )
    assert sorted(r.status_code for r in results) == [200, 409]
    conflict = next(r for r in results if r.status_code == 409)
    assert_failure(conflict, 409, "revision_conflict", "최신 상태를 다시 확인해야 합니다.")
    latest = (await runtime_client.get(SETTINGS, headers=headers)).json()["data"]
    assert conflict.json()["error"]["details"] == {
        "expectedRevision": before["revision"],
        "currentRevision": latest["revision"],
        "updatedAt": latest["updatedAt"],
        "updatedBy": USER_ID,
    }
    response = await runtime_client.put(
        SETTINGS, json=settings_body(latest, operatorRoleId="9001"), headers=headers
    )
    assert response.status_code == 200, response.text
    for path in (SETTINGS, SEASONS):
        assert_failure(
            await runtime_client.get(path, headers=headers),
            403,
            "operator_required",
            "길드 운영자 권한이 필요합니다.",
        )


@pytest.mark.parametrize(
    "body,field",
    [
        (
            {
                "roles": [{"id": GUILD_A, "name": "everyone", "position": 0, "managed": False}],
                "channels": [],
            },
            "roles",
        ),
        (
            {
                "roles": [{"id": "9001", "name": "  ", "position": 0, "managed": False}],
                "channels": [],
            },
            "roles.name",
        ),
        (
            {
                "roles": [],
                "channels": [{"id": "8001", "name": "ok", "position": -1, "kind": "TEXT"}],
            },
            "channels",
        ),
        (
            {
                "roles": [],
                "channels": [
                    {"id": "8001", "name": "ok", "parentName": " ", "position": 0, "kind": "TEXT"}
                ],
            },
            "channels.parentName",
        ),
        ({"roles": [CATALOG_BODY["roles"][0]] * 2, "channels": []}, "roles"),
    ],
)
async def test_catalog_validation_is_atomic(owner, runtime_client, body, field):
    headers, _ = await setup(owner, runtime_client)
    await catalog(runtime_client)
    before = (await runtime_client.get(SETTINGS, headers=headers)).json()["data"]
    response = await catalog(runtime_client, body)
    assert_failure(
        response, 422, "invalid_discord_catalog", "Discord 역할 및 채널 목록이 올바르지 않습니다."
    )
    assert response.json()["error"]["details"] == {"field": field}
    assert (await runtime_client.get(SETTINGS, headers=headers)).json()["data"] == before


async def test_catalog_and_settings_database_failures_rollback(owner, runtime_client):
    headers, _ = await setup(owner, runtime_client)
    await catalog(runtime_client)
    before = (await runtime_client.get(SETTINGS, headers=headers)).json()["data"]
    async with owner.begin() as c:
        await c.execute(
            text("REVOKE INSERT ON guild_discord_catalog_state FROM fastapi_test_runtime")
        )
    try:
        assert (await catalog(runtime_client, {"roles": [], "channels": []})).status_code == 500
    finally:
        async with owner.begin() as c:
            await c.execute(
                text("GRANT INSERT ON guild_discord_catalog_state TO fastapi_test_runtime")
            )
    assert (await runtime_client.get(SETTINGS, headers=headers)).json()["data"] == before
    async with owner.begin() as c:
        await c.execute(text("REVOKE INSERT ON admin_audit_log FROM fastapi_test_runtime"))
    try:
        response = await runtime_client.put(
            SETTINGS, json=settings_body(before, displayName="rollback"), headers=headers
        )
        assert response.status_code == 500
    finally:
        async with owner.begin() as c:
            await c.execute(text("GRANT INSERT ON admin_audit_log TO fastapi_test_runtime"))
    assert (await runtime_client.get(SETTINGS, headers=headers)).json()["data"] == before


@pytest.mark.parametrize("rust_first", [False, True])
async def test_season_rotation_rust_replay_and_original_name_hash(
    owner, runtime_client, rust_first
):
    headers, _ = await setup(owner, runtime_client)
    initial = (await runtime_client.get(SEASONS, headers=headers)).json()["data"]
    assert initial == await oracle("seasons")
    current = initial["current"]["id"]
    body = {"name": "  다음 시즌  ", "expectedCurrentSeasonId": current}
    if rust_first:
        expected = await oracle("rotate", body, "season-1")
        response = await rotate(runtime_client, headers, current, name=body["name"])
    else:
        response = await rotate(runtime_client, headers, current, name=body["name"])
        assert response.status_code == 200, response.text
        expected = await oracle("rotate", body, "season-1")
    assert response.status_code == 200 and response.json()["data"] == expected
    assert expected["current"]["name"] == "다음 시즌"
    assert expected["completed"][0]["id"] == current
    assert (await runtime_client.get(SEASONS, headers=headers)).json()["data"] == await oracle(
        "seasons"
    )
    assert_failure(
        await rotate(runtime_client, headers, current, name="다음 시즌"),
        409,
        "idempotency_conflict",
        "같은 Idempotency-Key가 다른 요청에 사용되었습니다.",
    )
    async with owner.connect() as c:
        assert (
            await c.scalar(
                text("SELECT count(*) FROM admin_audit_log WHERE action='SEASON_ROTATED'")
            )
            == 1
        )
        assert await c.scalar(text("SELECT season_id FROM recruitments")) == uuid.UUID(current)


async def test_seasons_summary_and_rotation_preserves_game_mmr(owner, runtime_client):
    series_path, headers, _ = await series_setup(owner, runtime_client, best_of=1)
    assert (
        await command(runtime_client, series_path, headers, "submit", 0, body=game())
    ).status_code == 201
    snapshot = (await runtime_client.get(SEASONS, headers=headers)).json()["data"]
    assert snapshot == await oracle("seasons")
    assert snapshot["current"]["participantCount"] == 10
    assert snapshot["current"]["generalSeriesCount"] == 1
    assert snapshot["current"]["confirmedGameCount"] == 1
    current = snapshot["current"]["id"]
    response = await rotate(runtime_client, headers, current)
    assert response.status_code == 200, response.text
    result = response.json()["data"]
    assert result["current"]["participantCount"] == 0
    assert result["completed"][0]["confirmedGameCount"] == 1
    assert result == await oracle("seasons")
    created = await runtime_client.post(
        PATH, json=BODY, headers={**headers, "idempotency-key": "new-season-create"}
    )
    assert created.status_code == 201
    async with owner.connect() as c:
        assert await c.scalar(text("SELECT count(*) FROM mmr_ledger_entries")) == 10
        assert (
            await c.scalar(
                text("SELECT count(*) FROM user_lane_mmr WHERE season_id=:id"),
                {"id": uuid.UUID(result["current"]["id"])},
            )
            == 0
        )
        assert await c.scalar(
            text("SELECT season_id FROM recruitments WHERE id=:id"),
            {"id": uuid.UUID(created.json()["data"]["recruitment"]["id"])},
        ) == uuid.UUID(result["current"]["id"])


async def test_season_concurrency_and_permission_before_replay(owner, runtime_client):
    headers, _ = await setup(owner, runtime_client)
    current = (await runtime_client.get(SEASONS, headers=headers)).json()["data"]["current"]["id"]
    results = await asyncio.gather(
        rotate(runtime_client, headers, current, key="a"),
        rotate(runtime_client, headers, current, key="b"),
    )
    assert sorted(r.status_code for r in results) == [200, 409]
    conflict = next(r for r in results if r.status_code == 409)
    assert_failure(
        conflict,
        409,
        "current_season_conflict",
        "현재 시즌이 변경되었습니다. 최신 상태를 다시 확인해 주세요.",
    )
    current_id = next(r for r in results if r.status_code == 200).json()["data"]["current"]["id"]
    assert conflict.json()["error"]["details"] == {
        "expectedCurrentSeasonId": current,
        "currentSeasonId": current_id,
    }
    async with owner.begin() as c:
        await c.execute(text("DELETE FROM guild_member_discord_roles"))
    key = "a" if results[0].status_code == 200 else "b"
    assert_failure(
        await rotate(runtime_client, headers, current, key=key),
        403,
        "operator_required",
        "길드 운영자 권한이 필요합니다.",
    )


async def test_season_audit_failure_rolls_back_rotation_and_idempotency(owner, runtime_client):
    headers, _ = await setup(owner, runtime_client)
    before = (await runtime_client.get(SEASONS, headers=headers)).json()["data"]
    async with owner.begin() as c:
        await c.execute(text("REVOKE INSERT ON admin_audit_log FROM fastapi_test_runtime"))
    try:
        assert (await rotate(runtime_client, headers, before["current"]["id"])).status_code == 500
    finally:
        async with owner.begin() as c:
            await c.execute(text("GRANT INSERT ON admin_audit_log TO fastapi_test_runtime"))
    assert (await runtime_client.get(SEASONS, headers=headers)).json()["data"] == before
    async with owner.connect() as c:
        assert (
            await c.scalar(
                text(
                    "SELECT count(*) FROM idempotency_records "
                    "WHERE route_template='/api/guilds/{guildId}/seasons'"
                )
            )
            == 0
        )
    assert (await rotate(runtime_client, headers, before["current"]["id"])).status_code == 200


async def test_admin_validation_auth_and_missing_season(owner, runtime_client):
    headers, before = await setup(owner, runtime_client)
    assert (
        await runtime_client.put(CATALOG, json=CATALOG_BODY, headers=headers)
    ).status_code == 401
    assert (await runtime_client.get(SETTINGS, headers={"cookie": ""})).status_code == 401
    assert (
        await runtime_client.get(SEASONS.replace(GUILD_A, "999"), headers=headers)
    ).status_code == 404
    no_csrf = {k: v for k, v in headers.items() if k != "x-csrf-token"}
    assert (
        await runtime_client.put(SETTINGS, json=settings_body(before), headers=no_csrf)
    ).status_code == 403
    for changes, code, message in [
        (
            {"expectedRevision": -1},
            "invalid_expected_revision",
            "expectedRevision은 0 이상이어야 합니다.",
        ),
        (
            {"recruitmentChannelId": None},
            "recruitment_channel_required",
            "모집 채널은 반드시 설정해야 합니다.",
        ),
        ({"displayName": " "}, "invalid_guild_settings", "길드 설정 값을 확인해 주세요."),
        ({"operatorRoleId": GUILD_A}, "invalid_guild_settings", "길드 설정 값을 확인해 주세요."),
    ]:
        assert_failure(
            await runtime_client.put(
                SETTINGS, json=settings_body(before, **changes), headers=headers
            ),
            422,
            code,
            message,
        )
    current = (await runtime_client.get(SEASONS, headers=headers)).json()["data"]["current"]["id"]
    assert_failure(
        await rotate(runtime_client, headers, current, key="has space"),
        400,
        "invalid_idempotency_key",
        "Idempotency-Key가 올바르지 않습니다.",
    )
    for name in (" ", "가" * 101):
        assert_failure(
            await rotate(runtime_client, headers, current, name=name),
            422,
            "invalid_season_name",
            "시즌 이름을 확인해 주세요.",
        )
    async with owner.begin() as c:
        await c.execute(text("UPDATE seasons SET status='CLOSED', ended_at=now()"))
    assert_failure(
        await runtime_client.get(SEASONS, headers=headers),
        409,
        "no_active_season",
        "길드에 진행 중인 시즌이 없습니다.",
    )


@pytest.mark.parametrize(
    "field,value",
    [("status_code", 201), ("response_body", {"current": {"status": "INVALID"}, "completed": []})],
)
async def test_season_corrupt_replay_fails_safely(owner, runtime_client, field, value):
    import json

    headers, _ = await setup(owner, runtime_client)
    current = (await runtime_client.get(SEASONS, headers=headers)).json()["data"]["current"]["id"]
    assert (await rotate(runtime_client, headers, current)).status_code == 200
    async with owner.begin() as c:
        if field == "status_code":
            await c.execute(
                text("UPDATE idempotency_records SET status_code=201 WHERE route_template=:route"),
                {"route": "/api/guilds/{guildId}/seasons"},
            )
        else:
            await c.execute(
                text(
                    "UPDATE idempotency_records SET response_body=CAST(:body AS jsonb) "
                    "WHERE route_template=:route"
                ),
                {"route": "/api/guilds/{guildId}/seasons", "body": json.dumps(value)},
            )
    assert_failure(
        await rotate(runtime_client, headers, current),
        500,
        "internal_error",
        "요청을 처리하는 중 내부 오류가 발생했습니다.",
    )
