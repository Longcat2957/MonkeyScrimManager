import logging

import httpx
import pytest
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncEngine

from monkeybot_api.repositories.guilds import record_profile as repository
from tests.support.assertions import assert_failure
from tests.support.guilds.context import GUILD_A, GUILD_B, USER_ID
from tests.support.guilds.record_profile import DUMMY, OTHER, PATH, PLAYER, setup

pytestmark = pytest.mark.postgres


async def test_profile_exact_identity_ordered_preferences_accounts_and_head(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    await setup(owner, runtime_client)
    for query in (PLAYER, "  lOnGcAt  ", "Longcat#KR1", "ＬｏｎｇＣａｔ#ＫＲ１", "LongAlt#KR2"):
        response = await runtime_client.get(PATH, params={"q": query})
        assert response.status_code == 200, response.text
        assert response.json()["meta"]["requestId"] == response.headers["x-request-id"]
        assert response.json()["data"] == {
            "member": {
                "displayName": "Longcat",
                "avatarUrl": f"https://cdn.discordapp.com/avatars/{PLAYER}/a_0123abcdef.webp?size=128",
                "mainLane": "MID",
                "profileHref": f"/users/{PLAYER}",
            },
            "championPreferences": {
                "TOP": ["Gnar"],
                "JUNGLE": [],
                "MID": ["Ahri", "Orianna"],
                "BOTTOM": [],
                "SUPPORT": [],
            },
            "riotAccounts": [
                {
                    "platform": "KR",
                    "gameName": "Longcat",
                    "tagLine": "KR1",
                    "profileIconId": 7,
                    "isMain": True,
                },
                {
                    "platform": "KR",
                    "gameName": "LongAlt",
                    "tagLine": "KR2",
                    "profileIconId": 8,
                    "isMain": False,
                },
            ],
        }
        assert not any(
            field in response.text
            for field in (
                '"userId"',
                '"discordId"',
                '"puuid"',
                '"routingRegion"',
                '"showRiotIds"',
                '"normalized',
            )
        )
    head = await runtime_client.head(PATH, params={"q": PLAYER})
    assert head.status_code == 200 and not head.content


@pytest.mark.parametrize("privacy", ["hidden", "missing"])
async def test_riot_link_privacy_and_public_profile_defaults(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, privacy: str
) -> None:
    await setup(owner, runtime_client)
    async with owner.begin() as c:
        if privacy == "hidden":
            await c.execute(
                text(
                    "UPDATE user_privacy_settings SET show_riot_ids=false, "
                    "public_profile_enabled=false WHERE user_id=:user"
                ),
                {"user": PLAYER},
            )
        else:
            await c.execute(
                text("DELETE FROM user_privacy_settings WHERE user_id=:user"), {"user": PLAYER}
            )
    response = await runtime_client.get(PATH, params={"q": PLAYER})
    data = response.json()["data"]
    assert data["riotAccounts"] == []
    assert data["member"]["profileHref"] == (None if privacy == "hidden" else f"/users/{PLAYER}")
    assert data["championPreferences"]["MID"] == ["Ahri", "Orianna"]
    assert_failure(
        await runtime_client.get(PATH, params={"q": "Longcat#KR1"}),
        404,
        "not_found",
        "요청한 길드 멤버를 찾을 수 없습니다.",
    )


async def test_ambiguous_display_or_riot_identity_and_id_precedence(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    await setup(owner, runtime_client)
    async with owner.begin() as c:
        await c.execute(
            text("UPDATE guild_members SET display_name='longcat' WHERE user_id=:user"),
            {"user": OTHER},
        )
    assert_failure(
        await runtime_client.get(PATH, params={"q": "Longcat"}),
        409,
        "ambiguous_guild_member",
        "같은 이름의 길드 멤버가 여러 명입니다.",
    )
    assert (await runtime_client.get(PATH, params={"q": PLAYER})).status_code == 200
    async with owner.begin() as c:
        await c.execute(
            text("UPDATE guild_members SET display_name='Longcat#KR1' WHERE user_id=:user"),
            {"user": OTHER},
        )
    assert_failure(
        await runtime_client.get(PATH, params={"q": "Longcat#KR1"}),
        409,
        "ambiguous_guild_member",
        "같은 이름의 길드 멤버가 여러 명입니다.",
    )
    async with owner.begin() as c:
        await c.execute(
            text("UPDATE user_privacy_settings SET show_riot_ids=false WHERE user_id=:user"),
            {"user": PLAYER},
        )
    # A literal display-name match remains accessible without publishing Riot links.
    response = await runtime_client.get(PATH, params={"q": "Longcat#KR1"})
    assert response.status_code == 200 and response.json()["data"]["riotAccounts"] == []
    async with owner.begin() as c:
        await c.execute(
            text("UPDATE guild_members SET display_name='999999999999999999' WHERE user_id=:user"),
            {"user": OTHER},
        )
    assert_failure(
        await runtime_client.get(PATH, params={"q": "999999999999999999"}),
        404,
        "not_found",
        "요청한 길드 멤버를 찾을 수 없습니다.",
    )


async def test_dummy_member_has_no_public_profile_link_and_unsafe_avatar_is_hidden(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    await setup(owner, runtime_client)
    async with owner.begin() as c:
        await c.execute(
            text("UPDATE users SET avatar_hash='../../secret' WHERE user_id=:user"), {"user": DUMMY}
        )
    response = await runtime_client.get(PATH, params={"q": DUMMY})
    assert response.status_code == 200
    assert response.json()["data"] == {
        "member": {
            "displayName": "Dummy",
            "avatarUrl": None,
            "mainLane": "MID",
            "profileHref": None,
        },
        "championPreferences": {lane: [] for lane in ("TOP", "JUNGLE", "MID", "BOTTOM", "SUPPORT")},
        "riotAccounts": [],
    }


@pytest.mark.parametrize(
    "mode",
    [
        "inactive_viewer",
        "disabled_guild",
        "other_guild",
        "inactive_target",
        "deleted_target",
        "missing_target",
    ],
)
async def test_profile_tenant_and_active_target_boundaries(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, mode: str
) -> None:
    await setup(owner, runtime_client)
    path, query = PATH, PLAYER
    async with owner.begin() as c:
        if mode == "inactive_viewer":
            await c.execute(
                text("UPDATE guild_members SET status='INACTIVE' WHERE user_id=:user"),
                {"user": USER_ID},
            )
        elif mode == "disabled_guild":
            await c.execute(
                text("UPDATE guilds SET status='DISABLED' WHERE guild_id=:guild"),
                {"guild": GUILD_A},
            )
        elif mode == "other_guild":
            path = f"/api/guilds/{GUILD_B}/records/profile"
        elif mode == "inactive_target":
            await c.execute(
                text("UPDATE guild_members SET status='INACTIVE' WHERE user_id=:user"),
                {"user": PLAYER},
            )
        elif mode == "deleted_target":
            await c.execute(
                text("UPDATE users SET deleted_at=now() WHERE user_id=:user"), {"user": PLAYER}
            )
        else:
            query = "Missing"
    assert_failure(
        await runtime_client.get(path, params={"q": query}),
        404,
        "not_found",
        "요청한 길드 멤버를 찾을 수 없습니다.",
    )


async def test_profile_auth_precedes_query_and_unicode_length_validation(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    assert_failure(await runtime_client.get(PATH), 401, "unauthorized", "인증이 필요합니다.")
    assert_failure(
        await runtime_client.get("/api/guilds/invalid/records/profile"),
        400,
        "bad_request",
        "요청 형식이 올바르지 않습니다.",
    )
    await setup(owner, runtime_client)
    for suffix in ("", "?q=x&q=y", "?q=x&extra=1"):
        assert_failure(
            await runtime_client.get(PATH + suffix),
            400,
            "bad_request",
            "요청 형식이 올바르지 않습니다.",
        )
    for query in ("", " \u2003 ", "가" * 101):
        response = await runtime_client.get(PATH, params={"q": query})
        assert_failure(response, 400, "invalid_search_query", "검색어가 올바르지 않습니다.")
        assert response.json()["error"]["details"] == {"maxLength": 100}
    for query in ("가", "가" * 100, "\x1c"):
        assert_failure(
            await runtime_client.get(PATH, params={"q": query}),
            404,
            "not_found",
            "요청한 길드 멤버를 찾을 수 없습니다.",
        )


async def test_profile_rechecks_riot_publication_after_identity_resolution(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, monkeypatch: pytest.MonkeyPatch
) -> None:
    await setup(owner, runtime_client)
    original = repository.visible_accounts

    async def revoke_before_accounts(connection, user):
        async with owner.begin() as writer:
            await writer.execute(
                text("UPDATE user_privacy_settings SET show_riot_ids=false WHERE user_id=:user"),
                {"user": user},
            )
        return await original(connection, user)

    monkeypatch.setattr(repository, "visible_accounts", revoke_before_accounts)
    assert_failure(
        await runtime_client.get(PATH, params={"q": "Longcat#KR1"}),
        404,
        "not_found",
        "요청한 길드 멤버를 찾을 수 없습니다.",
    )


async def test_preference_gap_returns_safe_error_with_request_id(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, caplog: pytest.LogCaptureFixture
) -> None:
    await setup(owner, runtime_client)
    async with owner.begin() as c:
        await c.execute(
            text(
                "DELETE FROM user_champion_preferences WHERE user_id=:user "
                "AND lane='MID' AND position=1"
            ),
            {"user": PLAYER},
        )
    with caplog.at_level(logging.ERROR):
        response = await runtime_client.get(PATH, params={"q": PLAYER})
    assert_failure(response, 500, "internal_error", "요청을 처리하는 중 내부 오류가 발생했습니다.")
    assert any(
        record.message == "guild_record_profile_invariant_failed"
        and record.request_id == response.headers["x-request-id"]
        for record in caplog.records
    )
