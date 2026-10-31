import asyncio
import hashlib
import json
import uuid

import pytest
from sqlalchemy import text

from tests.support.assertions import assert_failure
from tests.support.auctions.bidding import PATH
from tests.support.auctions.cancellation import ACTOR, BOT_HEADERS, cancel, effects, tournament
from tests.support.auctions.recruitments import BODY, call, filled, setup
from tests.support.auctions.recruitments import PATH as RECRUITMENTS
from tests.support.guilds.context import GUILD_A, GUILD_B, USER_ID

pytestmark = pytest.mark.postgres


async def test_browser_cancel_preview_concurrent_replay_hash_effects_and_revoked_role(
    owner, runtime_client
):
    h, record = await tournament(owner, runtime_client)
    preview_url = f"{PATH}/{record['id']}/cancel-preview"
    response = await runtime_client.get(preview_url)
    assert response.status_code == 200, response.text
    assert response.json()["data"] == {
        "tournament": {
            key: record[key] for key in ("id", "displayNumber", "title", "status", "revision")
        }
    }
    assert (await runtime_client.head(preview_url)).content == b""
    initial = await effects(owner)
    responses = await asyncio.gather(
        *[cancel(runtime_client, h, record, reason="  운영 사유\u3000") for _ in range(2)]
    )
    assert [r.status_code for r in responses] == [200, 200], [r.text for r in responses]
    result = responses[0].json()["data"]
    assert responses[1].json()["data"] == result
    assert result == {
        "tournamentId": record["id"],
        "tournamentStatus": "CANCELLED",
        "tournamentRevision": record["revision"] + 1,
        "sourceRecruitmentId": record["sourceRecruitmentId"],
        "sourceRecruitmentRevision": 13,
        "reversedGameCount": 0,
    }
    assert tuple(a - b for a, b in zip(await effects(owner), initial, strict=True)) == (
        1,
        1,
        2,
        1,
        0,
    )
    async with owner.connect() as c:
        stored_hash = await c.scalar(
            text("SELECT request_hash FROM idempotency_records WHERE idempotency_key='cancel'")
        )
        assert (
            stored_hash
            == hashlib.sha256(
                json.dumps(
                    [
                        GUILD_A,
                        {
                            "tournament_id": record["id"],
                            "command": {
                                "expectedRevision": record["revision"],
                                "reason": "운영 사유",
                            },
                        },
                    ],
                    ensure_ascii=False,
                    separators=(",", ":"),
                ).encode()
            ).digest()
        )
        audit = (
            await c.execute(
                text(
                    "SELECT before_summary,after_summary FROM admin_audit_log "
                    "WHERE action='AUCTION_TOURNAMENT_CANCELLED'"
                )
            )
        ).one()
        assert audit.before_summary == {
            "tournamentStatus": "BIDDING",
            "tournamentRevision": 0,
            "sourceRecruitmentId": record["sourceRecruitmentId"],
            "sourceRecruitmentStatus": "CONVERTED",
        }
        assert audit.after_summary["reason"] == "운영 사유"
        notification = (
            await c.execute(
                text(
                    "SELECT payload FROM bot_notification_outbox "
                    "WHERE notification_type='AUCTION_RECRUITMENT_MESSAGE_DELETE'"
                )
            )
        ).scalar_one()
        assert notification["snapshot"]["recruitment"]["status"] == "CANCELLED"
        assert await c.scalar(text("SELECT status FROM auction_recruitments")) == "CANCELLED"
    conflict = await cancel(runtime_client, h, record, reason="다른 사유")
    assert_failure(
        conflict,
        409,
        "idempotency_conflict",
        "같은 Idempotency-Key가 다른 경매 내전 조기 종료 요청에 사용되었습니다.",
    )
    async with owner.begin() as c:
        await c.execute(
            text("DELETE FROM guild_member_discord_roles WHERE user_id=:user"), {"user": USER_ID}
        )
    for denied in (await runtime_client.get(preview_url), await cancel(runtime_client, h, record)):
        assert_failure(
            denied,
            403,
            "operator_required",
            "경매 내전을 조기 종료하려면 현재 길드 운영자 권한이 필요합니다.",
        )


async def test_bot_cancel_replays_before_member_check_and_preview_is_service_authorized(
    owner, runtime_client
):
    _, record = await tournament(owner, runtime_client)
    async with owner.begin() as c:
        await c.execute(text("DELETE FROM guild_member_discord_roles"))
    url = f"{PATH}/{record['id']}/cancel-preview"
    assert (await runtime_client.get(url, headers=BOT_HEADERS)).status_code == 200
    response = await cancel(runtime_client, BOT_HEADERS, record, key="997001", bot=True)
    assert response.status_code == 200, response.text
    async with owner.begin() as c:
        await c.execute(
            text("UPDATE guild_members SET status='INACTIVE' WHERE user_id=:user"),
            {"user": USER_ID},
        )
    replay = await cancel(runtime_client, BOT_HEADERS, record, key="997001", bot=True)
    assert replay.status_code == 200 and replay.json()["data"] == response.json()["data"]
    denied = await cancel(runtime_client, BOT_HEADERS, record, key="997002", bot=True)
    assert_failure(
        denied, 403, "member_inactive", "비활성화된 길드 멤버는 이 작업을 수행할 수 없습니다."
    )
    missing = await runtime_client.get(url.replace(GUILD_A, GUILD_B), headers=BOT_HEADERS)
    assert_failure(missing, 404, "not_found", "요청한 경매 내전을 찾을 수 없습니다.")
    async with owner.connect() as c:
        expected_actor = {**ACTOR, "avatarHash": None}
        # Rust Actor serializes avatarHash before roleIds.
        expected_actor = {
            k: expected_actor[k]
            for k in ("id", "globalDisplayName", "displayName", "avatarHash", "roleIds")
        }
        digest = hashlib.sha256(
            json.dumps(
                [
                    GUILD_A,
                    {
                        "tournament_id": record["id"],
                        "command": {
                            "actor": expected_actor,
                            "expectedRevision": record["revision"],
                            "reason": "운영 사유",
                        },
                    },
                ],
                ensure_ascii=False,
                separators=(",", ":"),
            ).encode()
        ).digest()
        assert (
            await c.scalar(
                text("SELECT request_hash FROM idempotency_records WHERE idempotency_key='997001'")
            )
            == digest
        )


@pytest.mark.parametrize("state", ["OPEN", "CLOSED", "CONVERTED"])
async def test_bot_force_cancel_recruitment_states(owner, runtime_client, state):
    if state == "CONVERTED":
        _, record = await tournament(owner, runtime_client)
        identifier = record["sourceRecruitmentId"]
    elif state == "CLOSED":
        h, identifier = await filled(owner, runtime_client)
        await call(runtime_client, h, "close", identifier)
    else:
        h = await setup(owner, runtime_client)
        created = await call(runtime_client, h, "create", body=BODY)
        identifier = created["recruitment"]["id"]
    url = f"{RECRUITMENTS}/{identifier}/force-cancel"
    assert (await runtime_client.post(url, json={"actor": ACTOR})).status_code == 401
    response = await runtime_client.post(url, json={"actor": ACTOR}, headers=BOT_HEADERS)
    assert response.status_code == 200, response.text
    result = response.json()["data"]
    assert result["recruitmentId"] == identifier and result["recruitmentStatus"] == "CANCELLED"
    assert result["reversedGameCount"] == 0
    assert result["recruitmentRevision"] == {"OPEN": 1, "CLOSED": 12, "CONVERTED": 13}[state]
    assert result["tournamentStatus"] == ("CANCELLED" if state == "CONVERTED" else None)
    assert (await runtime_client.post(url, json={"actor": ACTOR}, headers=BOT_HEADERS)).json()[
        "data"
    ] == result
    denied = await runtime_client.post(
        url, json={"actor": ACTOR}, headers={**BOT_HEADERS, "idempotency-key": "997003"}
    )
    assert_failure(
        denied,
        409,
        "invalid_recruitment_state",
        "현재 모집 상태에서는 이 작업을 수행할 수 없습니다.",
    )
    async with owner.connect() as c:
        summary = await c.scalar(
            text(
                "SELECT after_summary FROM admin_audit_log "
                "WHERE action='AUCTION_RECRUITMENT_FORCE_CANCELLED'"
            )
        )
        assert summary["source"] == "DISCORD_FORCE_CANCEL"
        assert summary["tournamentStatus"] == result["tournamentStatus"]


async def test_cancel_validation_revision_state_and_atomic_outbox_failure(owner, runtime_client):
    h, record = await tournament(owner, runtime_client)
    invalid = await cancel(runtime_client, h, record, reason=" ")
    assert_failure(
        invalid, 400, "invalid_bot_command", "Bot command 요청 형식이 올바르지 않습니다."
    )
    assert invalid.json()["error"]["details"] == {"field": "reason"}
    invalid = await cancel(runtime_client, h, {**record, "revision": -1})
    assert_failure(
        invalid,
        422,
        "invalid_auction_tournament_cancel_request",
        "경매 내전 조기 종료 요청 형식이 올바르지 않습니다.",
    )
    stale = await cancel(runtime_client, h, {**record, "revision": 1})
    assert_failure(
        stale,
        409,
        "auction_tournament_revision_conflict",
        "다른 운영자가 경매 내전을 변경했습니다. 최신 상태를 확인해 주세요.",
    )
    assert stale.json()["error"]["details"] == {"expectedRevision": 1, "actualRevision": 0}
    async with owner.begin() as c:
        await c.execute(
            text(
                "UPDATE auction_tournaments SET status='COMPLETED',ended_at=now(), "
                "champion_team_id=(SELECT id FROM auction_teams LIMIT 1)"
            )
        )
    unavailable = await cancel(runtime_client, h, record)
    assert_failure(
        unavailable,
        409,
        "auction_tournament_cancel_unavailable",
        "완료되었거나 이미 취소된 경매 내전은 조기 종료할 수 없습니다.",
    )
    async with owner.begin() as c:
        await c.execute(
            text(
                "UPDATE auction_tournaments SET status='BIDDING',ended_at=NULL,"
                "champion_team_id=NULL"
            )
        )
        await c.execute(
            text("""CREATE FUNCTION cancel_failure() RETURNS trigger LANGUAGE plpgsql AS
            $$ BEGIN IF NEW.event_type='AUCTION_TOURNAMENT_CANCELLED' THEN
            RAISE EXCEPTION 'private cancellation failure'; END IF; RETURN NEW; END $$""")
        )
        await c.execute(
            text(
                "CREATE TRIGGER cancel_failure BEFORE INSERT ON outbox_events "
                "FOR EACH ROW EXECUTE FUNCTION cancel_failure()"
            )
        )
    before = await effects(owner)
    try:
        failure = await cancel(runtime_client, h, record)
        assert failure.status_code == 500 and "private cancellation" not in failure.text
        assert failure.json()["error"]["code"] == "internal_error"
        assert failure.json()["error"]["message"]
        assert await effects(owner) == before
        async with owner.connect() as c:
            assert await c.scalar(text("SELECT status FROM auction_tournaments")) == "BIDDING"
            assert await c.scalar(text("SELECT status FROM auction_recruitments")) == "CONVERTED"
    finally:
        async with owner.begin() as c:
            await c.execute(text("DROP TRIGGER cancel_failure ON outbox_events"))
            await c.execute(text("DROP FUNCTION cancel_failure()"))


@pytest.mark.parametrize("force", [False, True])
async def test_cancellation_reverses_all_games_with_bot_metadata(owner, runtime_client, force):
    from tests.support.auctions.games import play
    from tests.support.auctions.games import prepared as playable

    h, bracket, match = await playable(owner, runtime_client)
    match = await play(runtime_client, h, match)
    await play(runtime_client, h, match)
    identifier = bracket["tournament"]["id"]
    preview = await runtime_client.get(f"{PATH}/{identifier}/cancel-preview")
    record = preview.json()["data"]["tournament"]
    if force:
        async with owner.connect() as c:
            source = await c.scalar(text("SELECT source_recruitment_id FROM auction_tournaments"))
        response = await runtime_client.post(
            f"{RECRUITMENTS}/{source}/force-cancel",
            headers=BOT_HEADERS,
            json={"actor": {**ACTOR, "id": "667788990011223344"}},
        )
    else:
        response = await cancel(runtime_client, h, record)
    assert response.status_code == 200, response.text
    assert response.json()["data"]["reversedGameCount"] == 2
    async with owner.connect() as c:
        games = (
            await c.execute(
                text(
                    "SELECT status,mmr_status,voided_by_type,void_reason "
                    "FROM games ORDER BY game_number"
                )
            )
        ).all()
        assert games == [("VOIDED", "REVERSED", "BOT", "TOURNAMENT_CANCELLED")] * 2
        expected_actor = "667788990011223344" if force else USER_ID
        attribution = (
            await c.execute(
                text(
                    "SELECT DISTINCT actor_id,request_id FROM mmr_ledger_entries "
                    "WHERE source_type='GAME_REVERSAL'"
                )
            )
        ).all()
        assert attribution == [(expected_actor, response.headers["x-request-id"])]
        assert (
            await c.scalar(text("SELECT count(*) FROM game_inputs WHERE status='SUPERSEDED'")) == 2
        )
        assert (
            await c.scalar(
                text(
                    "SELECT count(*) FROM mmr_ledger_entries "
                    "WHERE source_type='GAME_REVERSAL' AND actor_type='BOT'"
                )
            )
            == 20
        )
        assert (
            await c.scalar(
                text(
                    "SELECT count(*) FROM user_lane_mmr "
                    "WHERE mmr<>1500 OR games_played<>0 OR wins<>0"
                )
            )
            == 0
        )
        assert (
            await c.execute(
                text("SELECT status,lineups_locked_at,winner_team_id FROM auction_matches")
            )
        ).all() == [("CANCELLED", None, None)]
        assert await c.scalar(text("SELECT count(*) FROM auction_match_lineups")) == 10


async def test_cancellation_import_checkpoint_blocks_older_game_and_rolls_back_later_reversal(
    owner, runtime_client
):
    from tests.support.auctions.games import play
    from tests.support.auctions.games import prepared as playable

    h, bracket, match = await playable(owner, runtime_client)
    match = await play(runtime_client, h, match)
    # An unchanged imported MMR is still a checkpoint: cancellation may not cross it.
    async with owner.begin() as c:
        entry = uuid.uuid7()
        await c.execute(
            text("""INSERT INTO mmr_ledger_entries
            (guild_id,id,season_id,user_id,lane,lane_revision,source_type,source_id,
             mmr_before,mmr_after,delta,games_played_delta,wins_delta,actor_type,
             actor_id,request_id,created_at)
            SELECT guild_id,:entry,season_id,user_id,lane,revision+1,'DATA_IMPORT_SEED',:entry,
                   mmr,mmr,0,0,0,'USER',:actor,'import-checkpoint',now()
            FROM user_lane_mmr WHERE games_played=1 ORDER BY user_id,lane LIMIT 1"""),
            {"entry": entry, "actor": USER_ID},
        )
        await c.execute(
            text("""UPDATE user_lane_mmr mmr SET revision=mmr.revision+1
            FROM mmr_ledger_entries ledger WHERE ledger.id=:entry
             AND mmr.guild_id=ledger.guild_id AND mmr.season_id=ledger.season_id
             AND mmr.user_id=ledger.user_id AND mmr.lane=ledger.lane"""),
            {"entry": entry},
        )
    await play(runtime_client, h, match)
    before = await effects(owner)
    async with owner.connect() as c:
        mmr_before = (
            await c.execute(
                text(
                    "SELECT user_id,lane,mmr,games_played,wins,revision "
                    "FROM user_lane_mmr ORDER BY user_id,lane"
                )
            )
        ).all()
    preview = await runtime_client.get(f"{PATH}/{bracket['tournament']['id']}/cancel-preview")
    record = preview.json()["data"]["tournament"]
    response = await cancel(runtime_client, h, record)
    assert_failure(
        response,
        409,
        "auction_tournament_cancel_mmr_history_advanced",
        "이후 MMR 변경이 있어 경매 내전 전체를 안전하게 취소할 수 없습니다.",
    )
    assert await effects(owner) == before
    async with owner.connect() as c:
        assert (
            await c.execute(
                text(
                    "SELECT user_id,lane,mmr,games_played,wins,revision "
                    "FROM user_lane_mmr ORDER BY user_id,lane"
                )
            )
        ).all() == mmr_before
        assert await c.scalar(text("SELECT count(*) FROM games WHERE status='CONFIRMED'")) == 2
        assert (
            await c.scalar(text("SELECT count(*) FROM game_inputs WHERE status='SUBMITTED'")) == 2
        )
        assert await c.scalar(text("SELECT status FROM auction_tournaments")) == "IN_GAME"
        assert await c.scalar(text("SELECT status FROM auction_recruitments")) == "CONVERTED"
