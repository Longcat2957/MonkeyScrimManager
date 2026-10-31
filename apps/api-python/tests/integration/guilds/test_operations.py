import uuid

import httpx
import pytest
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncEngine

from tests.support.assertions import assert_failure
from tests.support.guilds.context import GUILD_A, GUILD_B, USER_ID, authenticated_setup
from tests.support.guilds.operations import (
    PATH,
    auction_fixture,
    game_fixture,
    match_fixture,
    series_fixture,
)
from tests.support.recruitments.queries import recruitment_fixture

pytestmark = pytest.mark.postgres


async def test_operations_auth_empty_head_and_unused_query(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    assert_failure(await runtime_client.get(PATH), 401, "unauthorized", "인증이 필요합니다.")
    assert_failure(
        await runtime_client.get("/api/guilds/invalid/operations"),
        400,
        "bad_request",
        "요청 형식이 올바르지 않습니다.",
    )
    await authenticated_setup(owner, runtime_client)
    response = await runtime_client.get(PATH + "?limit=bad&limit=0&unknown=1")
    assert response.status_code == 200
    assert response.json()["data"] == {"operations": []}
    head = await runtime_client.head(PATH)
    assert head.status_code == 200 and not head.content


@pytest.mark.parametrize("mode", ["inactive_member", "disabled_guild", "other_guild"])
async def test_operations_require_active_membership(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, mode: str
) -> None:
    await authenticated_setup(owner, runtime_client)
    await recruitment_fixture(owner, 1, "CLOSED")
    path = PATH
    async with owner.begin() as connection:
        if mode == "inactive_member":
            await connection.execute(text("UPDATE guild_members SET status = 'INACTIVE'"))
        elif mode == "disabled_guild":
            await connection.execute(text("UPDATE guilds SET status = 'DISABLED'"))
        else:
            path = f"/api/guilds/{GUILD_B}/operations"
    assert_failure(
        await runtime_client.get(path), 404, "not_found", "요청한 길드를 찾을 수 없습니다."
    )


async def test_entry_counts_only_joined_participants_and_assignments(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    await authenticated_setup(owner, runtime_client)
    record = await recruitment_fixture(owner, 1, "CLOSED")
    for number, status in [(2, "OPEN"), (3, "CANCELLED"), (4, "CONVERTED")]:
        await recruitment_fixture(owner, number, status)
    async with owner.begin() as connection:
        params = {"guild": GUILD_A, "id": record, "user": USER_ID}
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
            INSERT INTO recruitment_entry_drafts (guild_id, recruitment_id, revision, updated_at)
            VALUES (:guild, :id, 0, now())
        """),
            params,
        )
        await connection.execute(
            text("""
            INSERT INTO recruitment_entry_assignments
                (guild_id, recruitment_id, user_id, team, lane)
            VALUES (:guild, :id, :user, 'TEAM_1', 'MID')
        """),
            params,
        )
    rows = (await runtime_client.get(PATH)).json()["data"]["operations"]
    assert rows == [
        {
            "id": f"entry:{record}",
            "kind": "ENTRY",
            "title": "일반 모집",
            "status": "편성 필요",
            "detail": "1/10명 · 0명 미배정",
            "updatedAt": "2026-09-13T01:02:03.123456Z",
            "href": f"/g/{GUILD_A}/recruitments/{record}/entry",
            "priority": "urgent",
        }
    ]
    async with owner.begin() as connection:
        await connection.execute(text("UPDATE recruitment_participants SET status = 'WITHDRAWN'"))
    assert (await runtime_client.get(PATH)).json()["data"]["operations"][0]["detail"] == (
        "0/10명 · 0명 미배정"
    )


@pytest.mark.parametrize("ending", ["cancelled", "deleted", "completed", "converted"])
async def test_series_hides_source_and_only_active_series_remain(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient, ending: str
) -> None:
    await authenticated_setup(owner, runtime_client)
    record = await recruitment_fixture(owner, 1, "CLOSED")
    series = await series_fixture(owner, record, best_of=1)
    await game_fixture(owner, series, 1)
    await game_fixture(owner, series, 2, voided=True)
    rows = (await runtime_client.get(PATH)).json()["data"]["operations"]
    assert len(rows) == 1
    assert rows[0]["id"] == f"series:{series}"
    assert rows[0]["kind"] == "PICK_BAN"
    assert rows[0]["status"] == "픽밴/결과 입력"
    assert rows[0]["detail"] == "1세트 완료 · 완료 처리 대기"
    assert rows[0]["href"] == f"/g/{GUILD_A}/series/{series}/pick-ban"
    async with owner.begin() as connection:
        if ending == "completed":
            await connection.execute(
                text(
                    "UPDATE series SET status = 'COMPLETED', "
                    "terminal_reason = 'EARLY_DRAW', ended_at = now()"
                )
            )
        elif ending == "deleted":
            await connection.execute(text("UPDATE series SET deleted_at = now()"))
        else:
            await connection.execute(
                text(
                    "UPDATE series SET status = 'CANCELLED', "
                    "terminal_reason = 'CANCELLED', ended_at = now()"
                )
            )
            if ending == "converted":
                await connection.execute(text("UPDATE recruitments SET converted_at = now()"))
    rows = (await runtime_client.get(PATH)).json()["data"]["operations"]
    assert [row["id"] for row in rows] == (
        [f"entry:{record}"] if ending in {"cancelled", "deleted"} else []
    )


async def test_mixed_priority_timestamp_kind_and_id_order_with_auction_formats(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    await authenticated_setup(owner, runtime_client)
    first = await recruitment_fixture(owner, 1, "CLOSED")
    second = await recruitment_fixture(owner, 2, "CLOSED")
    series = await series_fixture(owner, await recruitment_fixture(owner, 3, "CLOSED"))
    ten, _ = await auction_fixture(owner, 10)
    twenty, _ = await auction_fixture(owner, 20, twenty=True)
    rows = (await runtime_client.get(PATH)).json()["data"]["operations"]
    assert [row["id"] for row in rows] == [
        f"entry:{second}",
        f"entry:{first}",
        f"series:{series}",
        f"auction-recruitment:{twenty}",
        f"auction-recruitment:{ten}",
    ]
    assert rows[2]["detail"] == "0세트 완료 · 1세트 입력 대기"
    assert rows[3]["detail"] == "1/20명 · 4팀"
    assert rows[4]["detail"] == "1/10명 · 2팀"
    assert rows[4]["href"] == f"/g/{GUILD_A}/auction/recruitments/{ten}/draft"
    assert [row["priority"] for row in rows] == ["urgent"] * 3 + ["normal"] * 2
    async with owner.begin() as connection:
        await connection.execute(
            text("UPDATE auction_recruitment_participants SET status='WITHDRAWN'")
        )
        await connection.execute(
            text("UPDATE auction_recruitments SET converted_at = now() WHERE id = :id"),
            {"id": twenty},
        )
    rows = (await runtime_client.get(PATH)).json()["data"]["operations"]
    assert len(rows) == 4 and rows[-1]["detail"] == "0/10명 · 2팀"


async def test_auction_tournament_stages_next_match_and_exclusions(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    await authenticated_setup(owner, runtime_client)
    recruitment, tournament = await auction_fixture(owner, 1, tournament_status="BIDDING")
    assert tournament is not None
    base = f"/g/{GUILD_A}/auction/tournaments/{tournament}"
    rows = (await runtime_client.get(PATH)).json()["data"]["operations"]
    assert len(rows) == 1 and rows[0]["id"] == f"auction-tournament:{tournament}"
    assert (rows[0]["status"], rows[0]["detail"], rows[0]["href"]) == (
        "입찰 진행",
        "0팀 · 0명 편성",
        base + "/draft",
    )
    async with owner.begin() as connection:
        await connection.execute(text("UPDATE auction_tournaments SET status = 'BRACKET_SETUP'"))
    row = (await runtime_client.get(PATH)).json()["data"]["operations"][0]
    assert (row["status"], row["detail"], row["href"]) == (
        "대진 설정",
        "0팀 · 대진 확정 대기",
        base + "/bracket",
    )
    async with owner.begin() as connection:
        await connection.execute(text("UPDATE auction_tournaments SET status = 'IN_GAME'"))
    row = (await runtime_client.get(PATH)).json()["data"]["operations"][0]
    assert (row["status"], row["detail"], row["href"]) == (
        "대진 진행",
        "0경기 진행 또는 대기",
        base + "/bracket",
    )
    semifinal = await match_fixture(owner, tournament, "SEMIFINAL_A", "READY")
    await match_fixture(owner, tournament, "SEMIFINAL_B", "READY")
    final = await match_fixture(owner, tournament, "FINAL", "IN_PROGRESS")
    await match_fixture(owner, tournament, "FINAL", "IN_PROGRESS", deleted=True)
    await match_fixture(owner, tournament, "FINAL", "CANCELLED")
    row = (await runtime_client.get(PATH)).json()["data"]["operations"][0]
    assert (row["status"], row["detail"], row["href"]) == (
        "결과 입력",
        "3경기 진행 또는 대기",
        f"/g/{GUILD_A}/auction/matches/{final}",
    )
    async with owner.begin() as connection:
        await connection.execute(
            text("UPDATE auction_matches SET status = 'READY' WHERE id = :id"), {"id": final}
        )
    row = (await runtime_client.get(PATH)).json()["data"]["operations"][0]
    assert row["href"] == f"/g/{GUILD_A}/auction/matches/{semifinal}"
    async with owner.begin() as connection:
        await connection.execute(
            text(
                "UPDATE auction_matches SET status = 'LINEUP_PENDING', "
                "lineups_locked_at = NULL, lineups_locked_by = NULL WHERE status = 'READY'"
            )
        )
    row = (await runtime_client.get(PATH)).json()["data"]["operations"][0]
    assert row["status"] == "대진 진행" and row["href"] == base + "/bracket"
    async with owner.begin() as connection:
        await connection.execute(text("UPDATE auction_tournaments SET deleted_at = now()"))
    rows = (await runtime_client.get(PATH)).json()["data"]["operations"]
    assert [row["id"] for row in rows] == [f"auction-recruitment:{recruitment}"]
    async with owner.begin() as connection:
        await connection.execute(
            text(
                "UPDATE auction_tournaments SET deleted_at = NULL, "
                "status = 'CANCELLED', ended_at = now()"
            )
        )
    assert (await runtime_client.get(PATH)).json()["data"]["operations"] == rows


async def test_operation_limit_is_100_with_stable_id_ties(
    owner: AsyncEngine, runtime_client: httpx.AsyncClient
) -> None:
    await authenticated_setup(owner, runtime_client)
    # A single bulk insert is enough to exercise the fixed server-side cap.
    async with owner.begin() as connection:
        await connection.execute(
            text("""
            INSERT INTO recruitments
                (guild_id, id, season_id, display_number, title, status, revision,
                 created_by, created_at, updated_at, best_of, hard_fearless)
            SELECT :guild, :id, id, :number, '일반 모집', 'CLOSED', 0, :user, now(),
                   '2026-09-13T01:02:03Z', 3, true
            FROM seasons WHERE guild_id = :guild AND status = 'ACTIVE'
        """),
            [
                {"guild": GUILD_A, "id": uuid.UUID(int=number), "number": number, "user": USER_ID}
                for number in range(1, 102)
            ],
        )
    rows = (await runtime_client.get(PATH)).json()["data"]["operations"]
    assert len(rows) == 100
    assert rows[0]["id"] == f"entry:{uuid.UUID(int=101)}"
    assert rows[-1]["id"] == f"entry:{uuid.UUID(int=2)}"
