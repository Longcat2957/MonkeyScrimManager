import asyncio

import httpx
import pytest
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncEngine

from tests.support.assertions import assert_failure
from tests.support.guilds.context import (
    GUILD_A,
    GUILD_B,
    ROLE_B,
    USER_ID,
    authenticated_setup,
    guild_fixture,
)
from tests.support.users.profile import CHAMPIONS, DEFAULTS, PRIVACY, PROFILE, csrf

pytestmark = pytest.mark.postgres


async def test_privacy_defaults_upsert_and_public_profile_visibility(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    material = await authenticated_setup(owner, runtime_client)
    async with owner.begin() as connection:
        await connection.execute(text("DELETE FROM user_privacy_settings"))
    assert (await runtime_client.get(PRIVACY)).json()["data"] == {"settings": DEFAULTS}
    async with owner.connect() as connection:
        assert await connection.scalar(text("SELECT count(*) FROM user_privacy_settings")) == 0
    # Public endpoint works without cookies and exposes only the documented projection.
    public = await runtime_client.get(PROFILE, headers={"cookie": ""})
    assert public.status_code == 200
    assert set(public.json()["data"]) == {
        "discordId",
        "displayName",
        "activeGuildCount",
        "seasonCount",
    }
    assert public.json()["data"]["activeGuildCount"] == 1
    assert public.json()["data"]["seasonCount"] == 1
    hidden = {"publicProfile": False, "showRiotIds": False, "allowExternalRecordSearch": True}
    response = await runtime_client.put(PRIVACY, json=hidden, headers=csrf(material))
    assert response.status_code == 200 and response.json()["data"] == {"settings": hidden}
    assert (await runtime_client.get(PRIVACY)).json()["data"] == {"settings": hidden}
    for headers in ({}, {"cookie": ""}):
        assert_failure(
            await runtime_client.get(PROFILE, headers=headers),
            404,
            "not_found",
            "요청한 사용자 프로필을 찾을 수 없습니다.",
        )
    assert (
        await runtime_client.put(PRIVACY, json=DEFAULTS, headers=csrf(material))
    ).status_code == 200
    assert (await runtime_client.head(PROFILE)).status_code == 200


async def test_public_profile_counts_only_active_memberships_and_guilds(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    await authenticated_setup(owner, runtime_client)
    await guild_fixture(owner, GUILD_B, "Beta", ROLE_B)
    assert (await runtime_client.get(PROFILE)).json()["data"]["seasonCount"] == 2
    async with owner.begin() as connection:
        await connection.execute(
            text("UPDATE guilds SET status = 'DISABLED' WHERE guild_id = :id"), {"id": GUILD_B}
        )
        await connection.execute(
            text("UPDATE guild_members SET status = 'INACTIVE' WHERE guild_id = :id"),
            {"id": GUILD_A},
        )
    data = (await runtime_client.get(PROFILE)).json()["data"]
    assert data["activeGuildCount"] == data["seasonCount"] == 0
    async with owner.begin() as connection:
        await connection.execute(text("UPDATE users SET deleted_at = now()"))
    assert_failure(
        await runtime_client.get(PROFILE),
        404,
        "not_found",
        "요청한 사용자 프로필을 찾을 수 없습니다.",
    )


@pytest.mark.parametrize(
    "path,method,body",
    [
        (PRIVACY, "GET", None),
        (PRIVACY, "PUT", DEFAULTS),
        (CHAMPIONS, "GET", None),
        (CHAMPIONS + "/MID", "PUT", {"championIds": ["Ahri"]}),
    ],
)
async def test_self_service_requires_session(
    runtime_client: httpx.AsyncClient, path: str, method: str, body: object
) -> None:
    assert_failure(
        await runtime_client.request(method, path, json=body),
        401,
        "unauthorized",
        "인증이 필요합니다.",
    )


@pytest.mark.parametrize(
    "body",
    [
        {},
        {**DEFAULTS, "publicProfile": "false"},
        {**DEFAULTS, "publicProfile": None},
        {**DEFAULTS, "extra": True},
        {**DEFAULTS, "showRiotIds": 1},
    ],
)
async def test_privacy_rejects_non_strict_or_partial_json(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, body: object
) -> None:
    material = await authenticated_setup(owner, runtime_client)
    assert_failure(
        await runtime_client.put(PRIVACY, json=body, headers=csrf(material)),
        422,
        "unprocessable_entity",
        "요청 JSON의 필드 또는 값이 API 계약과 일치하지 않습니다.",
    )
    assert (await runtime_client.get(PRIVACY)).json()["data"] == {"settings": DEFAULTS}


async def test_csrf_denial_preserves_both_settings(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    await authenticated_setup(owner, runtime_client)
    for path, body in (
        (PRIVACY, {**DEFAULTS, "publicProfile": False}),
        (CHAMPIONS + "/MID", {"championIds": ["Ahri"]}),
    ):
        assert_failure(
            await runtime_client.put(path, json=body),
            403,
            "csrf_validation_failed",
            "요청 출처를 확인할 수 없습니다.",
        )
    assert (await runtime_client.get(PRIVACY)).json()["data"] == {"settings": DEFAULTS}
    assert all(
        not ids
        for ids in (await runtime_client.get(CHAMPIONS)).json()["data"]["preferences"].values()
    )


async def test_champions_order_replace_clear_and_concurrent_lanes(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    material = await authenticated_setup(owner, runtime_client)

    async def update(lane: str, ids: list[str]) -> httpx.Response:
        return await runtime_client.put(
            CHAMPIONS + "/" + lane, json={"championIds": ids}, headers=csrf(material)
        )

    results = await asyncio.gather(update("MID", ["Ahri", "Orianna"]), update("TOP", ["Gnar"]))
    assert all(response.status_code == 200 for response in results)
    snapshot = (await runtime_client.get(CHAMPIONS)).json()["data"]["preferences"]
    assert snapshot == {
        "TOP": ["Gnar"],
        "JUNGLE": [],
        "MID": ["Ahri", "Orianna"],
        "BOTTOM": [],
        "SUPPORT": [],
    }
    results = await asyncio.gather(update("MID", ["Ahri", "Syndra"]), update("MID", ["Orianna"]))
    assert all(response.status_code == 200 for response in results)
    assert (await runtime_client.get(CHAMPIONS)).json()["data"]["preferences"]["MID"] in (
        ["Ahri", "Syndra"],
        ["Orianna"],
    )
    snapshot = (await update("MID", [])).json()["data"]["preferences"]
    assert snapshot["MID"] == [] and snapshot["TOP"] == ["Gnar"]


@pytest.mark.parametrize(
    "lane,ids,status,code,message",
    [
        ("mid", [], 400, "invalid_lane", "지원하지 않는 포지션입니다."),
        (
            "MID",
            ["Ahri"] * 11,
            422,
            "too_many_champion_preferences",
            "한 포지션에는 선호 챔피언을 최대 10개까지 저장할 수 있습니다.",
        ),
        (
            "MID",
            ["Ahri", "Ahri"],
            422,
            "duplicate_champion_id",
            "같은 챔피언을 한 포지션에 중복해서 저장할 수 없습니다.",
        ),
        ("MID", ["아리"], 422, "invalid_champion_id", "챔피언 ID 형식이 올바르지 않습니다."),
        ("MID", ["A" * 65], 422, "invalid_champion_id", "챔피언 ID 형식이 올바르지 않습니다."),
        ("MID", [""], 422, "invalid_champion_id", "챔피언 ID 형식이 올바르지 않습니다."),
    ],
)
async def test_invalid_preferences_explain_failure_without_mutation(
    owner: AsyncEngine,
    runtime_client: httpx.AsyncClient,
    lane: str,
    ids: list[str],
    status: int,
    code: str,
    message: str,
) -> None:
    material = await authenticated_setup(owner, runtime_client)
    response = await runtime_client.put(
        CHAMPIONS + "/" + lane, json={"championIds": ids}, headers=csrf(material)
    )
    assert_failure(response, status, code, message)
    async with owner.connect() as connection:
        assert await connection.scalar(text("SELECT count(*) FROM user_champion_preferences")) == 0


async def test_invalid_snapshot_rolls_back_lane_replacement(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    material = await authenticated_setup(owner, runtime_client)
    async with owner.begin() as connection:
        await connection.execute(
            text(
                "INSERT INTO user_champion_preferences "
                "(user_id, lane, position, champion_id) VALUES "
                "(:id, 'TOP', 2, 'Gnar'), (:id, 'MID', 1, 'Ahri')"
            ),
            {"id": USER_ID},
        )
    response = await runtime_client.put(
        CHAMPIONS + "/MID", json={"championIds": ["Orianna"]}, headers=csrf(material)
    )
    assert response.status_code == 500
    async with owner.connect() as connection:
        assert (
            await connection.scalar(
                text("SELECT champion_id FROM user_champion_preferences WHERE lane = 'MID'")
            )
            == "Ahri"
        )
