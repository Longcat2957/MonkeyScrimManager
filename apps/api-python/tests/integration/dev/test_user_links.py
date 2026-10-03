"""Dummy-user linked resources preserve snapshots, revisions, and replay."""

import uuid

import httpx
import pytest
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncEngine

from tests.support.assertions import assert_failure
from tests.support.auth.session import TOKEN, USER_ID, seed_session
from tests.support.guilds.context import GUILD_A, GUILD_B, ROLE_A, guild_fixture

pytestmark = pytest.mark.postgres
CSRF = {
    "cookie": f"monkeybot_session={TOKEN}; monkeybot_csrf=integration-csrf",
    "origin": "http://localhost:5173",
    "x-csrf-token": "integration-csrf",
}
MMR = {lane: 1500.0 for lane in ("top", "jungle", "mid", "bottom", "support")}


def headers(key: str) -> dict[str, str]:
    return {**CSRF, "idempotency-key": key}


async def setup(owner: AsyncEngine, *, season: bool = True) -> str:
    await seed_session(owner)
    if season:
        await guild_fixture(owner, GUILD_A, "Alpha", ROLE_A)
    else:
        async with owner.begin() as c:
            await c.execute(
                text("""
                INSERT INTO guilds (guild_id, discord_name, status, installed_at, updated_at)
                VALUES (:guild, 'No season', 'ACTIVE', now(), now())
            """),
                {"guild": GUILD_B},
            )
    user = f"dmy_{uuid.uuid7()}"
    async with owner.begin() as c:
        await c.execute(
            text("""
            INSERT INTO users (user_id, identity_kind, display_name, created_at, updated_at)
            VALUES (:user, 'DUMMY', 'Link dummy', now(), now())
        """),
            {"user": user},
        )
        await c.execute(
            text("""
            INSERT INTO dummy_user_profiles
              (user_id, stable_key, state, revision, created_by, updated_by, created_at, updated_at)
            VALUES (:user, :key, 'ACTIVE', 0, :actor, :actor, now(), now())
        """),
            {"user": user, "key": f"link_{uuid.uuid7().hex}", "actor": USER_ID},
        )
    return user


async def test_guild_membership_upsert_remove_and_replay(
    owner: AsyncEngine, dummy_enabled_client: httpx.AsyncClient
) -> None:
    user = await setup(owner)
    path = f"/api/dev/dummy-users/{user}/guilds/{GUILD_A}"
    body = {
        "displayName": "길드 더미",
        "status": "ACTIVE",
        "mainLane": "TOP",
        "laneMmr": {**MMR, "top": 1612.0},
    }
    created = await dummy_enabled_client.put(path, json=body, headers=headers("link-member-1"))
    assert created.status_code == 200, created.text
    snapshot = created.json()["data"]
    assert snapshot["revision"] == 0
    assert snapshot["guildMemberships"][0]["laneMmr"] == body["laneMmr"]
    replay = await dummy_enabled_client.put(path, json=body, headers=headers("link-member-1"))
    assert replay.status_code == 200 and replay.json()["data"] == snapshot
    assert_failure(
        await dummy_enabled_client.put(
            path, json={**body, "displayName": "Other"}, headers=headers("link-member-1")
        ),
        409,
        "idempotency_conflict",
        "같은 Idempotency-Key가 다른 요청에 사용되었습니다.",
    )
    updated = await dummy_enabled_client.put(
        path,
        json={"displayName": "Inactive", "status": "INACTIVE"},
        headers=headers("link-member-2"),
    )
    assert updated.status_code == 200, updated.text
    assert updated.json()["data"]["guildMemberships"][0]["status"] == "INACTIVE"
    assert updated.json()["data"]["revision"] == 0
    removed = await dummy_enabled_client.delete(path, headers=headers("link-member-3"))
    assert removed.status_code == 200, removed.text
    assert removed.json()["data"]["guildMemberships"] == []
    replay_removed = await dummy_enabled_client.delete(path, headers=headers("link-member-3"))
    assert replay_removed.status_code == 200
    assert replay_removed.json()["data"] == removed.json()["data"]
    async with owner.connect() as c:
        assert (
            await c.scalar(
                text("""
            SELECT revision FROM guild_admin_state
            WHERE guild_id=:guild AND scope='MEMBERS'
        """),
                {"guild": GUILD_A},
            )
            == 3
        )
        assert (
            await c.scalar(
                text("SELECT count(*) FROM user_lane_mmr WHERE user_id=:user"), {"user": user}
            )
            == 0
        )
        actions = (
            (
                await c.execute(
                    text("""
            SELECT action FROM dummy_user_audit_log ORDER BY created_at, id
        """)
                )
            )
            .scalars()
            .all()
        )
    assert actions == ["DUMMY_MEMBER_ATTACHED", "DUMMY_MEMBER_UPDATED", "DUMMY_MEMBER_DETACHED"]


async def test_champion_and_riot_links_preserve_order_and_revision(
    owner: AsyncEngine, dummy_enabled_client: httpx.AsyncClient
) -> None:
    user = await setup(owner)
    base = f"/api/dev/dummy-users/{user}"
    champion = await dummy_enabled_client.put(
        base + "/champion-preferences/TOP",
        json={"championIds": ["Aatrox", "LeeSin"]},
        headers=headers("link-champion-1"),
    )
    assert champion.status_code == 200, champion.text
    assert champion.json()["data"]["championPreferences"]["TOP"] == ["Aatrox", "LeeSin"]
    assert champion.json()["data"]["revision"] == 1
    assert_failure(
        await dummy_enabled_client.put(
            base + "/champion-preferences/TOP",
            json={"championIds": ["Aatrox", "Aatrox"]},
            headers=headers("link-champion-2"),
        ),
        422,
        "invalid_dummy_user_input",
        "더미 사용자 입력값을 확인해 주세요.",
    )
    assert_failure(
        await dummy_enabled_client.put(
            base + "/champion-preferences/NOPE",
            json={"championIds": []},
            headers=headers("link-champion-3"),
        ),
        422,
        "invalid_dummy_user_input",
        "더미 사용자 입력값을 확인해 주세요.",
    )
    riot_path = base + "/riot-account"
    riot = {
        "platform": "KR",
        "routingRegion": "ASIA",
        "gameName": "Dummy Top",
        "tagLine": "TEST",
        "profileIconId": 29,
        "mainPosition": "TOP",
    }
    created = await dummy_enabled_client.put(riot_path, json=riot, headers=headers("link-riot-1"))
    assert created.status_code == 200, created.text
    assert created.json()["data"]["riotAccount"] == riot
    assert created.json()["data"]["revision"] == 2
    async with owner.connect() as c:
        puuid = await c.scalar(
            text("SELECT puuid FROM riot_accounts WHERE user_id=:user"), {"user": user}
        )
        assert puuid.startswith("dummy:")
    changed = await dummy_enabled_client.put(
        riot_path, json={**riot, "gameName": "Second Dummy"}, headers=headers("link-riot-2")
    )
    assert changed.status_code == 200, changed.text
    assert changed.json()["data"]["revision"] == 3
    async with owner.connect() as c:
        assert (
            await c.scalar(
                text("SELECT puuid FROM riot_accounts WHERE user_id=:user"), {"user": user}
            )
            == puuid
        )
    removed = await dummy_enabled_client.delete(riot_path, headers=headers("link-riot-3"))
    assert removed.status_code == 200, removed.text
    assert removed.json()["data"]["riotAccount"] is None
    assert removed.json()["data"]["revision"] == 4
    replay = await dummy_enabled_client.delete(riot_path, headers=headers("link-riot-3"))
    assert replay.status_code == 200 and replay.json()["data"] == removed.json()["data"]


async def test_link_errors_and_transaction_rollback(
    owner: AsyncEngine, dummy_enabled_client: httpx.AsyncClient
) -> None:
    user = await setup(owner, season=False)
    path = f"/api/dev/dummy-users/{user}/guilds/{GUILD_B}"
    response = await dummy_enabled_client.put(
        path,
        json={"displayName": "No season", "status": "ACTIVE", "laneMmr": MMR},
        headers=headers("link-no-season"),
    )
    assert_failure(
        response,
        409,
        "active_season_required",
        "초기 MMR을 저장하려면 길드의 활성 시즌이 필요합니다.",
    )
    async with owner.connect() as c:
        assert (
            await c.scalar(
                text("SELECT count(*) FROM guild_members WHERE user_id=:user"), {"user": user}
            )
            == 0
        )
    response = await dummy_enabled_client.delete(path, headers=headers("link-no-member"))
    assert_failure(
        response, 404, "dummy_membership_not_found", "해당 길드의 더미 회원을 찾을 수 없습니다."
    )
    response = await dummy_enabled_client.delete(
        f"/api/dev/dummy-users/{user}/riot-account", headers=headers("link-no-riot")
    )
    assert_failure(
        response, 404, "dummy_riot_account_not_found", "합성 Riot 계정을 찾을 수 없습니다."
    )
    async with owner.begin() as c:
        await c.execute(
            text("UPDATE dummy_user_profiles SET state='ARCHIVED' WHERE user_id=:user"),
            {"user": user},
        )
    response = await dummy_enabled_client.put(
        f"/api/dev/dummy-users/{user}/champion-preferences/TOP",
        json={"championIds": ["Aatrox"]},
        headers=headers("link-archived"),
    )
    assert_failure(
        response,
        409,
        "dummy_user_archived",
        "보관된 더미 사용자는 복원한 뒤 수정하거나 배치할 수 있습니다.",
    )


async def test_membership_with_retained_mmr_history_must_be_inactivated(
    owner: AsyncEngine, dummy_enabled_client: httpx.AsyncClient
) -> None:
    user = await setup(owner)
    path = f"/api/dev/dummy-users/{user}/guilds/{GUILD_A}"
    attached = await dummy_enabled_client.put(
        path,
        json={"displayName": "History", "status": "ACTIVE", "laneMmr": MMR},
        headers=headers("link-history-create"),
    )
    assert attached.status_code == 200, attached.text
    async with owner.begin() as c:
        await c.execute(
            text("""
            INSERT INTO mmr_ledger_entries
              (guild_id, id, season_id, user_id, lane, lane_revision,
               source_type, source_id, mmr_before, mmr_after, delta,
               games_played_delta, wins_delta, actor_type, actor_id, note, request_id, created_at)
            SELECT guild_id, :entry, season_id, user_id, lane, revision+1,
                   'MANUAL_ADJUSTMENT', :entry, mmr, mmr+1, 1,
                   0, 0, 'USER', :actor, 'Test history', 'retained-history', now()
            FROM user_lane_mmr WHERE guild_id=:guild AND user_id=:user AND lane='TOP'
        """),
            {"entry": uuid.uuid7(), "actor": USER_ID, "guild": GUILD_A, "user": user},
        )
    response = await dummy_enabled_client.delete(path, headers=headers("link-history-delete"))
    assert_failure(
        response,
        409,
        "dummy_membership_has_history",
        "내전 이력이 있는 길드 소속은 제거할 수 없습니다. 비활성 상태로 변경해 주세요.",
    )
    inactive = await dummy_enabled_client.put(
        path,
        json={"displayName": "History", "status": "INACTIVE"},
        headers=headers("link-history-inactive"),
    )
    assert inactive.status_code == 200, inactive.text
    assert inactive.json()["data"]["guildMemberships"][0]["status"] == "INACTIVE"
