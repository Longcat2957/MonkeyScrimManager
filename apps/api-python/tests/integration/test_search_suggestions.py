import pytest
from sqlalchemy import text

from tests.support.assertions import assert_failure
from tests.support.guilds.context import (
    GUILD_B,
    ROLE_B,
    USER_ID,
    authenticated_setup,
    guild_fixture,
)
from tests.support.search_suggestions import GUILD_PATH, PUBLIC_PATH, member, snapshot

pytestmark = pytest.mark.postgres


@pytest.fixture(autouse=True)
async def isolated_cache(owner):
    async with owner.begin() as connection:
        await connection.execute(text("DELETE FROM riot_records_snapshots"))
    yield
    async with owner.begin() as connection:
        await connection.execute(text("DELETE FROM riot_records_snapshots"))


async def suggestions(client, path, query):
    response = await client.get(path, params=query)
    assert response.status_code == 200, response.text
    assert response.json()["meta"]["requestId"] == response.headers["x-request-id"]
    return response.json()["data"]["suggestions"]


async def test_public_suggestions_use_cached_canonical_identity_without_linkage_or_privacy(
    owner,
    runtime_client,
):
    await authenticated_setup(owner, runtime_client)
    await member(owner, 1, "Alpha", show_riot=True, game_name="Longcat")
    await member(owner, 2, "Alpine", show_riot=False, game_name="LongHidden")
    await member(owner, 3, "Other platform", game_name="Lollipop", platform="JP1")
    await member(owner, 4, "Dummy", game_name="LongDummy", origin="DUMMY")
    await snapshot(owner, "duplicate", "LONGCAT", icon=77)
    await snapshot(owner, "cache-only", "LoneWolf", icon=12)
    runtime_client.cookies.clear()
    rows = await suggestions(runtime_client, PUBLIC_PATH, {"q": "  ＬＯ  "})
    assert rows == [
        {
            "kind": "riotAccount",
            "value": "LoneWolf#KR1",
            "label": "LoneWolf#KR1",
            "description": "KR",
            "profileIconId": 12,
        },
        {
            "kind": "riotAccount",
            "value": "LONGCAT#KR1",
            "label": "LONGCAT#KR1",
            "description": "KR",
            "profileIconId": 77,
        },
        {
            "kind": "riotAccount",
            "value": "LongHidden#KR1",
            "label": "LongHidden#KR1",
            "description": "KR",
            "profileIconId": 2,
        },
    ]
    assert all(secret not in str(rows) for secret in ("puuid", "userId", "account:", USER_ID))
    # Equal timestamps prefer the standalone snapshot; newer linked identity wins afterward.
    async with owner.begin() as connection:
        await connection.execute(text("UPDATE riot_accounts SET updated_at='2026-09-02'"))
    assert (await suggestions(runtime_client, PUBLIC_PATH, {"q": "longcat#k"}))[0][
        "profileIconId"
    ] == 77
    async with owner.begin() as connection:
        await connection.execute(text("UPDATE riot_accounts SET updated_at='2026-09-03'"))
    assert (await suggestions(runtime_client, PUBLIC_PATH, {"q": "longcat#k"}))[0][
        "value"
    ] == "Longcat#KR1"
    japanese = await suggestions(runtime_client, PUBLIC_PATH, {"q": "lo", "platform": " JP "})
    assert japanese[0]["value"] == "Lollipop#KR1" and japanese[0]["description"] == "JP1"
    response = await runtime_client.head(PUBLIC_PATH, params={"q": "lo"})
    assert response.status_code == 200 and response.content == b""


async def test_guild_suggestions_filter_members_and_only_show_public_main_riot_accounts(
    owner,
    runtime_client,
):
    await authenticated_setup(owner, runtime_client)
    await guild_fixture(owner, GUILD_B, "Other guild", ROLE_B)
    await member(owner, 1, "Alpha", show_riot=True, game_name="Longcat")
    await member(owner, 2, "Alpine", show_riot=False, game_name="Hidden")
    await member(owner, 3, "Albatross", game_name="NoPrivacy")
    await member(owner, 4, "Alchemy", active=False)
    await member(owner, 5, "Alone", guild=GUILD_B)
    await member(owner, 6, "Alabaster", deleted=True)
    await member(owner, 7, "Also", show_riot=True, game_name="AltOnly", main=False)
    rows = await suggestions(runtime_client, GUILD_PATH, {"q": " AL "})
    assert [row["value"] for row in rows] == ["Albatross", "Alpha", "Alpine", "Also"]
    assert rows[1] == {
        "kind": "guildMember",
        "value": "Alpha",
        "label": "Alpha",
        "description": "Longcat#KR1",
        "profileIconId": 1,
    }
    assert all(row["description"] is row["profileIconId"] is None for row in [rows[0], *rows[2:]])
    assert all(secret not in str(rows) for secret in ("userId", "puuid", "Hidden", "NoPrivacy"))
    assert await suggestions(runtime_client, GUILD_PATH, {"q": "ＡＬ"}) == []
    assert await suggestions(runtime_client, GUILD_PATH, {"q": "not found"}) == []
    response = await runtime_client.head(GUILD_PATH, params={"q": "Al"})
    assert response.status_code == 200 and response.content == b""


async def test_suggestions_prefix_is_literal_and_limits_follow_stable_order(owner, runtime_client):
    await authenticated_setup(owner, runtime_client)
    for number, name in enumerate(
        ["Al%Literal", "Al_Literal", "Al\\Literal", *[f"Al{number}" for number in range(8)]], 1
    ):
        await member(owner, number, name)
        await snapshot(owner, str(number), name)
    for path in (GUILD_PATH, PUBLIC_PATH):
        for prefix, name in [("Al%", "Al%Literal"), ("Al_", "Al_Literal"), ("Al\\", "Al\\Literal")]:
            rows = await suggestions(runtime_client, path, {"q": prefix})
            assert [row["value"].split("#")[0] for row in rows] == [name]
        rows = await suggestions(runtime_client, path, {"q": "Al"})
        assert len(rows) == 8
        assert [row["value"].split("#")[0] for row in rows[:3]] == ["Al%Literal", "Al0", "Al1"]
        limited = await suggestions(runtime_client, path, {"q": "Al", "limit": "+0002"})
        assert limited == rows[:2]


@pytest.mark.parametrize(
    "icon, expected",
    [(None, None), ("7", None), (-1, None), (2147483648, None), (2147483647, 2147483647), (1.5, 2)],
)
async def test_snapshot_icon_extraction_matches_postgres_numeric_rules(
    owner,
    runtime_client,
    icon,
    expected,
):
    await snapshot(owner, "icon", "Known", icon=icon)
    rows = await suggestions(runtime_client, PUBLIC_PATH, {"q": "kn"})
    assert rows[0]["profileIconId"] == expected


@pytest.mark.parametrize(
    "query",
    [
        "",
        "?limit=1",
        "?q=Al&q=Bo",
        "?q=Al&other=1",
        "?q=Al&limit=1&limit=2",
        "?q=Al&limit=-1",
        "?q=Al&limit=1.0",
        "?q=Al&limit=",
        "?q=Al&limit=18446744073709551616",
        "?q=Al&limit=１",
    ],
)
async def test_suggestion_query_shape_follows_guild_authentication(owner, runtime_client, query):
    assert_failure(
        await runtime_client.get(GUILD_PATH + query),
        401,
        "unauthorized",
        "인증이 필요합니다.",
    )
    await authenticated_setup(owner, runtime_client)
    for path in (GUILD_PATH, PUBLIC_PATH):
        assert_failure(
            await runtime_client.get(path + query),
            400,
            "bad_request",
            "요청 형식이 올바르지 않습니다.",
        )


async def test_suggestion_value_errors_preserve_order_and_unicode_length(owner, runtime_client):
    query = {"q": "a", "limit": "0"}
    assert_failure(
        await runtime_client.get(GUILD_PATH, params=query),
        401,
        "unauthorized",
        "인증이 필요합니다.",
    )
    await authenticated_setup(owner, runtime_client)
    for path in (GUILD_PATH, PUBLIC_PATH):
        for limit in ("0", "9", "18446744073709551615"):
            response = await runtime_client.get(path, params={"q": "a", "limit": limit})
            assert_failure(
                response, 400, "invalid_search_limit", "검색 결과 개수 설정이 올바르지 않습니다."
            )
            assert response.json()["error"]["details"] == {"maxLimit": 8}
        for value in ("", " a ", "😀"):
            response = await runtime_client.get(path, params={"q": value})
            assert_failure(
                response, 400, "invalid_search_query", "검색어를 두 글자 이상 입력해 주세요."
            )
            assert response.json()["error"]["details"] == {"minLength": 2}
        assert await suggestions(runtime_client, path, {"q": "😀😀"}) == []
        assert await suggestions(runtime_client, path, {"q": "\x1ca"}) == []
    # Public NFKC/casefold may expand one scalar; guild display names are only trimmed.
    assert await suggestions(runtime_client, PUBLIC_PATH, {"q": "ß"}) == []
    assert_failure(
        await runtime_client.get(GUILD_PATH, params={"q": "ß"}),
        400,
        "invalid_search_query",
        "검색어를 두 글자 이상 입력해 주세요.",
    )
    response = await runtime_client.get(PUBLIC_PATH, params={"q": "Al", "platform": "unsupported"})
    assert_failure(response, 400, "invalid_region", "지원하지 않는 Riot 플랫폼입니다.")
    assert response.json()["error"]["details"] == {"platform": "unsupported"}
    assert_failure(
        await runtime_client.get(GUILD_PATH, params={"q": "Al", "platform": "KR"}),
        400,
        "bad_request",
        "요청 형식이 올바르지 않습니다.",
    )


@pytest.mark.parametrize("mode", ["missing", "inactive", "disabled", "not_member"])
async def test_guild_suggestions_hide_access_even_from_service_admin(owner, runtime_client, mode):
    await authenticated_setup(owner, runtime_client)
    path = GUILD_PATH
    async with owner.begin() as connection:
        if mode == "missing":
            path = f"/api/guilds/{GUILD_B}/records/suggestions"
        elif mode == "inactive":
            await connection.execute(text("UPDATE guild_members SET status='INACTIVE'"))
        elif mode == "disabled":
            await connection.execute(text("UPDATE guilds SET status='DISABLED'"))
        else:
            await connection.execute(text("DELETE FROM guild_members"))
    assert_failure(
        await runtime_client.get(path, params={"q": "Al"}),
        404,
        "not_found",
        "요청한 길드를 찾을 수 없습니다.",
    )


async def test_suggestion_database_failures_are_safe(owner, runtime_client, caplog):
    async with owner.begin() as connection:
        await connection.execute(text("REVOKE SELECT ON riot_accounts FROM fastapi_test_runtime"))
    try:
        response = await runtime_client.get(PUBLIC_PATH, params={"q": "Al"})
        assert_failure(
            response, 500, "internal_error", "요청을 처리하는 중 내부 오류가 발생했습니다."
        )
        assert "riot_accounts" not in response.text
        assert any(record.message == "database_request_failed" for record in caplog.records)
    finally:
        async with owner.begin() as connection:
            await connection.execute(text("GRANT SELECT ON riot_accounts TO fastapi_test_runtime"))
