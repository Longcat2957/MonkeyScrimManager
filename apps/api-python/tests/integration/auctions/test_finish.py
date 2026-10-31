import asyncio
import uuid

import pytest
from sqlalchemy import text

from tests.support.assertions import assert_failure
from tests.support.auctions.bidding import PATH
from tests.support.auctions.cancellation import effects, tournament
from tests.support.auctions.finish import finish, later_mmr_checkpoint, preserved, scenario
from tests.support.auctions.games import PATH as MATCHES
from tests.support.auctions.games import command, failure
from tests.support.auctions.lineups import command as lineup_command
from tests.support.auctions.lineups import denied
from tests.support.guilds.context import GUILD_A, GUILD_B, ROLE_B, USER_ID, guild_fixture
from tests.support.recruitments.creation import BOT

pytestmark = pytest.mark.postgres


@pytest.mark.parametrize(
    "kind", ["bidding", "bracket", "tied", "leading", "twenty-semifinals", "twenty-final"]
)
async def test_finish_preserves_games_mmr_and_reports_nullable_early_results(
    owner, runtime_client, kind
):
    h, initial, played = await scenario(owner, runtime_client, kind)
    url = f"{PATH}/{initial['id']}"
    async with owner.begin() as c:
        await c.execute(text("UPDATE guild_config SET result_channel_id='5555555'"))
    if kind == "tied":
        # Finishing preserves MMR, so unlike cancellation it may cross a later import checkpoint.
        await later_mmr_checkpoint(owner)
    preview = await runtime_client.get(url + "/finish-preview")
    assert preview.status_code == 200, preview.text
    record = preview.json()["data"]["tournament"]
    assert set(record) == {"id", "displayNumber", "title", "status", "revision"}
    assert record["id"] == initial["id"]
    assert record["status"] == {
        "bidding": "BIDDING",
        "bracket": "BRACKET_SETUP",
    }.get(kind, "IN_GAME")
    head = await runtime_client.head(url + "/finish-preview")
    assert head.status_code == 200 and head.content == b""
    async with owner.connect() as c:
        before_matches = (
            await c.execute(text("SELECT id,status,revision FROM auction_matches ORDER BY id"))
        ).all()
        source_before = await c.scalar(
            text("SELECT to_jsonb(row) FROM auction_recruitments AS row")
        )
    before = await preserved(owner)
    response = await finish(runtime_client, h, record, reason="  남은 경기 진행 불가\u3000")
    assert response.status_code == 200, response.text
    result = response.json()["data"]
    champion = (
        played[-1]["match"]["leftTeamId"]
        if kind == "leading"
        else played[-1]["match"]["rightTeamId"]
        if kind == "twenty-final"
        else None
    )
    assert result == {
        "tournamentId": record["id"],
        "tournamentStatus": "COMPLETED",
        "tournamentRevision": record["revision"] + 1,
        "championTeamId": champion,
    }
    assert await preserved(owner) == before
    replay = await finish(runtime_client, h, record)
    assert replay.status_code == 200 and replay.json()["data"] == result
    snapshot = await runtime_client.get(url + "/result")
    assert snapshot.status_code == 200, snapshot.text
    snapshot = snapshot.json()["data"]
    assert snapshot["tournament"]["endedEarly"] is True
    assert snapshot["tournament"]["championTeamId"] == champion
    assert not any(snapshot["permissions"].values())
    matches = {m["id"]: m for m in snapshot["matches"]}
    for identifier, status, _revision in before_matches:
        match = matches[str(identifier)]
        assert match["status"] == "COMPLETED"
        assert match["endedEarly"] == (status != "COMPLETED")
    assert sum(len(m["games"]) for m in matches.values()) == sum(len(m["games"]) for m in played)
    async with owner.connect() as c:
        assert dict((await c.execute(text("SELECT id,revision FROM auction_matches"))).all()) == {
            identifier: revision + 1 for identifier, _, revision in before_matches
        }
    for old in played:
        response = await runtime_client.get(f"{MATCHES}/{old['match']['id']}")
        assert response.status_code == 200, response.text
        state = response.json()["data"]
        assert state["games"] == old["games"]
        assert state["match"]["bestOf"] == old["match"]["bestOf"]
        assert state["match"]["endedEarly"] == matches[old["match"]["id"]]["endedEarly"]
        assert not any(state["permissions"].values())
        for action in ("submit", "last", "complete"):
            failure(
                await command(runtime_client, h, state, action),
                "auction_match_tournament_not_ready",
            )
        denied(
            await lineup_command(runtime_client, h, state, "UNLOCK"),
            "auction_match_tournament_not_ready",
        )
    async with owner.connect() as c:
        assert await c.scalar(text("SELECT to_jsonb(row) FROM auction_recruitments AS row")) == (
            source_before
        )
        lifecycle = (
            await c.execute(
                text("SELECT status,ended_early,ended_at IS NOT NULL FROM auction_tournaments")
            )
        ).one()
        assert lifecycle == ("COMPLETED", True, True)
        audit = (
            await c.execute(
                text(
                    "SELECT after_summary FROM admin_audit_log "
                    "WHERE action='AUCTION_TOURNAMENT_FINISHED'"
                )
            )
        ).scalar_one()
        assert audit["reason"] == "남은 경기 진행 불가"
        assert (
            await c.scalar(
                text(
                    "SELECT count(*) FROM outbox_events "
                    "WHERE event_type='AUCTION_TOURNAMENT_FINISHED'"
                )
            )
            > 0
        )
        card = (
            await c.execute(
                text(
                    "SELECT payload FROM bot_notification_outbox "
                    "WHERE notification_type='AUCTION_TOURNAMENT_RESULT_CREATE'"
                )
            )
        ).scalar_one()
        assert card["tournament"]["endedEarly"] is True
        assert (card["champion"]["teamId"] if card["champion"] else None) == champion
        assert len(card["matches"]) == len(matches)
        if kind == "bidding":
            assert card["matches"] == [] and card["champion"] is None
        if kind == "bracket":
            assert card["matches"][0]["winnerTeamName"] is None
            assert card["matches"][0]["leftScore"] == card["matches"][0]["rightScore"] == 0
        if kind == "twenty-semifinals":
            final = next(m for m in card["matches"] if m["matchKey"] == "FINAL")
            assert final["rightTeamName"] is None and final["winnerTeamName"] is None
        assert [m["endedEarly"] for m in card["matches"]] == [
            m["endedEarly"] for m in snapshot["matches"]
        ]
    repeated = await finish(
        runtime_client, h, {**record, "revision": record["revision"] + 1}, key="again"
    )
    assert_failure(
        repeated,
        409,
        "auction_tournament_finish_unavailable",
        "완료되었거나 취소된 경매 내전은 마무리할 수 없습니다.",
    )
    assert await preserved(owner) == before


async def test_finish_validation_auth_tenant_replay_and_concurrent_duplicate(owner, runtime_client):
    h, record = await tournament(owner, runtime_client)
    url = f"{PATH}/{record['id']}"
    body = {"expectedRevision": record["revision"], "reason": "마무리"}
    for invalid in (
        {**body, "expectedRevision": True},
        {**body, "expectedRevision": 2**63},
        {**body, "unexpected": 1},
    ):
        assert_failure(
            await runtime_client.post(url + "/finish", headers=h, json=invalid),
            422,
            "unprocessable_entity",
            "요청 JSON의 필드 또는 값이 API 계약과 일치하지 않습니다.",
        )
    for raw, status, code, message in (
        (
            b'{"expectedRevision":-0,"reason":"finish"}',
            422,
            "unprocessable_entity",
            "요청 JSON의 필드 또는 값이 API 계약과 일치하지 않습니다.",
        ),
        (
            b'{"expectedRevision":1e400,"reason":"finish"}',
            400,
            "bad_request",
            "요청 형식이 올바르지 않습니다.",
        ),
    ):
        assert_failure(
            await runtime_client.post(
                url + "/finish", headers={**h, "content-type": "application/json"}, content=raw
            ),
            status,
            code,
            message,
        )
    assert_failure(
        await finish(runtime_client, h, {**record, "revision": -1}),
        422,
        "invalid_auction_tournament_finish_request",
        "경매 내전 마무리 요청 형식이 올바르지 않습니다.",
    )
    for reason in ("\u3000 ", "가" * 241):
        response = await finish(runtime_client, h, record, reason=reason)
        assert_failure(
            response, 400, "invalid_bot_command", "Bot command 요청 형식이 올바르지 않습니다."
        )
        assert response.json()["error"]["details"] == {"field": "reason"}
    assert_failure(
        await finish(runtime_client, {}, record),
        403,
        "csrf_validation_failed",
        "요청 출처를 확인할 수 없습니다.",
    )
    assert_failure(
        await runtime_client.post(
            url + "/finish",
            json=body,
            headers={k: v for k, v in h.items() if k != "idempotency-key"},
        ),
        400,
        "invalid_idempotency_key",
        "Idempotency-Key 헤더가 필요합니다.",
    )
    stale = await finish(runtime_client, h, {**record, "revision": record["revision"] + 1})
    assert_failure(
        stale,
        409,
        "auction_tournament_revision_conflict",
        "다른 운영자가 경매 내전을 변경했습니다. 최신 상태를 확인해 주세요.",
    )
    assert stale.json()["error"]["details"] == {"expectedRevision": 1, "actualRevision": 0}
    await guild_fixture(owner, GUILD_B, "Other", ROLE_B)
    async with owner.begin() as c:
        await c.execute(
            text("INSERT INTO guild_member_discord_roles VALUES (:g,:u,:r,now())"),
            {"g": GUILD_B, "u": USER_ID, "r": ROLE_B},
        )
    for response in (
        await runtime_client.get(url.replace(GUILD_A, GUILD_B) + "/finish-preview"),
        await runtime_client.post(url.replace(GUILD_A, GUILD_B) + "/finish", headers=h, json=body),
        await runtime_client.get(f"{PATH}/{uuid.uuid7()}/finish-preview"),
    ):
        assert_failure(
            response, 404, "auction_tournament_not_found", "요청한 경매 내전을 찾을 수 없습니다."
        )
    replies = await asyncio.gather(
        finish(runtime_client, h, record), finish(runtime_client, h, record)
    )
    assert [r.status_code for r in replies] == [200, 200], [r.text for r in replies]
    assert replies[0].json()["data"] == replies[1].json()["data"]
    assert_failure(
        await finish(runtime_client, h, record, reason="다른 사유"),
        409,
        "idempotency_conflict",
        "같은 Idempotency-Key가 다른 경매 내전 마무리 요청에 사용되었습니다.",
    )
    before = await effects(owner)
    async with owner.begin() as c:
        await c.execute(text("DELETE FROM guild_member_discord_roles"))
    for response in (
        await runtime_client.get(url + "/finish-preview"),
        await finish(runtime_client, h, record),
    ):
        assert_failure(
            response,
            403,
            "operator_required",
            "경매 내전을 마무리하려면 현재 길드 운영자 권한이 필요합니다.",
        )
    async with owner.begin() as c:
        await c.execute(
            text("UPDATE guild_members SET status='INACTIVE' WHERE user_id=:u"), {"u": USER_ID}
        )
    assert_failure(
        await finish(runtime_client, h, record),
        403,
        "guild_membership_required",
        "해당 길드의 활성 멤버만 경매 내전을 마무리할 수 있습니다.",
    )
    assert await effects(owner) == before
    runtime_client.cookies.clear()
    for suffix, method in (("finish-preview", "GET"), ("finish", "POST")):
        response = await runtime_client.request(
            method, url + "/" + suffix, headers={**h, "authorization": f"Bearer {BOT}"}, json=body
        )
        assert_failure(response, 401, "unauthorized", "인증이 필요합니다.")


async def test_finish_rolls_back_outbox_failure_and_competing_revisions(owner, runtime_client):
    h, record = await tournament(owner, runtime_client)
    async with owner.begin() as c:
        await c.execute(
            text("""CREATE FUNCTION finish_failure() RETURNS trigger LANGUAGE plpgsql AS
            $$ BEGIN IF NEW.event_type='AUCTION_TOURNAMENT_FINISHED' THEN
            RAISE EXCEPTION 'private finish failure'; END IF; RETURN NEW; END $$""")
        )
        await c.execute(
            text(
                "CREATE TRIGGER finish_failure BEFORE INSERT ON outbox_events "
                "FOR EACH ROW EXECUTE FUNCTION finish_failure()"
            )
        )
    before = await effects(owner)
    try:
        response = await finish(runtime_client, h, record)
        assert response.status_code == 500 and "private finish" not in response.text
        assert_failure(response, 500, "internal_error", response.json()["error"]["message"])
        assert response.json()["error"]["message"]
        assert await effects(owner) == before
        async with owner.connect() as c:
            assert (
                await c.execute(text("SELECT status,ended_early,revision FROM auction_tournaments"))
            ).one() == ("BIDDING", False, 0)
            assert await c.scalar(text("SELECT status FROM auction_recruitments")) == "CONVERTED"
    finally:
        async with owner.begin() as c:
            await c.execute(text("DROP TRIGGER finish_failure ON outbox_events"))
            await c.execute(text("DROP FUNCTION finish_failure()"))
    responses = await asyncio.gather(
        finish(runtime_client, h, record, key="competing-a"),
        finish(runtime_client, h, record, key="competing-b"),
    )
    assert sorted(r.status_code for r in responses) == [200, 409], [r.text for r in responses]
    assert_failure(
        next(r for r in responses if r.status_code == 409),
        409,
        "auction_tournament_revision_conflict",
        "다른 운영자가 경매 내전을 변경했습니다. 최신 상태를 확인해 주세요.",
    )
