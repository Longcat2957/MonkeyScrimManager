"""Exercise the built Docker image using only a dedicated test fixture database."""

import asyncio
import base64
import hashlib
import json
import os
import sys
import uuid
from urllib.parse import parse_qs, urlsplit

import asyncpg
import httpx


async def notification_smoke(
    client: httpx.AsyncClient, database_url: str, recruitment_id: str
) -> None:
    headers = {"authorization": "Bearer isolated-bot-service-token-32-characters"}
    response = await client.post(
        "/internal/heartbeat",
        json={
            "instanceId": "docker-smoke",
            "version": "test",
            "commitSha": "working-tree",
        },
        headers=headers,
    )
    assert response.status_code == 200 and response.json()["data"]["accepted"], (
        response.text
    )
    connection = await asyncpg.connect(database_url)
    try:
        async with connection.transaction():
            # This database belongs exclusively to the test script; prepare pending deliveries.
            await connection.execute("DELETE FROM bot_notification_outbox")
            await connection.execute(
                "UPDATE recruitments SET message_id = NULL WHERE id = $1",
                uuid.UUID(recruitment_id),
            )
            await connection.execute(
                """
                INSERT INTO auction_recruitments (guild_id, id, season_id, display_number, title,
                    format, target_count, status, revision, created_by, created_at, updated_at)
                SELECT guild_id, id, season_id, display_number, title, 'TEN_PERSON', 10, 'OPEN',
                    0, created_by, created_at, updated_at FROM recruitments WHERE id = $1
            """,
                uuid.UUID(recruitment_id),
            )
            for kind, payload in (
                (
                    "RECRUITMENT_MESSAGE_CREATE",
                    {
                        "schemaVersion": 1,
                        "recruitmentId": recruitment_id,
                        "snapshot": {"recruitment": {"revision": 0}},
                    },
                ),
                (
                    "AUCTION_RECRUITMENT_MESSAGE_CREATE",
                    {
                        "schemaVersion": 2,
                        "auctionRecruitmentId": recruitment_id,
                        "snapshot": {"recruitment": {"revision": 0}},
                    },
                ),
                ("SERIES_RESULT_CREATE", {}),
            ):
                identifier = uuid.uuid7()
                await connection.execute(
                    """
                    INSERT INTO bot_notification_outbox (id, guild_id, deduplication_key, discord_nonce,
                        channel_id, notification_type, payload, created_at, available_at)
                    VALUES ($1, '123', $2, $3, '789', $2, $4::jsonb, now(), now())
                """,
                    identifier,
                    kind,
                    base64.urlsafe_b64encode(identifier.bytes).decode().rstrip("="),
                    json.dumps(payload),
                )
        response = await client.post(
            "/internal/notifications/claim",
            json={"instanceId": "docker-smoke"},
            headers=headers,
        )
        assert response.status_code == 200, response.text
        rows = response.json()["data"]["notifications"]
        assert len(rows) == 3
        for row in rows:
            body = {
                "instanceId": "docker-smoke",
                "notificationId": row["id"],
                "leaseToken": row["leaseToken"],
            }
            if row["type"] == "SERIES_RESULT_CREATE":
                action = "nack"
                body["failure"] = {
                    "kind": "TRANSPORT",
                    "message": "isolated retry test",
                }
            else:
                action = (
                    "auction-" if row["type"].startswith("AUCTION_") else ""
                ) + "recruitment-message/ack"
                body["discordMessageId"] = "987"
            response = await client.post(
                "/internal/notifications/" + action, json=body, headers=headers
            )
            assert response.status_code == 200, response.text
        await connection.execute(
            "UPDATE bot_notification_outbox SET available_at=now() WHERE delivered_at IS NULL"
        )
        response = await client.post(
            "/internal/notifications/claim",
            json={"instanceId": "docker-smoke"},
            headers=headers,
        )
        rows = response.json()["data"]["notifications"]
        assert (
            len(rows) == 2
        )  # Retried generic delivery and cancelled recruitment message deletion.
        for row in rows:
            response = await client.post(
                "/internal/notifications/ack",
                json={
                    "instanceId": "docker-smoke",
                    "notificationId": row["id"],
                    "leaseToken": row["leaseToken"],
                    "discordMessageId": "987",
                },
                headers=headers,
            )
            assert (
                response.status_code == 200 and response.json()["data"]["acknowledged"]
            ), response.text
    finally:
        await connection.close()


async def auction_smoke(
    client: httpx.AsyncClient, database_url: str, headers: dict[str, str]
) -> None:
    connection = await asyncpg.connect(database_url)
    try:
        await connection.execute(
            "INSERT INTO guild_counters VALUES ('123','AUCTION_RECRUITMENT',2)"
        )
    finally:
        await connection.close()
    base = "/api/guilds/123/auction-recruitments"

    async def command(method, suffix, body, key):
        response = await client.request(
            method,
            base + suffix,
            json=body,
            headers={**headers, "idempotency-key": "docker-auction-" + key},
        )
        assert response.status_code == (201 if not suffix else 200), response.text
        return response.json()["data"]

    body = {"format": "TEN_PERSON", "expectedStartAt": "2026-09-15T20:00:00+09:00"}
    created = await command("POST", "", body, "create")
    assert await command("POST", "", body, "create") == created
    identifier = created["recruitment"]["id"]
    path = "/" + identifier
    listed = await client.get(base + "?state=actionable", headers=headers)
    assert listed.status_code == 200 and any(
        r["id"] == identifier for r in listed.json()["data"]["recruitments"]
    )
    # Deliver the actual generated payload through the migrated typed ACK handler.
    bot_headers = {"authorization": "Bearer isolated-bot-service-token-32-characters"}
    claim = await client.post(
        "/internal/notifications/claim",
        json={"instanceId": "auction-smoke"},
        headers=bot_headers,
    )
    row = next(
        r
        for r in claim.json()["data"]["notifications"]
        if r["type"] == "AUCTION_RECRUITMENT_MESSAGE_CREATE"
    )
    ack = await client.post(
        "/internal/notifications/auction-recruitment-message/ack",
        json={
            "instanceId": "auction-smoke",
            "notificationId": row["id"],
            "leaseToken": row["leaseToken"],
            "discordMessageId": "998877",
        },
        headers=bot_headers,
    )
    assert ack.status_code == 200, ack.text
    for i in range(1, 11):
        await command(
            "POST",
            path + "/members",
            {
                "memberId": str(2000 + i),
                "declaredLane": ["TOP", "JUNGLE", "MID", "BOTTOM", "SUPPORT"][
                    (i - 1) % 5
                ],
            },
            f"join-{i}",
        )
    await command("DELETE", path + "/members/2001", {}, "leave")
    await command(
        "POST", path + "/members", {"memberId": "2001", "declaredLane": "TOP"}, "rejoin"
    )
    closed = await command("POST", path + "/close", {}, "close")
    draft = await client.get(base + path + "/setup-draft", headers=headers)
    assert draft.status_code == 200, draft.text
    saved = await command(
        "PATCH",
        path + "/setup-draft",
        {
            "expectedRevision": draft.json()["data"]["draft"]["revision"],
            "teams": [
                {
                    "teamIndex": i,
                    "teamName": f"Docker team {i}",
                    "captainUserId": str(2000 + i),
                    "initialPoints": 1000,
                }
                for i in (1, 2)
            ],
        },
        "setup",
    )
    assert saved["permissions"]["canCreateTournament"]
    assert await command("POST", path + "/participant-confirmation-dms", {}, "dm") == {
        "queuedRecipientCount": 10,
        "alreadyQueuedRecipientCount": 0,
    }
    reopened = await command(
        "POST",
        path + "/reopen",
        {"expectedRevision": closed["recruitment"]["revision"]},
        "reopen",
    )
    cancelled = await command(
        "DELETE",
        path,
        {"expectedRevision": reopened["recruitment"]["revision"]},
        "cancel",
    )
    detail = await client.get(base + path, headers=headers)
    assert detail.status_code == 200 and detail.json()["data"] == cancelled
    restored = await command(
        "POST",
        path + "/restore",
        {"expectedRevision": cancelled["recruitment"]["revision"]},
        "restore",
    )
    assert (
        restored["recruitment"]["status"] == "OPEN"
        and restored["recruitment"]["messageId"] is None
    )

    # Continue the recruitment into the auction tournament and all nine draft commands.
    closed = await command("POST", path + "/close", {}, "tournament-close")
    draft = await client.get(base + path + "/setup-draft", headers=headers)
    connection = await asyncpg.connect(database_url)
    try:
        await connection.execute(
            "INSERT INTO guild_counters VALUES ('123','AUCTION_TOURNAMENT',1)"
        )
    finally:
        await connection.close()
    tournaments = "/api/guilds/123/auction-tournaments"
    response = await client.post(
        tournaments,
        json={
            "sourceRecruitmentId": identifier,
            "expectedRevision": draft.json()["data"]["draft"]["revision"],
        },
        headers={**headers, "idempotency-key": "docker-tournament-create"},
    )
    assert response.status_code == 201, response.text
    state = response.json()["data"]
    tournament_path = tournaments + "/" + state["tournament"]["id"]

    async def draft_command(kind, **extra):
        nonlocal state
        body = {
            "kind": kind,
            "expectedRevision": state["tournament"]["revision"],
            **extra,
        }
        key = "docker-draft-" + str(state["tournament"]["revision"])
        response = await client.post(
            tournament_path + "/commands",
            json=body,
            headers={**headers, "idempotency-key": key},
        )
        assert response.status_code == 200, response.text
        state = response.json()["data"]
        replay = await client.post(
            tournament_path + "/commands",
            json=body,
            headers={**headers, "idempotency-key": key},
        )
        assert replay.status_code == 200 and replay.json()["data"] == state

    listed = await client.get(tournaments + "?state=active", headers=headers)
    assert listed.status_code == 200 and len(listed.json()["data"]["tournaments"]) == 1
    await draft_command("RESHUFFLE_CANDIDATES")
    await draft_command("DRAW_NEXT")
    target = state["draft"]["currentTarget"]["userId"]
    team = state["teams"][0]["id"]
    await draft_command("MARK_UNSOLD", memberId=target)
    await draft_command("RESTORE_UNSOLD", memberId=target)
    await draft_command("AWARD_BID", memberId=target, teamId=team, points=150)
    await draft_command("REVERT_LAST")
    assert state["teams"][0]["currentPoints"] == 1000
    await draft_command("ASSIGN_CURRENT", memberId=target, teamId=team)
    await draft_command("DRAW_NEXT")
    target = state["draft"]["currentTarget"]["userId"]
    await draft_command("MARK_UNSOLD", memberId=target)
    await draft_command("ASSIGN_UNSOLD", memberId=target, teamId=team)
    response = await client.get(tournament_path, headers=headers)
    assert response.status_code == 200 and response.json()["data"] == state
    # Preserve Rust's actual deployment grant behavior: ledger DELETE is denied.
    reset = await client.post(
        tournament_path + "/commands",
        json={
            "kind": "RESET_TO_INITIAL",
            "expectedRevision": state["tournament"]["revision"],
        },
        headers={**headers, "idempotency-key": "docker-draft-reset"},
    )
    assert (
        reset.status_code == 500 and reset.json()["error"]["code"] == "internal_error"
    )
    assert (await client.get(tournament_path, headers=headers)).json()["data"] == state


async def main() -> None:
    database_url = os.environ["FASTAPI_TEST_OWNER_URL"]
    if (
        "/monkeybot_fastapi_test?" not in database_url
        or "@127.0.0.1:" not in database_url
    ):
        raise SystemExit("Docker smoke requires an isolated test database")
    token = "docker-smoke-test-session"
    user_id = "123456789012345679"
    connection = await asyncpg.connect(database_url)
    try:
        async with connection.transaction():
            await connection.execute(
                """
                INSERT INTO users (user_id, identity_kind, display_name, created_at, updated_at)
                VALUES ($1, 'DISCORD', 'Docker 검증 사용자', now(), now())
            """,
                user_id,
            )
            await connection.execute(
                """
                INSERT INTO user_discord_identities (user_id, discord_id, created_at, updated_at)
                VALUES ($1, $1, now(), now())
            """,
                user_id,
            )
            await connection.execute(
                """
                INSERT INTO user_sessions
                    (id, user_id, token_hash, csrf_secret_hash,
                     created_at, last_seen_at, expires_at)
                VALUES ($1, $2, $3, $3, now(), now(), now() + interval '1 hour')
            """,
                uuid.uuid7(),
                user_id,
                hashlib.sha256(token.encode()).digest(),
            )
    finally:
        await connection.close()
    async with httpx.AsyncClient(base_url=sys.argv[1], timeout=5) as client:
        for path in ("/healthz", "/readyz"):
            response = await client.get(path)
            assert response.status_code == 200, (path, response.status_code)
        anonymous = await client.get("/api/me")
        assert anonymous.status_code == 401
        assert anonymous.json()["error"]["code"] == "unauthorized"
        assert anonymous.json()["error"]["message"] == "인증이 필요합니다."
        recruitment_auth = await client.get(
            "/api/guilds/123/recruitments?state=actionable"
        )
        assert recruitment_auth.status_code == 401
        assert recruitment_auth.json()["error"]["code"] == "unauthorized"
        detail_auth = await client.get(
            "/api/guilds/123/recruitments/00000000-0000-0000-0000-000000000001"
        )
        assert detail_auth.status_code == 401
        assert detail_auth.json()["error"]["code"] == "unauthorized"
        authenticated = await client.get(
            "/api/me", headers={"cookie": f"monkeybot_session={token}"}
        )
        assert authenticated.status_code == 200
        assert authenticated.json()["data"] == {
            "discordId": user_id,
            "displayName": "Docker 검증 사용자",
            "avatarUrl": None,
            "defaultGuildId": None,
            "serviceAdmin": False,
        }
        assert (
            authenticated.json()["meta"]["requestId"]
            == authenticated.headers["x-request-id"]
        )
        assert (
            await client.head(
                "/api/me", headers={"cookie": f"monkeybot_session={token}"}
            )
        ).content == b""
        auth_headers = {"cookie": f"monkeybot_session={token}; monkeybot_csrf={token}"}
        guilds = await client.get("/api/me/guilds", headers=auth_headers)
        assert guilds.status_code == 200 and guilds.json()["data"] == {"guilds": []}
        hidden = await client.get("/api/guilds/123", headers=auth_headers)
        assert hidden.status_code == 404
        assert hidden.json()["error"]["message"] == "요청한 길드를 찾을 수 없습니다."
        denied = await client.post("/api/logout", headers=auth_headers)
        assert denied.status_code == 403
        assert denied.json()["error"]["code"] == "csrf_validation_failed"
        csrf_headers = {
            **auth_headers,
            "origin": "http://localhost:5173",
            "x-csrf-token": token,
        }
        no_membership = await client.put(
            "/api/me/default-guild", json={"guildId": "123"}, headers=csrf_headers
        )
        assert no_membership.status_code == 404
        assert (
            no_membership.json()["error"]["message"]
            == "요청한 길드를 찾을 수 없습니다."
        )

        profile_path = f"/api/users/{user_id}/profile"
        assert (await client.get(profile_path)).json()["data"]["activeGuildCount"] == 0
        privacy = await client.get("/api/me/privacy", headers=auth_headers)
        assert privacy.json()["data"]["settings"]["publicProfile"] is True
        private = {
            "publicProfile": False,
            "showRiotIds": False,
            "allowExternalRecordSearch": False,
        }
        changed = await client.put(
            "/api/me/privacy", json=private, headers=csrf_headers
        )
        assert changed.status_code == 200 and changed.json()["data"] == {
            "settings": private
        }
        assert (await client.get(profile_path)).status_code == 404
        champions = await client.put(
            "/api/me/champion-preferences/MID",
            json={"championIds": ["Ahri", "Orianna"]},
            headers=csrf_headers,
        )
        assert champions.status_code == 200
        assert champions.json()["data"]["preferences"]["MID"] == ["Ahri", "Orianna"]
        stored = await client.get("/api/me/champion-preferences", headers=auth_headers)
        assert stored.json()["data"] == champions.json()["data"]

        connection = await asyncpg.connect(database_url)
        try:
            async with connection.transaction():
                await connection.execute(
                    "INSERT INTO guilds "
                    "(guild_id, discord_name, status, installed_at, updated_at) "
                    "VALUES ('123', 'Docker guild', 'ACTIVE', now(), now())"
                )
                await connection.execute(
                    "INSERT INTO guild_members "
                    "(guild_id, user_id, display_name, status, first_seen_at) "
                    "VALUES ('123', $1, 'Docker member', 'ACTIVE', now())",
                    user_id,
                )
                await connection.execute(
                    "INSERT INTO guild_config "
                    "(guild_id, display_name, operator_role_id, recruitment_channel_id, "
                    "is_externally_public, revision, updated_at) "
                    "VALUES ('123', 'Docker guild', '456', '789', false, 0, now())"
                )
                await connection.execute(
                    "INSERT INTO seasons "
                    "(guild_id, id, name, status, started_at, created_at) "
                    "VALUES ('123', $1, 'Docker season', 'ACTIVE', now(), now())",
                    uuid.uuid7(),
                )
                await connection.execute(
                    "INSERT INTO guild_counters "
                    "(guild_id, counter_key, next_value) VALUES ('123', 'RECRUITMENT', 1)"
                )
        finally:
            await connection.close()
        create_headers = {**csrf_headers, "idempotency-key": "docker-create-1"}
        body = {
            "bestOf": 3,
            "hardFearless": True,
            "expectedStartAt": "2026-09-13T20:00:00+09:00",
        }
        created = await client.post(
            "/api/guilds/123/recruitments", json=body, headers=create_headers
        )
        assert created.status_code == 201, created.text
        replayed = await client.post(
            "/api/guilds/123/recruitments", json=body, headers=create_headers
        )
        assert (
            replayed.status_code == 201
            and replayed.json()["data"] == created.json()["data"]
        )
        recruitment_id = created.json()["data"]["recruitment"]["id"]
        detail = await client.get(
            f"/api/guilds/123/recruitments/{recruitment_id}", headers=auth_headers
        )
        assert (
            detail.status_code == 200
            and detail.json()["data"] == created.json()["data"]
        )

        connection = await asyncpg.connect(database_url)
        try:
            await connection.execute(
                "INSERT INTO guild_member_discord_roles "
                "(guild_id, user_id, role_id, last_synced_at) VALUES ('123', $1, '456', now())",
                user_id,
            )
        finally:
            await connection.close()
        member_path = f"/api/guilds/123/recruitments/{recruitment_id}/members"
        joined = await client.post(
            member_path,
            json={"memberId": user_id},
            headers={**csrf_headers, "idempotency-key": "docker-join"},
        )
        assert joined.status_code == 200, joined.text
        assert joined.json()["data"]["participants"][0]["id"] == user_id
        left = await client.request(
            "DELETE",
            member_path + "/" + user_id,
            json={},
            headers={**csrf_headers, "idempotency-key": "docker-leave"},
        )
        assert left.status_code == 200 and left.json()["data"]["participants"] == []

        recruitment_path = member_path.removesuffix("/members")
        cancelled = await client.request(
            "DELETE",
            recruitment_path,
            json={"expectedRevision": 2},
            headers={**csrf_headers, "idempotency-key": "docker-cancel"},
        )
        assert cancelled.status_code == 200, cancelled.text
        assert cancelled.json()["data"]["recruitment"]["status"] == "CANCELLED"
        assert (
            await client.get(recruitment_path, headers=auth_headers)
        ).status_code == 200
        restored = await client.post(
            recruitment_path + "/restore",
            json={"expectedRevision": 3},
            headers={**csrf_headers, "idempotency-key": "docker-restore"},
        )
        assert restored.status_code == 200, restored.text
        assert restored.json()["data"]["recruitment"]["status"] == "OPEN"
        connection = await asyncpg.connect(database_url)
        try:
            async with connection.transaction():
                await connection.execute(
                    "INSERT INTO users (user_id, identity_kind, display_name, created_at, updated_at) "
                    "SELECT (2000+n)::text, 'DISCORD', 'Docker player', now(), now() "
                    "FROM generate_series(1,10) n"
                )
                await connection.execute(
                    "INSERT INTO guild_members "
                    "(guild_id, user_id, display_name, status, first_seen_at) "
                    "SELECT '123', (2000+n)::text, 'Docker player', 'ACTIVE', now() "
                    "FROM generate_series(1,10) n"
                )
                await connection.execute(
                    "INSERT INTO recruitment_participants "
                    "(guild_id, recruitment_id, user_id, status, joined_at) "
                    "SELECT '123', $1, (2000+n)::text, 'JOINED', now() "
                    "FROM generate_series(1,10) n",
                    uuid.UUID(recruitment_id),
                )
        finally:
            await connection.close()
        closed = await client.post(
            recruitment_path + "/close",
            json={},
            headers={**csrf_headers, "idempotency-key": "docker-close"},
        )
        assert closed.status_code == 200, closed.text
        assert closed.json()["data"]["recruitment"]["status"] == "CLOSED"
        edited = await client.patch(
            recruitment_path + "/entry-draft",
            json={
                "expectedRevision": 2,
                "operation": {
                    "type": "MOVE_PARTICIPANT",
                    "participantId": "2001",
                    "slot": "TEAM_1_TOP",
                },
            },
            headers={**csrf_headers, "idempotency-key": "docker-entry-edit"},
        )
        assert edited.status_code == 200, edited.text
        assert edited.json()["data"]["draft"]["assignments"] == [
            {"participantId": "2001", "team": "TEAM_1", "lane": "TOP"}
        ]
        reset = await client.post(
            recruitment_path + "/entry-draft/reset",
            json={"expectedRevision": 3},
            headers={**csrf_headers, "idempotency-key": "docker-entry-reset"},
        )
        assert reset.status_code == 200, reset.text
        assert reset.json()["data"]["draft"]["assignments"] == []
        reopened = await client.post(
            recruitment_path + "/reopen",
            json={"expectedRevision": 5},
            headers={**csrf_headers, "idempotency-key": "docker-reopen"},
        )
        assert reopened.status_code == 200, reopened.text
        entry = await client.get(
            recruitment_path + "/entry-draft", headers=auth_headers
        )
        assert entry.status_code == 200, entry.text
        assert entry.json()["data"] == reopened.json()["data"]
        assert entry.json()["data"]["recruitment"]["revision"] == 6
        assert len(entry.json()["data"]["draft"]["participants"]) == 10

        preferred = await client.put(
            recruitment_path + "/members/2001/preferred-lanes",
            json={
                "actor": {
                    "id": "2001",
                    "globalDisplayName": "Docker player",
                    "displayName": "Docker player",
                    "roleIds": [],
                },
                "preferredLanes": ["MID", "TOP"],
            },
            headers={
                "authorization": "Bearer isolated-bot-service-token-32-characters",
                "idempotency-key": "2000001",
            },
        )
        assert preferred.status_code == 200, preferred.text
        player = next(
            p for p in preferred.json()["data"]["participants"] if p["id"] == "2001"
        )
        assert player["preferredLanes"] == ["MID", "TOP"]

        bound = await client.put(
            recruitment_path + "/message",
            json={
                "actor": {
                    "id": user_id,
                    "globalDisplayName": "Docker member",
                    "displayName": "Docker member",
                    "roleIds": ["456"],
                },
                "channelId": "789",
                "messageId": "987",
            },
            headers={
                "authorization": "Bearer isolated-bot-service-token-32-characters",
                "idempotency-key": "2000002",
            },
        )
        assert bound.status_code == 200, bound.text
        connection = await asyncpg.connect(database_url)
        try:
            async with connection.transaction():
                await connection.execute(
                    "INSERT INTO recruitment_entry_assignments "
                    "(guild_id, recruitment_id, user_id, team, lane) "
                    "SELECT '123', $1, (2000+n)::text, "
                    "CASE WHEN n <= 5 THEN 'TEAM_1' ELSE 'TEAM_2' END, "
                    "(ARRAY['TOP','JUNGLE','MID','BOTTOM','SUPPORT'])[(n-1)%5+1] "
                    "FROM generate_series(1,10) n",
                    uuid.UUID(recruitment_id),
                )
                await connection.execute(
                    "INSERT INTO guild_counters (guild_id, counter_key, next_value) "
                    "VALUES ('123', 'SERIES', 1)"
                )
                await connection.execute(
                    "UPDATE guild_config SET entry_channel_id = '789' WHERE guild_id = '123'"
                )
        finally:
            await connection.close()
        closed = await client.post(
            recruitment_path + "/close",
            json={},
            headers={**csrf_headers, "idempotency-key": "docker-close-series"},
        )
        assert closed.status_code == 200, closed.text
        dispatch = await client.post(
            recruitment_path + "/participant-confirmation-dms",
            json={},
            headers={**csrf_headers, "idempotency-key": "docker-dm"},
        )
        assert dispatch.status_code == 200, dispatch.text
        assert dispatch.json()["data"] == {
            "queuedRecipientCount": 10,
            "alreadyQueuedRecipientCount": 0,
        }
        series_body = {
            "sourceRecruitmentId": recruitment_id,
            "expectedRevision": closed.json()["data"]["recruitment"]["revision"],
        }
        series_headers = {**csrf_headers, "idempotency-key": "docker-series"}
        series = await client.post(
            "/api/guilds/123/series", json=series_body, headers=series_headers
        )
        assert series.status_code == 201, series.text
        replay = await client.post(
            "/api/guilds/123/series", json=series_body, headers=series_headers
        )
        assert (
            replay.status_code == 201 and replay.json()["data"] == series.json()["data"]
        )
        assert (
            await client.get(recruitment_path, headers=auth_headers)
        ).status_code == 404

        series_path = "/api/guilds/123/series/" + series.json()["data"]["series"]["id"]
        listed = await client.get(
            "/api/guilds/123/series?state=active", headers=auth_headers
        )
        assert listed.status_code == 200 and len(listed.json()["data"]["series"]) == 1
        detail = await client.get(series_path, headers=auth_headers)
        assert (
            detail.status_code == 200
            and detail.json()["data"]["currentGame"]["setNumber"] == 1
        )
        picked = await client.patch(
            series_path + "/pickban",
            json={
                "expectedRevision": 0,
                "operation": {"type": "SET_SIDE", "sidePreset": "DEFAULT"},
            },
            headers=csrf_headers,
        )
        assert picked.status_code == 200, picked.text
        game_result = await client.post(
            series_path + "/games",
            json={
                "expectedRevision": 1,
                "setNumber": 1,
                "sidePreset": "DEFAULT",
                "winner": "BLUE",
            },
            headers={**csrf_headers, "idempotency-key": "docker-game"},
        )
        assert game_result.status_code == 201, game_result.text
        completed = await client.post(
            series_path + "/complete",
            json={"expectedRevision": 2, "outcome": "TEAM_1"},
            headers={**csrf_headers, "idempotency-key": "docker-early"},
        )
        assert completed.status_code == 200, completed.text
        result = await client.get(series_path + "/result", headers=auth_headers)
        assert result.status_code == 200 and result.json()["data"]["score"] == {
            "team1": 1,
            "team2": 0,
        }
        undone = await client.request(
            "DELETE",
            series_path + "/games/last",
            json={"expectedRevision": 3},
            headers={**csrf_headers, "idempotency-key": "docker-game-undo"},
        )
        assert undone.status_code == 200, undone.text
        reverted = await client.post(
            series_path + "/revert",
            json={"expectedRevision": 4},
            headers={**csrf_headers, "idempotency-key": "docker-series-revert"},
        )
        assert reverted.status_code == 200, reverted.text
        source = await client.get(recruitment_path, headers=auth_headers)
        assert (
            source.status_code == 200
            and source.json()["data"]["recruitment"]["status"] == "CLOSED"
        )
        second = await client.post(
            "/api/guilds/123/series",
            json={
                "sourceRecruitmentId": recruitment_id,
                "expectedRevision": source.json()["data"]["recruitment"]["revision"],
            },
            headers={**csrf_headers, "idempotency-key": "docker-series-second"},
        )
        assert second.status_code == 201, second.text
        cancelled = await client.request(
            "DELETE",
            "/api/guilds/123/series/" + second.json()["data"]["series"]["id"],
            json={"expectedRevision": 0},
            headers={**csrf_headers, "idempotency-key": "docker-series-delete"},
        )
        assert cancelled.status_code == 200, cancelled.text

        # Exercise operator settings and season rotation after completing the game workflow.
        catalog = await client.put(
            "/internal/guilds/123/discord-catalog",
            json={
                "roles": [
                    {"id": "456", "name": "Operators", "position": 1, "managed": False}
                ],
                "channels": [
                    {"id": "789", "name": "Recruitment", "position": 0, "kind": "TEXT"}
                ],
            },
            headers={
                "authorization": "Bearer isolated-bot-service-token-32-characters"
            },
        )
        assert catalog.status_code == 200, catalog.text
        settings = await client.get("/api/guilds/123/settings", headers=auth_headers)
        assert settings.status_code == 200, settings.text
        current_settings = settings.json()["data"]
        updated = await client.put(
            "/api/guilds/123/settings",
            json={
                **current_settings["settings"],
                "displayName": "Docker updated guild",
                "expectedRevision": current_settings["revision"],
            },
            headers=csrf_headers,
        )
        assert updated.status_code == 200, updated.text
        assert updated.json()["data"]["revision"] == current_settings["revision"] + 1
        seasons = await client.get("/api/guilds/123/seasons", headers=auth_headers)
        assert seasons.status_code == 200, seasons.text
        season_body = {
            "name": "Docker next season",
            "expectedCurrentSeasonId": seasons.json()["data"]["current"]["id"],
        }
        season_headers = {**csrf_headers, "idempotency-key": "docker-season-rotate"}
        rotated = await client.post(
            "/api/guilds/123/seasons", json=season_body, headers=season_headers
        )
        assert rotated.status_code == 200, rotated.text
        assert rotated.json()["data"]["current"]["name"] == "Docker next season"
        replay = await client.post(
            "/api/guilds/123/seasons", json=season_body, headers=season_headers
        )
        assert (
            replay.status_code == 200
            and replay.json()["data"] == rotated.json()["data"]
        )

        await notification_smoke(client, database_url, recruitment_id)
        await auction_smoke(client, database_url, csrf_headers)

        # Reuse the cancelled regular source after both series have been removed.
        source = (await client.get(recruitment_path, headers=auth_headers)).json()[
            "data"
        ]
        restored = await client.post(
            recruitment_path + "/restore",
            json={"expectedRevision": source["recruitment"]["revision"]},
            headers={**csrf_headers, "idempotency-key": "docker-conversion-restore"},
        )
        assert restored.status_code == 200, restored.text
        conversion_body = {
            "expectedRevision": restored.json()["data"]["recruitment"]["revision"],
            "expectedStartAt": "2026-09-20T11:00:00Z",
        }
        conversion_headers = {**csrf_headers, "idempotency-key": "docker-entry-auction"}
        converted = await client.post(
            recruitment_path + "/auction-recruitment",
            json=conversion_body,
            headers=conversion_headers,
        )
        assert converted.status_code == 201, converted.text
        data = converted.json()["data"]
        assert data["recruitment"]["status"] == "CLOSED"
        assert len(data["participants"]) == 10
        assert "2026-09-20 20:00 (KST)" in data["recruitment"]["title"]
        replay = await client.post(
            recruitment_path + "/auction-recruitment",
            json=conversion_body,
            headers=conversion_headers,
        )
        assert replay.status_code == 201 and replay.json()["data"] == data
        detail = await client.get(
            "/api/guilds/123/auction-recruitments/" + data["recruitment"]["id"],
            headers=auth_headers,
        )
        assert detail.status_code == 200 and detail.json()["data"] == data
        source = await client.get(recruitment_path, headers=auth_headers)
        # Converted regular recruitments are hidden by the existing read contract.
        assert (
            source.status_code == 404 and source.json()["error"]["code"] == "not_found"
        )
        connection = await asyncpg.connect(database_url)
        try:
            assert (
                await connection.fetchval(
                    "SELECT status FROM recruitments WHERE guild_id='123' AND id=$1",
                    uuid.UUID(recruitment_id),
                )
                == "CONVERTED"
            )
        finally:
            await connection.close()

        # Stop before any provider exchange: test authorization denial and one-shot consumption.
        start = await client.get("/oauth/discord/start", params={"next": "/me"})
        assert start.status_code == 302
        state = parse_qs(urlsplit(start.headers["location"]).query)["state"][0]
        params = {"state": state, "error": "access_denied"}
        callback = await client.get("/oauth/callback", params=params)
        assert callback.status_code == 400
        assert callback.json()["error"]["code"] == "oauth_authorization_denied"
        replay = await client.get("/oauth/callback", params=params)
        assert replay.status_code == 400
        assert replay.json()["error"]["code"] == "invalid_oauth_state"
        logout = await client.post("/api/logout", headers=csrf_headers)
        assert logout.status_code == 200 and logout.json()["data"] == {
            "loggedOut": True
        }
        assert (await client.get("/api/me", headers=auth_headers)).status_code == 401
    print(
        "Docker HTTP: health/readiness, session, guild authorization, "
        "profile/privacy, champion preferences, recruitment create/replay/detail/join/leave/"
        "cancel/restore/close/reopen/entry/edit/reset/preferred-lanes/bind/DM/series/list/detail/pickban/games/result/revert/complete/delete, notification claim/ack/typed-ack/nack/heartbeat, catalog/settings/seasons/rotation/replay, auction recruitment 12 operations/typed ACK/replay, entry-to-auction conversion/replay, tournament creation/list/detail/9 draft commands/Rust reset grant failure, OAuth denial/replay, CSRF/logout passed."
    )


if __name__ == "__main__":
    asyncio.run(main())
