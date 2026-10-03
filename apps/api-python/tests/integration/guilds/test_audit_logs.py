import base64
import json
import uuid
from datetime import UTC, datetime

import pytest
from sqlalchemy import text

from tests.support.assertions import assert_failure
from tests.support.guilds.audit_logs import (
    CHECKPOINT,
    CREATED_AT,
    FALLBACK_USER,
    IDS,
    PATH,
    insert_audit,
    projected_entries,
    setup,
)
from tests.support.guilds.context import GUILD_A, USER_ID

pytestmark = pytest.mark.postgres


def token(payload):
    return base64.urlsafe_b64encode(payload.encode()).decode().rstrip("=")


async def test_audit_projection_tenant_boundary_and_keyset_pagination(owner, runtime_client):
    await projected_entries(owner, runtime_client)
    response = await runtime_client.get(PATH)
    assert response.status_code == 200, response.text
    snapshot = response.json()["data"]
    entries = snapshot["entries"]
    assert [row["id"] for row in entries] == [str(record_id) for record_id in IDS]
    assert snapshot["nextCursor"] is None
    assert entries[0] == {
        "id": str(IDS[0]),
        "principalType": "USER",
        "principalId": USER_ID,
        "principalDisplayName": "테스트 멤버",
        "action": "MEMBER_MMR_ADJUSTED",
        "targetType": "GUILD_MEMBER_MMR",
        "targetId": USER_ID,
        "targetDisplayName": "테스트 멤버",
        "checkpointId": None,
        "requestId": "request-mmr",
        "beforeSummary": {"mmr": 1500, "nested": [None, True, {"name": "한글"}]},
        "afterSummary": [1520, "unmodified"],
        "result": "APPLIED",
        "note": "수동 조정",
        "createdAt": "2026-01-02T03:04:05.123456Z",
    }
    assert entries[1]["principalType"] == "BOT"
    assert entries[1]["targetDisplayName"] == "Audit Guild"
    assert entries[2]["principalType"] == "SYSTEM"
    assert entries[2]["targetDisplayName"] == "현재 시즌"
    assert entries[2]["checkpointId"] == str(CHECKPOINT)
    assert entries[2]["result"] == "REVERTED"
    assert entries[3]["principalType"] == "MIGRATION"
    assert entries[3]["targetDisplayName"] == "금요일 내전 모집"
    assert all(row["principalDisplayName"] is None for row in entries[1:4])
    assert entries[4]["principalId"] == FALLBACK_USER
    assert entries[4]["principalDisplayName"] == "Global Fallback"
    assert entries[4]["targetDisplayName"] is None
    assert entries[5]["targetDisplayName"] == "Audit Guild"
    assert entries[6]["targetDisplayName"] is None
    assert entries[7]["principalDisplayName"] is None
    assert entries[7]["targetDisplayName"] is None
    assert entries[7]["action"] == "UNRECOGNIZED_ACTION"

    first = (await runtime_client.get(PATH, params={"limit": "+1"})).json()["data"]
    assert first["entries"] == entries[:1]
    cursor = first["nextCursor"]
    assert json.loads(base64.urlsafe_b64decode(cursor + "=" * (-len(cursor) % 4))) == {
        "createdAtNanos": 1767323045123456000,
        "id": str(IDS[0]),
    }
    second = (await runtime_client.get(PATH, params={"limit": "1", "cursor": cursor})).json()[
        "data"
    ]
    assert second["entries"] == entries[1:2]
    remaining = (
        await runtime_client.get(PATH, params={"limit": "100", "cursor": second["nextCursor"]})
    ).json()["data"]
    assert remaining == {"entries": entries[2:], "nextCursor": None}
    head = await runtime_client.head(PATH)
    assert head.status_code == 200 and head.content == b""


async def test_audit_authorization_empty_and_validation_order(owner, runtime_client):
    for query, status, code, message in [
        ("?unknown=1", 400, "bad_request", "요청 형식이 올바르지 않습니다."),
        ("?limit=1&limit=2", 400, "bad_request", "요청 형식이 올바르지 않습니다."),
        ("?cursor=a&cursor=b", 400, "bad_request", "요청 형식이 올바르지 않습니다."),
        ("?limit=invalid", 401, "unauthorized", "인증이 필요합니다."),
        ("?cursor=invalid", 401, "unauthorized", "인증이 필요합니다."),
    ]:
        assert_failure(await runtime_client.get(PATH + query), status, code, message)
    assert_failure(
        await runtime_client.get(PATH.replace(GUILD_A, "invalid")),
        400,
        "bad_request",
        "요청 형식이 올바르지 않습니다.",
    )
    await setup(owner, runtime_client)
    assert (await runtime_client.get(PATH)).json()["data"] == {"entries": [], "nextCursor": None}
    async with owner.begin() as connection:
        await connection.execute(text("DELETE FROM guild_member_discord_roles"))
    assert_failure(
        await runtime_client.get(PATH),
        403,
        "operator_required",
        "길드 운영자 권한이 필요합니다.",
    )
    # Value validation precedes the repository's permission check.
    assert_failure(
        await runtime_client.get(PATH + "?limit=0&cursor=invalid"),
        400,
        "invalid_limit",
        "limit은 1 이상 100 이하이어야 합니다.",
    )
    async with owner.begin() as connection:
        await connection.execute(text("UPDATE guild_members SET status='INACTIVE'"))
    assert_failure(
        await runtime_client.get(PATH), 404, "not_found", "요청한 길드를 찾을 수 없습니다."
    )
    async with owner.begin() as connection:
        await connection.execute(text("UPDATE guild_members SET status='ACTIVE'"))
        await connection.execute(text("UPDATE guilds SET status='DISABLED'"))
    assert_failure(
        await runtime_client.get(PATH), 404, "not_found", "요청한 길드를 찾을 수 없습니다."
    )
    assert_failure(
        await runtime_client.get(PATH.replace(GUILD_A, "999")),
        404,
        "not_found",
        "요청한 길드를 찾을 수 없습니다.",
    )


async def test_audit_limit_and_canonical_cursor_errors(owner, runtime_client):
    await setup(owner, runtime_client)
    for limit in ("", "0", "101", "-1", "text", "65536", " 1", "1.0", "١"):
        response = await runtime_client.get(PATH, params={"limit": limit})
        assert_failure(response, 400, "invalid_limit", "limit은 1 이상 100 이하이어야 합니다.")
        assert response.json()["error"]["details"] == {"min": 1, "max": 100}
    record_id = str(IDS[0])
    canonical = f'{{"createdAtNanos":0,"id":"{record_id}"}}'
    malformed = [
        "",
        "not-base64!",
        "a" * 513,
        token("{}"),
        token(canonical) + "=",
        token(f'{{"id":"{record_id}","createdAtNanos":0}}'),
        token(canonical.replace(":0", ": 0")),
        token(canonical.replace(":0", ":-0")),
        token(canonical.replace(":0", ":true")),
        token(canonical.replace(":0", ":0.0")),
        token(canonical.replace(":0", ":9223372036854775808")),
        token(canonical.replace(record_id, record_id.replace("-", ""))),
        token(canonical.replace('"createdAtNanos":0', '"createdAtNanos":0,"createdAtNanos":0')),
        token(canonical[:-1] + ',"extra":1}'),
    ]
    for cursor in malformed:
        assert_failure(
            await runtime_client.get(PATH, params={"cursor": cursor}),
            400,
            "invalid_cursor",
            "cursor 형식이 올바르지 않습니다.",
        )


@pytest.mark.parametrize(
    "created_at,nanos",
    [
        (CREATED_AT, 1767323045123456999),
        (datetime(1999, 12, 31, 23, 59, 59, 999999, tzinfo=UTC), 946684799999998001),
    ],
)
async def test_audit_cursor_matches_sqlx_submicrosecond_tuple_boundary(
    owner, runtime_client, created_at, nanos
):
    await setup(owner, runtime_client)
    low, middle, high = (uuid.UUID(int=value) for value in (1, 2, 3))
    for record_id in (low, middle, high):
        await insert_audit(owner, record_id, created_at=created_at)
    cursor = token(f'{{"createdAtNanos":{nanos},"id":"{middle}"}}')
    response = await runtime_client.get(PATH, params={"cursor": cursor})
    assert response.status_code == 200, response.text
    assert [row["id"] for row in response.json()["data"]["entries"]] == [str(low)]


async def test_audit_database_failure_is_safe(owner, runtime_client):
    await setup(owner, runtime_client)
    async with owner.begin() as connection:
        await connection.execute(text("REVOKE SELECT ON admin_audit_log FROM fastapi_test_runtime"))
    try:
        assert_failure(
            await runtime_client.get(PATH),
            500,
            "internal_error",
            "요청을 처리하는 중 내부 오류가 발생했습니다.",
        )
    finally:
        async with owner.begin() as connection:
            await connection.execute(
                text("GRANT SELECT ON admin_audit_log TO fastapi_test_runtime")
            )
