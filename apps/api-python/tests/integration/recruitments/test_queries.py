import base64
import json
import uuid
from datetime import UTC, datetime

import httpx
import pytest
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncEngine

from monkeybot_api.core.pagination import decode_cursor, encode_cursor
from tests.support.assertions import assert_failure
from tests.support.guilds.context import GUILD_A, GUILD_B, USER_ID, authenticated_setup
from tests.support.recruitments.queries import PATH, recruitment_fixture

pytestmark = pytest.mark.postgres


async def test_actionable_pagination_ties_counts_and_links(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    await authenticated_setup(owner, runtime_client)
    first = await recruitment_fixture(owner, 1)
    second = await recruitment_fixture(owner, 2, "CLOSED")
    await recruitment_fixture(owner, 3, "CANCELLED")
    await recruitment_fixture(owner, 4, "CONVERTED")
    async with owner.begin() as connection:
        params = {"guild": GUILD_A, "id": second, "user": USER_ID}
        await connection.execute(
            text(
                "INSERT INTO recruitment_participants "
                "(guild_id, recruitment_id, user_id, status, joined_at) "
                "VALUES (:guild, :id, :user, 'JOINED', now())"
            ),
            params,
        )
        await connection.execute(
            text(
                "INSERT INTO recruitment_entry_drafts "
                "(guild_id, recruitment_id, revision, updated_at) "
                "VALUES (:guild, :id, 0, now())"
            ),
            params,
        )
        await connection.execute(
            text(
                "INSERT INTO recruitment_entry_assignments "
                "(guild_id, recruitment_id, user_id, team, lane) "
                "VALUES (:guild, :id, :user, 'TEAM_1', 'MID')"
            ),
            params,
        )
    response = await runtime_client.get(PATH, params={"state": "actionable", "limit": "1"})
    assert response.status_code == 200
    data = response.json()["data"]
    assert data["recruitments"] == [
        {
            "id": str(second),
            "displayNumber": 2,
            "title": "일반 모집",
            "status": "CLOSED",
            "targetCount": 10,
            "bestOf": 3,
            "hardFearless": True,
            "revision": 0,
            "updatedAt": "2026-09-13T01:02:03.123456Z",
            "joinedCount": 1,
            "assignedCount": 1,
            "href": f"/g/{GUILD_A}/recruitments/{second}/entry",
        }
    ]
    # Exact Rust cursor JSON/base64 representation; no float timestamp precision loss.
    raw = json.loads(
        base64.urlsafe_b64decode(data["nextCursor"] + "=" * (-len(data["nextCursor"]) % 4))
    )
    assert raw == {"updatedAtNanos": 1789261323123456000, "id": str(second)}
    following = await runtime_client.get(
        PATH, params={"state": "actionable", "limit": 1, "cursor": data["nextCursor"]}
    )
    data = following.json()["data"]
    assert data["nextCursor"] is None
    assert data["recruitments"][0]["id"] == str(first)
    assert data["recruitments"][0]["href"] == f"/g/{GUILD_A}/recruitments/{first}"
    async with owner.begin() as connection:
        await connection.execute(text("UPDATE recruitment_participants SET status = 'WITHDRAWN'"))
    data = (await runtime_client.get(PATH, params={"state": "actionable"})).json()["data"]
    assert data["recruitments"][0]["joinedCount"] == data["recruitments"][0]["assignedCount"] == 0


@pytest.mark.parametrize("mode", ["inactive_member", "disabled_guild", "other_guild"])
async def test_recruitments_hidden_from_non_members_even_service_admin(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, mode: str
) -> None:
    await authenticated_setup(owner, runtime_client)
    await recruitment_fixture(owner, 1)
    path = PATH
    async with owner.begin() as connection:
        if mode == "inactive_member":
            await connection.execute(text("UPDATE guild_members SET status = 'INACTIVE'"))
        elif mode == "disabled_guild":
            await connection.execute(text("UPDATE guilds SET status = 'DISABLED'"))
        else:
            path = f"/api/guilds/{GUILD_B}/recruitments"
    assert_failure(
        await runtime_client.get(path, params={"state": "actionable"}),
        404,
        "not_found",
        "요청한 길드를 찾을 수 없습니다.",
    )


@pytest.mark.parametrize(
    "query,code,message",
    [
        ("state=actionable&limit=0", "invalid_limit", "limit은 1 이상 100 이하이어야 합니다."),
        ("state=actionable&limit=101", "invalid_limit", "limit은 1 이상 100 이하이어야 합니다."),
        ("state=actionable&cursor=bad", "invalid_cursor", "cursor 형식이 올바르지 않습니다."),
        ("state=actionable&cursor=", "invalid_cursor", "cursor 형식이 올바르지 않습니다."),
        ("state=actionable&limit=65536", "bad_request", "요청 형식이 올바르지 않습니다."),
        ("state=actionable&state=actionable", "bad_request", "요청 형식이 올바르지 않습니다."),
        ("state=actionable&extra=1", "bad_request", "요청 형식이 올바르지 않습니다."),
        ("", "bad_request", "요청 형식이 올바르지 않습니다."),
    ],
)
async def test_recruitment_query_errors(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, query: str, code: str, message: str
) -> None:
    await authenticated_setup(owner, runtime_client)
    assert_failure(await runtime_client.get(PATH + "?" + query), 400, code, message)


async def test_query_extraction_precedes_auth_but_cursor_validation_follows_auth(
    runtime_client: httpx.AsyncClient,
) -> None:
    assert_failure(
        await runtime_client.get(PATH), 400, "bad_request", "요청 형식이 올바르지 않습니다."
    )
    assert_failure(
        await runtime_client.get(PATH + "?state=actionable&cursor=bad"),
        401,
        "unauthorized",
        "인증이 필요합니다.",
    )


async def test_live_series_hides_recruitment_until_cancelled_or_deleted(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    await authenticated_setup(owner, runtime_client)
    record_id = await recruitment_fixture(owner, 1, "CLOSED")
    async with owner.begin() as connection:
        await connection.execute(
            text("""
            INSERT INTO series (guild_id, id, season_id, source_recruitment_id, display_number,
                                title, best_of, status, revision, started_at, updated_at)
            SELECT guild_id, :series_id, season_id, id, 1, title, 3, 'IN_PROGRESS', 0, now(), now()
            FROM recruitments WHERE id = :id
        """),
            {"id": record_id, "series_id": uuid.uuid7()},
        )
    assert (await runtime_client.get(PATH + "?state=actionable")).json()["data"][
        "recruitments"
    ] == []
    async with owner.begin() as connection:
        await connection.execute(
            text(
                "UPDATE series SET status = 'CANCELLED', "
                "terminal_reason = 'CANCELLED', ended_at = now()"
            )
        )
    assert (
        len((await runtime_client.get(PATH + "?state=actionable")).json()["data"]["recruitments"])
        == 1
    )
    async with owner.begin() as connection:
        await connection.execute(
            text(
                "UPDATE series SET status = 'IN_PROGRESS', "
                "terminal_reason = NULL, ended_at = NULL, deleted_at = now()"
            )
        )
    assert (
        len((await runtime_client.get(PATH + "?state=actionable")).json()["data"]["recruitments"])
        == 1
    )
    async with owner.begin() as connection:
        await connection.execute(text("UPDATE recruitments SET converted_at = now()"))
    assert (await runtime_client.get(PATH + "?state=actionable")).json()["data"][
        "recruitments"
    ] == []


def test_cursor_round_trip_preserves_microseconds() -> None:
    date = datetime(2026, 9, 13, 1, 2, 3, 123456, tzinfo=UTC)
    record_id = uuid.UUID(int=2)
    assert decode_cursor(encode_cursor(date, record_id)) == (date, record_id)


@pytest.mark.parametrize("status", ["OPEN", "CLOSED", "CANCELLED"])
async def test_detail_preserves_axum_statuses_and_participant_projection(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, status: str
) -> None:
    await authenticated_setup(owner, runtime_client)
    record_id = await recruitment_fixture(owner, 1, status)
    params = {"guild": GUILD_A, "id": record_id, "user": USER_ID}
    async with owner.begin() as connection:
        await connection.execute(
            text("""
            INSERT INTO recruitment_participants
                (guild_id, recruitment_id, user_id, status, joined_at)
            VALUES (:guild, :id, :user, 'JOINED', now())
        """),
            params,
        )
        await connection.execute(
            text("""
            INSERT INTO recruitment_preferred_lanes
                (guild_id, recruitment_id, user_id, priority, lane)
            VALUES (:guild, :id, :user, 1, 'MID'), (:guild, :id, :user, 2, 'TOP')
        """),
            params,
        )
    response = await runtime_client.get(f"{PATH}/{record_id}")
    assert response.status_code == 200
    assert response.json()["data"] == {
        "recruitment": {
            "id": str(record_id),
            "displayNumber": 1,
            "title": "일반 모집",
            "status": status,
            "targetCount": 10,
            "bestOf": 3,
            "hardFearless": True,
            "revision": 0,
            "updatedAt": "2026-09-13T01:02:03.123456Z",
            "channelId": None,
            "messageId": None,
            "createdById": USER_ID,
        },
        "participants": [
            {"id": USER_ID, "displayName": "테스트 멤버", "preferredLanes": ["MID", "TOP"]}
        ],
        "links": {"entry": f"/g/{GUILD_A}/recruitments/{record_id}/entry"},
    }
    assert (await runtime_client.head(f"{PATH}/{record_id}")).content == b""
    async with owner.begin() as connection:
        await connection.execute(text("UPDATE recruitment_participants SET status = 'WITHDRAWN'"))
    assert (await runtime_client.get(f"{PATH}/{record_id}")).json()["data"]["participants"] == []


@pytest.mark.parametrize("mode", ["converted", "missing", "other_guild", "inactive_member"])
async def test_detail_retains_axum_not_found_boundaries(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, mode: str
) -> None:
    await authenticated_setup(owner, runtime_client)
    record_id = await recruitment_fixture(owner, 1, "CONVERTED" if mode == "converted" else "OPEN")
    path = f"{PATH}/{record_id}"
    if mode == "missing":
        path = f"{PATH}/{uuid.uuid7()}"
    elif mode == "other_guild":
        path = f"/api/guilds/{GUILD_B}/recruitments/{record_id}"
    elif mode == "inactive_member":
        async with owner.begin() as connection:
            await connection.execute(text("UPDATE guild_members SET status = 'INACTIVE'"))
    message = (
        "요청한 길드를 찾을 수 없습니다."
        if mode in {"other_guild", "inactive_member"}
        else "요청한 모집을 찾을 수 없습니다."
    )
    assert_failure(await runtime_client.get(path), 404, "not_found", message)


async def test_detail_path_validation_precedes_auth(runtime_client: httpx.AsyncClient) -> None:
    assert_failure(
        await runtime_client.get(f"{PATH}/invalid"),
        400,
        "bad_request",
        "요청 형식이 올바르지 않습니다.",
    )
    assert_failure(
        await runtime_client.get(f"{PATH}/{uuid.uuid7()}"),
        401,
        "unauthorized",
        "인증이 필요합니다.",
    )
