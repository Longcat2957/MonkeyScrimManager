"""Exercise the built Docker image using only a dedicated test fixture database."""

import asyncio
import base64
import hashlib
import json
import math
import os
import sys
import uuid
from urllib.parse import parse_qs, urlsplit

import asyncpg
import httpx


def record_projection_smoke(data: dict, *, public: bool) -> None:
    assert set(data["member"]) == {
        "displayName",
        "avatarUrl",
        "mainLane",
        "profileHref",
    }
    assert data["member"]["profileHref"] == ("/users/2002" if public else None)
    assert set(data["championPreferences"]) == {
        "TOP",
        "JUNGLE",
        "MID",
        "BOTTOM",
        "SUPPORT",
    }
    assert data["riotAccounts"] == (
        [
            {
                "platform": "KR",
                "gameName": "Docker visible",
                "tagLine": "KR1",
                "profileIconId": 123,
                "isMain": True,
            }
        ]
        if public
        else []
    )
    serialized = json.dumps(data)
    assert not any(
        f'"{private}"' in serialized
        for private in (
            "userId",
            "discordId",
            "puuid",
            "routingRegion",
            "normalizedGameName",
            "normalizedTagLine",
            "showRiotIds",
            "publicProfile",
        )
    ), serialized


async def record_identity_smoke(
    client: httpx.AsyncClient, headers: dict[str, str], viewer_id: str
) -> None:
    base = "/api/guilds/123/records/"
    for endpoint in ("profile", "search"):
        for query, public in (
            ("2001", False),
            ("2002", True),
            (" ＤＯＣＫＥＲ visible#kr1 ", True),
        ):
            response = await client.get(
                base + endpoint, params={"q": query}, headers=headers
            )
            assert response.status_code == 200, response.text
            record_projection_smoke(response.json()["data"], public=public)
        head = await client.head(base + endpoint, params={"q": "2001"}, headers=headers)
        assert head.status_code == 200 and head.content == b"", endpoint
    own = await client.get(base + "profile", params={"q": viewer_id}, headers=headers)
    assert own.status_code == 200, own.text
    assert own.json()["data"]["championPreferences"]["MID"] == ["Ahri", "Orianna"]

    response = await client.get(
        base + "suggestions", params={"q": " dOcKeR p ", "limit": 2}, headers=headers
    )
    assert response.status_code == 200, response.text
    assert response.json()["data"]["suggestions"] == [
        {
            "kind": "guildMember",
            "value": "Docker player",
            "label": "Docker player",
            "description": description,
            "profileIconId": icon,
        }
        for description, icon in ((None, None), ("Docker visible#KR1", 123))
    ]
    head = await client.head(
        base + "suggestions", params={"q": "Docker"}, headers=headers
    )
    assert head.status_code == 200 and head.content == b""
    public_path = "/api/riot/records/suggestions"
    response = await client.get(public_path, params={"q": " ＤＯＣＫＥＲ "})
    assert response.status_code == 200, response.text
    # Riot identities are public even when their Discord association is private.
    assert response.json()["data"]["suggestions"] == [
        {
            "kind": "riotAccount",
            "value": name + "#KR1",
            "label": name + "#KR1",
            "description": "KR",
            "profileIconId": icon,
        }
        for name, icon in (("Docker private", 456), ("Docker visible", 123))
    ]
    head = await client.head(public_path, params={"q": "Docker"})
    assert head.status_code == 200 and head.content == b""
    other_platform = await client.get(
        public_path, params={"q": "Docker", "platform": "NA1"}
    )
    assert other_platform.status_code == 200, other_platform.text
    assert other_platform.json()["data"] == {"suggestions": []}

    cases = [
        (endpoint, {"q": query}, status, code, message)
        for endpoint in (base + "profile", base + "search")
        for query, status, code, message in (
            (" ", 400, "invalid_search_query", "검색어가 올바르지 않습니다."),
            (
                "Docker player",
                409,
                "ambiguous_guild_member",
                "같은 이름의 길드 멤버가 여러 명입니다.",
            ),
            (
                "Docker private#KR1",
                404,
                "not_found",
                "요청한 길드 멤버를 찾을 수 없습니다.",
            ),
        )
    ]
    cases.extend(
        (base + "search", {"q": "2001", field: value}, status, code, message)
        for field, value, status, code, message in (
            (
                "cursor",
                "invalid",
                400,
                "invalid_cursor",
                "cursor 형식이 올바르지 않습니다.",
            ),
            (
                "limit",
                "51",
                400,
                "invalid_limit",
                "limit은 1 이상 50 이하이어야 합니다.",
            ),
            (
                "seasonId",
                "invalid",
                400,
                "invalid_season_id",
                "seasonId 형식이 올바르지 않습니다.",
            ),
            (
                "seasonId",
                str(uuid.uuid7()),
                404,
                "season_not_found",
                "요청한 시즌을 찾을 수 없습니다.",
            ),
        )
    )
    cases.extend(
        (endpoint, params, 400, code, message)
        for endpoint in (base + "suggestions", public_path)
        for params, code, message in (
            (
                {"q": "D"},
                "invalid_search_query",
                "검색어를 두 글자 이상 입력해 주세요.",
            ),
            (
                {"q": "Docker", "limit": "9"},
                "invalid_search_limit",
                "검색 결과 개수 설정이 올바르지 않습니다.",
            ),
        )
    )
    for endpoint, params, status, code, message in cases:
        response = await client.get(endpoint, params=params, headers=headers)
        assert response.status_code == status, response.text
        error = response.json()["error"]
        assert (error["code"], error["message"]) == (code, message), error
        assert error["requestId"] == response.headers["x-request-id"]


async def records_smoke(
    client: httpx.AsyncClient,
    headers: dict[str, str],
    season_name: str,
    *,
    source: str | None = None,
    identifier: str | None = None,
    champion: str | None = None,
    season_id: str | None = None,
) -> None:
    params = {"q": "2001", "limit": "1"}
    if season_id is not None:
        params["seasonId"] = season_id
    response = await client.get(
        "/api/guilds/123/records/search", params=params, headers=headers
    )
    assert response.status_code == 200, response.text
    data = response.json()["data"]
    record_projection_smoke(data, public=False)
    selected = next(
        season for season in data["seasons"] if season["id"] == data["selectedSeasonId"]
    )
    assert selected["name"] == season_name
    if season_id is not None:
        assert selected["id"] == season_id and selected["status"] == "CLOSED"
        assert data["seasons"][0]["status"] == "ACTIVE"
    assert [lane["lane"] for lane in data["laneMmr"]] == [
        "TOP",
        "JUNGLE",
        "MID",
        "BOTTOM",
        "SUPPORT",
    ]
    assert data["nextCursor"] is None
    if source is None:
        assert data["summary"] == {"games": 0, "wins": 0, "losses": 0}
        assert data["records"] == data["championRecords"] == []
        assert all(
            lane["mmr"] == 1500 and lane["games"] == lane["wins"] == 0
            for lane in data["laneMmr"]
        )
        return
    assert len(data["records"]) == 1, data
    record = data["records"][0]
    assert (record["kind"], record["source"], record["id"]) == (
        "SERIES",
        source,
        identifier,
    )
    assert record["participantCount"] == 10 and len(record["games"]) == 1
    game = record["games"][0]
    assert game["gameNumber"] == 1 and game["playedAt"] == record["playedAt"]
    assert game["winningSide"] == ("BLUE" if source == "REGULAR" else "RED")
    target = game["target"]
    wins = int(target["side"] == game["winningSide"])
    assert data["summary"] == {"games": 1, "wins": wins, "losses": 1 - wins}
    assert record["wins"] == wins and record["losses"] == 1 - wins
    assert record["result"] == ("WIN" if wins else "LOSS")
    assert target["championId"] == champion
    assert target["mmrBefore"] is not None and target["mmrAfter"] is not None
    assert record["mmrDelta"] is not None and math.isclose(
        record["mmrDelta"], target["mmrAfter"] - target["mmrBefore"], abs_tol=1e-9
    )
    assert len(game["blueTeam"]) == len(game["redTeam"]) == 5
    assert sum(player["isTarget"] for player in game["blueTeam"] + game["redTeam"]) == 1
    assert game["blueBans"] == game["redBans"] == []
    assert record["stage"] == ("FINAL" if source == "AUCTION" else None)
    assert data["championRecords"] == (
        [
            {
                "championId": champion,
                "lane": target["lane"],
                "games": 1,
                "wins": wins,
                "losses": 1 - wins,
            }
        ]
        if champion
        else []
    )


async def operation_smoke(
    client: httpx.AsyncClient,
    headers: dict[str, str],
    identifier: str,
    expected: tuple[str, str, str] | None,
) -> None:
    response = await client.get("/api/guilds/123/operations", headers=headers)
    assert response.status_code == 200, response.text
    matching = [
        row for row in response.json()["data"]["operations"] if row["id"] == identifier
    ]
    if expected is None:
        assert matching == [], matching
        return
    assert len(matching) == 1, response.text
    row = matching[0]
    assert (row["kind"], row["status"], row["href"]) == expected, row
    assert row["priority"] == ("normal" if row["kind"] == "AUCTION" else "urgent")
    assert row["title"] and row["detail"] and row["updatedAt"]


async def statistics_smoke(
    client: httpx.AsyncClient,
    headers: dict[str, str],
    regular_games: int,
    auction_games: int,
    completed_tournaments: int = 0,
    season_id: str | None = None,
) -> None:
    for scope, regular, auction in (
        ("ALL", regular_games, auction_games),
        ("REGULAR", regular_games, 0),
        ("AUCTION", 0, auction_games),
    ):
        params = {"scope": scope}
        if season_id is not None:
            params["seasonId"] = season_id
        response = await client.get(
            "/api/guilds/123/statistics", params=params, headers=headers
        )
        assert response.status_code == 200, response.text
        data = response.json()["data"]
        assert data["scope"] == scope
        assert data["selectedSeasonId"] in {season["id"] for season in data["seasons"]}
        if season_id is not None:
            assert data["selectedSeasonId"] == season_id
        overview = data["overview"]
        assert overview["games"] == regular + auction, data
        assert (
            overview["regularGames"] == regular and overview["auctionGames"] == auction
        )
        assert overview["uniquePlayers"] == (10 if regular + auction else 0)
        assert overview["blueWins"] == regular and overview["redWins"] == auction
        assert overview["averageDurationSeconds"] is None
        assert overview["coverage"]["durationGames"] == 0
        assert data["players"]["minimumWinRateGames"] == 3
        assert data["players"]["bestWinRates"] == []
        assert data["auction"]["completedTournaments"] == (
            0 if scope == "REGULAR" else completed_tournaments
        )
        if scope == "REGULAR":
            assert data["auction"]["captains"] == data["auction"]["topBids"] == []
        assert not any(
            private in response.text
            for private in ("puuid", "riotId", "profileIconId", "showRiotIds")
        )


async def leaderboard_smoke(
    client: httpx.AsyncClient, headers: dict[str, str], season_name: str, played: bool
) -> None:
    response = await client.get("/api/guilds/123/leaderboard", headers=headers)
    assert response.status_code == 200, response.text
    data = response.json()["data"]
    assert data["season"]["name"] == season_name
    players = {player["id"]: player for player in data["players"]}
    assert set(players) == ({str(2000 + i) for i in range(1, 11)} if played else set())
    for player in players.values():
        assert len(player["stats"]) == 1 and player["mainRole"] in player["stats"]
        stat = player["stats"][player["mainRole"]]
        assert stat["games"] == 1 and stat["wins"] in (0, 1)
        assert player["compositeMmr"] == stat["mmr"] and not player["isMe"]
    if played:
        assert (
            players["2001"]["tag"] is None and players["2001"]["profileIconId"] is None
        )
        assert players["2002"]["tag"] == "Docker visible#KR1"
        assert players["2002"]["profileIconId"] == 123


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
                    INSERT INTO bot_notification_outbox
                        (id, guild_id, deduplication_key, discord_nonce,
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
    await operation_smoke(
        client,
        headers,
        "auction-recruitment:" + identifier,
        ("AUCTION", "팀장 설정", f"/g/123/auction/recruitments/{identifier}/draft"),
    )
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
    operation_id = "auction-tournament:" + state["tournament"]["id"]
    web_tournament = "/g/123/auction/tournaments/" + state["tournament"]["id"]
    await operation_smoke(client, headers, "auction-recruitment:" + identifier, None)
    await operation_smoke(
        client,
        headers,
        operation_id,
        ("AUCTION", "입찰 진행", web_tournament + "/draft"),
    )

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

    while state["tournament"]["status"] == "BIDDING":
        await draft_command("DRAW_NEXT")
        team = next(t for t in state["teams"] if len(t["members"]) < 5)
        await draft_command(
            "ASSIGN_CURRENT",
            memberId=state["draft"]["currentTarget"]["userId"],
            teamId=team["id"],
        )
    bracket_path = tournament_path + "/bracket"
    response = await client.get(bracket_path, headers=headers)
    assert response.status_code == 200, response.text
    state = response.json()["data"]

    async def bracket_command(kind, **extra):
        nonlocal state
        body = {
            "kind": kind,
            "expectedRevision": state["tournament"]["revision"],
            **extra,
        }
        command_headers = {
            **headers,
            "idempotency-key": "docker-bracket-" + str(body["expectedRevision"]),
        }
        response = await client.post(
            bracket_path + "/commands", json=body, headers=command_headers
        )
        assert response.status_code == 200, response.text
        state = response.json()["data"]
        replay = await client.post(
            bracket_path + "/commands", json=body, headers=command_headers
        )
        assert replay.status_code == 200 and replay.json()["data"] == state
        current = await client.get(bracket_path, headers=headers)
        assert current.status_code == 200 and current.json()["data"] == state

    await operation_smoke(
        client,
        headers,
        operation_id,
        ("AUCTION", "대진 설정", web_tournament + "/bracket"),
    )
    for slot, team in zip(("A_0", "A_1"), state["teams"], strict=True):
        await bracket_command("ASSIGN_TEAM", slotKey=slot, teamId=team["id"])
    await bracket_command("CLEAR_SLOT", slotKey="A_0")
    await bracket_command("ASSIGN_TEAM", slotKey="A_0", teamId=state["teams"][0]["id"])
    await bracket_command("LOCK_BRACKET")
    old_match = state["bracket"]["matches"][0]["id"]
    assert state["bracket"]["locked"]
    assert [m["matchKey"] for m in state["bracket"]["matches"]] == ["FINAL"]
    await bracket_command("UNLOCK_BRACKET")
    assert not state["bracket"]["locked"] and state["bracket"]["matches"] == []
    await bracket_command("LOCK_BRACKET")
    assert state["bracket"]["matches"][0]["id"] != old_match
    match_path = (
        "/api/guilds/123/auction-matches/" + state["bracket"]["matches"][0]["id"]
    )
    response = await client.get(match_path, headers=headers)
    assert response.status_code == 200, response.text
    match = response.json()["data"]

    async def match_command(method, suffix, **extra):
        nonlocal match
        body = {"expectedRevision": match["match"]["revision"], **extra}
        command_headers = {
            **headers,
            "idempotency-key": "docker-match-" + str(body["expectedRevision"]) + suffix,
        }
        response = await client.request(
            method, match_path + suffix, json=body, headers=command_headers
        )
        assert response.status_code == (201 if suffix == "/games" else 200), (
            response.text
        )
        match = response.json()["data"]
        replay = await client.request(
            method, match_path + suffix, json=body, headers=command_headers
        )
        assert (
            replay.status_code == response.status_code
            and replay.json()["data"] == match
        )

    for team in match["teams"]:
        for lane, member in zip(
            ("TOP", "JUNGLE", "MID", "BOTTOM", "SUPPORT"), team["members"], strict=True
        ):
            await match_command(
                "PATCH",
                "/lineup",
                teamId=team["id"],
                lane=lane,
                teamMemberId=member["teamMemberId"],
            )
    await match_command("POST", "/lineup/lock")
    assert (
        match["match"]["status"] == "READY" and match["permissions"]["canEnterResults"]
    )
    await operation_smoke(
        client,
        headers,
        operation_id,
        ("AUCTION", "결과 입력", "/g/123/auction/matches/" + match["match"]["id"]),
    )
    await match_command("POST", "/lineup/unlock")
    assert not match["match"]["lineupsLocked"]
    await match_command("POST", "/lineup/lock")
    picks = {
        member["userId"]: champion
        for member, champion in zip(
            match["lineups"],
            (
                "Aatrox",
                "Ahri",
                "Akali",
                "Alistar",
                "Amumu",
                "Anivia",
                "Annie",
                "Ashe",
                "Azir",
                "Bard",
            ),
            strict=True,
        )
    }
    await match_command(
        "POST",
        "/games",
        setNumber=1,
        sidePreset="SWAPPED",
        winner="RED",
        picks=picks,
        bans={"BLUE": [None] * 5, "RED": [None] * 5},
        patchVersion=None,
    )
    assert match["match"]["leftScore"] == 1 and len(match["games"]) == 1
    await match_command("POST", "/complete")
    assert match["match"]["status"] == "COMPLETED" and match["match"]["bestOf"] == 1
    result = await client.get(tournament_path + "/result", headers=headers)
    assert result.status_code == 200, result.text
    result = result.json()["data"]
    assert result["tournament"]["status"] == "COMPLETED"
    assert result["tournament"]["championTeamId"] == match["match"]["leftTeamId"]
    assert len(result["players"]) == 10 and all(
        p["games"] == 1 for p in result["players"]
    )
    await operation_smoke(client, headers, operation_id, None)
    await statistics_smoke(client, headers, 0, 1, completed_tournaments=1)
    await leaderboard_smoke(client, headers, "Docker next season", played=True)
    await records_smoke(
        client,
        headers,
        "Docker next season",
        source="AUCTION",
        identifier=match["match"]["id"],
        champion=picks["2001"],
    )
    await match_command("DELETE", "/games/last")
    assert match["match"]["status"] == "READY" and match["games"] == []
    preview = await client.get(tournament_path + "/cancel-preview", headers=headers)
    assert preview.status_code == 200, preview.text
    body = {
        "expectedRevision": preview.json()["data"]["tournament"]["revision"],
        "reason": "Docker cancellation smoke",
    }
    cancel_headers = {**headers, "idempotency-key": "docker-tournament-cancel"}
    cancelled = await client.post(
        tournament_path + "/cancel", json=body, headers=cancel_headers
    )
    assert cancelled.status_code == 200, cancelled.text
    assert cancelled.json()["data"]["tournamentStatus"] == "CANCELLED"
    replay = await client.post(
        tournament_path + "/cancel", json=body, headers=cancel_headers
    )
    assert (
        replay.status_code == 200 and replay.json()["data"] == cancelled.json()["data"]
    )
    archived = await client.get(tournament_path + "/result", headers=headers)
    assert archived.status_code == 200, archived.text
    assert len(archived.json()["data"]["matches"]) == 2
    await operation_smoke(client, headers, operation_id, None)
    await statistics_smoke(client, headers, 0, 0)
    await leaderboard_smoke(client, headers, "Docker next season", played=False)
    await records_smoke(client, headers, "Docker next season")


async def guild_admin_smoke(
    client: httpx.AsyncClient, headers: dict[str, str], operator_id: str
) -> None:
    base = "/api/guilds/123"
    members_path = base + "/members"
    target_path = members_path + "/2001"
    response = await client.get(members_path, headers=headers)
    assert response.status_code == 200, response.text
    original = response.json()["data"]
    members = {member["id"]: member for member in original["members"]}
    assert members[operator_id]["role"] == "OPERATOR"
    member = members["2001"]
    assert member["identityKind"] == "DISCORD" and member["role"] == "MEMBER"
    assert member["status"] == "ACTIVE"
    assert set(member["laneMmr"]) == {"TOP", "JUNGLE", "MID", "BOTTOM", "SUPPORT"}

    adjustment = {
        "lane": "TOP",
        "delta": 25,
        "note": "  Docker manual adjustment  ",
        "expectedRevision": original["revision"],
    }
    adjustment_headers = {**headers, "idempotency-key": "docker-member-mmr"}
    adjusted = await client.patch(
        target_path + "/mmr", json=adjustment, headers=adjustment_headers
    )
    assert adjusted.status_code == 200, adjusted.text
    snapshot = adjusted.json()["data"]
    assert snapshot["revision"] == original["revision"] + 1
    assert snapshot["updatedBy"] == operator_id
    updated_member = next(row for row in snapshot["members"] if row["id"] == "2001")
    assert updated_member["laneMmr"] == {
        **member["laneMmr"],
        "TOP": member["laneMmr"]["TOP"] + 25,
    }
    replay = await client.patch(
        target_path + "/mmr", json=adjustment, headers=adjustment_headers
    )
    assert replay.status_code == 200 and replay.json()["data"] == snapshot, replay.text

    status_request_ids = []
    for status in ("INACTIVE", "ACTIVE"):
        response = await client.patch(
            target_path + "/status",
            json={"status": status, "expectedRevision": snapshot["revision"]},
            headers=headers,
        )
        assert response.status_code == 200, response.text
        current = response.json()["data"]
        assert current["revision"] == snapshot["revision"] + 1
        assert current["updatedBy"] == operator_id
        current_member = next(row for row in current["members"] if row["id"] == "2001")
        assert current_member == {**updated_member, "status": status}
        status_request_ids.append(response.headers["x-request-id"])
        snapshot = current

    denied = await client.patch(
        members_path + f"/{operator_id}/status",
        json={"status": "INACTIVE", "expectedRevision": snapshot["revision"]},
        headers=headers,
    )
    assert denied.status_code == 409, denied.text
    assert denied.json()["error"] == {
        "code": "self_deactivation_forbidden",
        "message": "자기 자신은 비활성화할 수 없습니다.",
        "requestId": denied.headers["x-request-id"],
    }
    refreshed = await client.get(members_path, headers=headers)
    assert refreshed.status_code == 200 and refreshed.json()["data"] == snapshot

    audit_path = base + "/audit-logs"
    response = await client.get(audit_path, params={"limit": 2}, headers=headers)
    assert response.status_code == 200, response.text
    first = response.json()["data"]
    assert len(first["entries"]) == 2 and first["nextCursor"]
    for entry, before, after, request_id in zip(
        first["entries"],
        ("INACTIVE", "ACTIVE"),
        ("ACTIVE", "INACTIVE"),
        reversed(status_request_ids),
        strict=True,
    ):
        assert entry["action"] == "MEMBER_STATUS_CHANGED"
        assert entry["targetType"] == "GUILD_MEMBER"
        assert entry["beforeSummary"] == {"status": before}
        assert entry["afterSummary"] == {"status": after}
        assert entry["requestId"] == request_id
    response = await client.get(
        audit_path,
        params={"limit": 1, "cursor": first["nextCursor"]},
        headers=headers,
    )
    assert response.status_code == 200, response.text
    second = response.json()["data"]
    assert len(second["entries"]) == 1
    entry = second["entries"][0]
    assert entry["id"] not in {row["id"] for row in first["entries"]}
    assert entry["action"] == "MEMBER_MMR_ADJUSTED"
    assert entry["targetType"] == "GUILD_MEMBER_MMR"
    assert entry["note"] == "Docker manual adjustment"
    assert entry["requestId"] == adjusted.headers["x-request-id"]
    assert entry["beforeSummary"]["lane"] == entry["afterSummary"]["lane"] == "TOP"
    assert entry["beforeSummary"]["mmr"] == member["laneMmr"]["TOP"]
    assert entry["afterSummary"]["mmr"] == updated_member["laneMmr"]["TOP"]
    assert entry["afterSummary"]["delta"] == 25
    assert (
        entry["afterSummary"]["laneRevision"]
        == entry["beforeSummary"]["laneRevision"] + 1
    )
    for entry in first["entries"] + second["entries"]:
        assert entry["principalType"] == "USER" and entry["principalId"] == operator_id
        assert entry["principalDisplayName"] == members[operator_id]["discordName"]
        assert entry["targetId"] == "2001"
        assert entry["targetDisplayName"] == member["discordName"]
        assert entry["checkpointId"] is None and entry["result"] == "APPLIED"
        assert entry["createdAt"]
    invalid = await client.get(
        audit_path, params={"cursor": "invalid"}, headers=headers
    )
    assert invalid.status_code == 400, invalid.text
    assert invalid.json()["error"] == {
        "code": "invalid_cursor",
        "message": "cursor 형식이 올바르지 않습니다.",
        "requestId": invalid.headers["x-request-id"],
    }


async def full_data_export_smoke(
    client: httpx.AsyncClient, headers: dict[str, str]
) -> None:
    path = "/api/guilds/123/full-data-export"
    for response, status, code, message in (
        (
            await client.post(path),
            401,
            "unauthorized",
            "인증이 필요합니다.",
        ),
        (
            await client.post(path, headers={"cookie": headers["cookie"]}),
            403,
            "csrf_validation_failed",
            "요청 출처를 확인할 수 없습니다.",
        ),
    ):
        assert response.status_code == status, response.text
        assert response.json()["error"] == {
            "code": code,
            "message": message,
            "requestId": response.headers["x-request-id"],
        }
    response = await client.post(path, headers=headers)
    assert response.status_code == 200, response.text
    request_id = response.headers["x-request-id"]
    assert response.json()["meta"]["requestId"] == request_id
    data = response.json()["data"]
    archive = data["archive"]
    assert archive["format"] == "MONKEYBOT_DATA_TRANSFER"
    assert archive["kind"] == "GUILD_FULL_SNAPSHOT" and archive["schemaVersion"] == 1
    assert archive["source"]["guildId"] == "123" and archive["source"]["guildName"]
    assert archive["exportedAt"]
    assert data["fileName"] == "monkeybot-full-123-v1.json"
    tables = archive["tables"]
    assert len(tables) == 41
    assert not {
        "user_sessions",
        "oauth_login_attempts",
        "guild_config",
        "guild_discord_roles",
        "guild_member_discord_roles",
        "bot_notification_outbox",
        "idempotency_records",
    }.intersection(tables)
    assert data["summary"]["totalRows"] == sum(len(rows) for rows in tables.values())
    for table in (
        "guild_members",
        "seasons",
        "recruitments",
        "series",
        "auction_tournaments",
    ):
        assert tables[table], table
        assert all(row["guild_id"] == "123" for row in tables[table]), table
    assert all(row["checkpoint_id"] is None for row in tables["admin_audit_log"])
    assert not any(row["request_id"] == request_id for row in tables["admin_audit_log"])
    audit = await client.get("/api/guilds/123/audit-logs?limit=1", headers=headers)
    assert audit.status_code == 200, audit.text
    entry = audit.json()["data"]["entries"][0]
    assert entry["action"] == "FULL_DATA_TRANSFER_EXPORTED"
    assert entry["requestId"] == request_id and entry["afterSummary"] == data["summary"]


async def tournament_finish_smoke(
    client: httpx.AsyncClient, database_url: str, headers: dict[str, str]
) -> None:
    async def command(method: str, path: str, body: dict) -> dict:
        response = await client.request(
            method,
            path,
            json=body,
            headers={
                **headers,
                "idempotency-key": "docker-finish-" + str(uuid.uuid7()),
            },
        )
        assert response.status_code in (200, 201), response.text
        return response.json()["data"]

    base = "/api/guilds/123/auction-recruitments"
    source = await command("POST", base, {"format": "TEN_PERSON"})
    source_path = base + "/" + source["recruitment"]["id"]
    for i in range(1, 11):
        await command(
            "POST",
            source_path + "/members",
            {
                "memberId": str(2000 + i),
                "declaredLane": ["TOP", "JUNGLE", "MID", "BOTTOM", "SUPPORT"][
                    (i - 1) % 5
                ],
            },
        )
    await command("POST", source_path + "/close", {})
    response = await client.get(source_path + "/setup-draft", headers=headers)
    assert response.status_code == 200, response.text
    setup = await command(
        "PATCH",
        source_path + "/setup-draft",
        {
            "expectedRevision": response.json()["data"]["draft"]["revision"],
            "teams": [
                {
                    "teamIndex": i,
                    "teamName": f"Finish team {i}",
                    "captainUserId": str(2000 + i),
                    "initialPoints": 1000,
                }
                for i in (1, 2)
            ],
        },
    )
    tournament = await command(
        "POST",
        "/api/guilds/123/auction-tournaments",
        {
            "sourceRecruitmentId": source["recruitment"]["id"],
            "expectedRevision": setup["draft"]["revision"],
        },
    )
    identifier = tournament["tournament"]["id"]
    path = "/api/guilds/123/auction-tournaments/" + identifier
    anonymous = await client.get(path + "/finish-preview")
    assert anonymous.status_code == 401, anonymous.text
    assert anonymous.json()["error"] == {
        "code": "unauthorized",
        "message": "인증이 필요합니다.",
        "requestId": anonymous.headers["x-request-id"],
    }
    preview = await client.get(path + "/finish-preview", headers=headers)
    assert preview.status_code == 200, preview.text
    record = preview.json()["data"]["tournament"]
    assert record["status"] == "BIDDING"
    head = await client.head(path + "/finish-preview", headers=headers)
    assert head.status_code == 200 and not head.content
    connection = await asyncpg.connect(database_url)
    try:
        await connection.execute(
            "UPDATE guild_config SET result_channel_id='789' WHERE guild_id='123'"
        )
        body = {
            "expectedRevision": record["revision"],
            "reason": "Docker manual completion",
        }
        finish_headers = {**headers, "idempotency-key": "docker-finish-completion"}
        response = await client.post(
            path + "/finish", json=body, headers=finish_headers
        )
        assert response.status_code == 200, response.text
        result = response.json()["data"]
        assert result == {
            "tournamentId": identifier,
            "tournamentStatus": "COMPLETED",
            "tournamentRevision": record["revision"] + 1,
            "championTeamId": None,
        }
        replay = await client.post(path + "/finish", json=body, headers=finish_headers)
        assert replay.status_code == 200 and replay.json()["data"] == result, (
            replay.text
        )
        response = await client.get(path + "/result", headers=headers)
        assert response.status_code == 200, response.text
        snapshot = response.json()["data"]
        assert (
            snapshot["tournament"]["endedEarly"]
            and snapshot["tournament"]["championTeamId"] is None
        )
        assert snapshot["matches"] == [] and not any(snapshot["permissions"].values())
        source = await client.get(source_path, headers=headers)
        assert source.json()["data"]["recruitment"]["status"] == "CONVERTED"
        payload = json.loads(
            await connection.fetchval(
                "SELECT payload FROM bot_notification_outbox WHERE guild_id='123' "
                "AND notification_type='AUCTION_TOURNAMENT_RESULT_CREATE' "
                "AND payload->>'tournamentId'=$1",
                identifier,
            )
        )
        assert payload["tournament"]["endedEarly"] is True
        assert payload["champion"] is None and payload["matches"] == []
        assert (
            await connection.fetchval(
                "SELECT count(*) FROM admin_audit_log WHERE guild_id='123' "
                "AND target_id=$1 AND action='AUCTION_TOURNAMENT_FINISHED'",
                identifier,
            )
            == 1
        )
    finally:
        await connection.close()


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
        imported_id = str(uuid.uuid7())
        migrated_routes = (
            ("POST", "/api/guilds/123/full-data-imports/preview", 401, "unauthorized"),
            (
                "POST",
                f"/api/guilds/123/full-data-imports/{imported_id}/apply",
                401,
                "unauthorized",
            ),
            ("POST", "/api/guilds/123/data-export", 401, "unauthorized"),
            ("POST", "/api/guilds/123/data-imports/preview", 401, "unauthorized"),
            ("POST", f"/api/guilds/123/data-imports/{imported_id}/apply", 401, "unauthorized"),
            ("POST", "/internal/guilds/123/setup", 401, "unauthorized"),
            ("GET", "/api/admin/status", 401, "unauthorized"),
            ("GET", "/api/me/riot-accounts", 401, "unauthorized"),
            (
                "GET",
                "/api/riot/records?gameName=Longcat&tagLine=KR1&platform=KR",
                503,
                "riot_unavailable",
            ),
            ("GET", "/api/dev/dummy-users", 404, "not_found"),
        )
        for method, path, status, code in migrated_routes:
            response = await client.request(method, path)
            assert response.status_code == status, (path, response.text)
            assert response.json()["error"]["code"] == code, (path, response.text)
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
        for endpoint in ("operations", "leaderboard", "statistics"):
            path = "/api/guilds/123/" + endpoint
            anonymous = await client.get(path)
            assert anonymous.status_code == 401, anonymous.text
            assert anonymous.json()["error"] == {
                "code": "unauthorized",
                "message": "인증이 필요합니다.",
                "requestId": anonymous.headers["x-request-id"],
            }
            hidden_read = await client.get(path, headers=auth_headers)
            assert hidden_read.status_code == 404, hidden_read.text
            assert hidden_read.json()["error"] == {
                "code": "not_found",
                "message": "요청한 길드를 찾을 수 없습니다.",
                "requestId": hidden_read.headers["x-request-id"],
            }
        for endpoint in ("profile", "search", "suggestions"):
            path = "/api/guilds/123/records/" + endpoint
            anonymous = await client.get(path, params={"q": "Docker"})
            assert anonymous.status_code == 401, anonymous.text
            assert anonymous.json()["error"] == {
                "code": "unauthorized",
                "message": "인증이 필요합니다.",
                "requestId": anonymous.headers["x-request-id"],
            }
            hidden_read = await client.get(
                path, params={"q": "Docker"}, headers=auth_headers
            )
            assert hidden_read.status_code == 404, hidden_read.text
            assert hidden_read.json()["error"] == {
                "code": "not_found",
                "message": (
                    "요청한 길드를 찾을 수 없습니다."
                    if endpoint == "suggestions"
                    else "요청한 길드 멤버를 찾을 수 없습니다."
                ),
                "requestId": hidden_read.headers["x-request-id"],
            }
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
        for endpoint in ("operations", "leaderboard", "statistics"):
            head = await client.head(
                "/api/guilds/123/" + endpoint, headers=auth_headers
            )
            assert head.status_code == 200 and head.content == b"", endpoint
        await leaderboard_smoke(client, auth_headers, "Docker season", played=False)
        await statistics_smoke(client, auth_headers, 0, 0)
        for params, status, code, message in (
            (
                {"scope": "invalid"},
                400,
                "invalid_scope",
                "scope은 ALL, REGULAR, AUCTION 중 하나여야 합니다.",
            ),
            (
                {"seasonId": "invalid"},
                400,
                "invalid_season_id",
                "seasonId 형식이 올바르지 않습니다.",
            ),
            (
                {"seasonId": str(uuid.uuid7())},
                404,
                "season_not_found",
                "요청한 시즌을 찾을 수 없습니다.",
            ),
        ):
            invalid = await client.get(
                "/api/guilds/123/statistics", params=params, headers=auth_headers
            )
            assert invalid.status_code == status, invalid.text
            error = invalid.json()["error"]
            assert (error["code"], error["message"]) == (code, message)
            assert error["requestId"] == invalid.headers["x-request-id"]
            if code == "invalid_scope":
                assert error["details"] == {"allowed": ["ALL", "REGULAR", "AUCTION"]}
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
                    "INSERT INTO users "
                    "(user_id, identity_kind, display_name, created_at, updated_at) "
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
                    "INSERT INTO user_privacy_settings "
                    "(user_id, public_profile_enabled, show_riot_ids, "
                    "allow_external_record_search, updated_at) "
                    "VALUES ('2001', false, false, false, now()), "
                    "('2002', true, true, false, now())"
                )
                await connection.execute(
                    "INSERT INTO riot_accounts (puuid, user_id, platform, routing_region, "
                    "game_name, tag_line, normalized_game_name, normalized_tag_line, "
                    "profile_icon_id, is_main, created_at, updated_at) "
                    "VALUES ('docker-private-puuid', '2001', 'KR', 'ASIA', "
                    "'Docker private', 'KR1', 'docker private', 'kr1', 456, true, now(), now()), "
                    "('docker-visible-puuid', '2002', 'KR', 'ASIA', "
                    "'Docker visible', 'KR1', 'docker visible', 'kr1', 123, true, now(), now())"
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
        await record_identity_smoke(client, auth_headers, user_id)
        await records_smoke(client, auth_headers, "Docker season")
        closed = await client.post(
            recruitment_path + "/close",
            json={},
            headers={**csrf_headers, "idempotency-key": "docker-close"},
        )
        assert closed.status_code == 200, closed.text
        assert closed.json()["data"]["recruitment"]["status"] == "CLOSED"
        await operation_smoke(
            client,
            auth_headers,
            "entry:" + recruitment_id,
            ("ENTRY", "편성 필요", f"/g/123/recruitments/{recruitment_id}/entry"),
        )
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
        series_id = series.json()["data"]["series"]["id"]
        await operation_smoke(client, auth_headers, "entry:" + recruitment_id, None)
        await operation_smoke(
            client,
            auth_headers,
            "series:" + series_id,
            ("PICK_BAN", "픽밴/결과 입력", f"/g/123/series/{series_id}/pick-ban"),
        )
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
        await operation_smoke(client, auth_headers, "series:" + series_id, None)
        await statistics_smoke(client, auth_headers, 1, 0)
        await leaderboard_smoke(client, auth_headers, "Docker season", played=True)
        await records_smoke(
            client,
            auth_headers,
            "Docker season",
            source="REGULAR",
            identifier=series_id,
        )
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
        await operation_smoke(
            client,
            auth_headers,
            "series:" + second.json()["data"]["series"]["id"],
            None,
        )
        await statistics_smoke(client, auth_headers, 0, 0)
        await leaderboard_smoke(client, auth_headers, "Docker season", played=False)
        await records_smoke(client, auth_headers, "Docker season")

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
        await leaderboard_smoke(
            client, auth_headers, "Docker next season", played=False
        )
        await statistics_smoke(
            client, auth_headers, 0, 0, season_id=season_body["expectedCurrentSeasonId"]
        )
        await records_smoke(client, auth_headers, "Docker next season")
        await records_smoke(
            client,
            auth_headers,
            "Docker season",
            season_id=season_body["expectedCurrentSeasonId"],
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

        force_url = (
            "/api/guilds/123/auction-recruitments/"
            + data["recruitment"]["id"]
            + "/force-cancel"
        )
        force_body = {
            "actor": {
                "id": user_id,
                "displayName": "Docker member",
                "globalDisplayName": "Docker member",
                "roleIds": ["456"],
            }
        }
        force_headers = {
            "authorization": "Bearer isolated-bot-service-token-32-characters",
            "idempotency-key": "2000003",
        }
        forced = await client.post(force_url, json=force_body, headers=force_headers)
        assert forced.status_code == 200, forced.text
        assert forced.json()["data"]["recruitmentStatus"] == "CANCELLED"
        replay = await client.post(force_url, json=force_body, headers=force_headers)
        assert (
            replay.status_code == 200 and replay.json()["data"] == forced.json()["data"]
        )

        # Administrative changes run after all game workflows and restore membership status.
        await guild_admin_smoke(client, csrf_headers, user_id)
        await tournament_finish_smoke(client, database_url, csrf_headers)
        await full_data_export_smoke(client, csrf_headers)

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
        "profile/privacy, champion preferences, members/MMR/replay/status/audit cursor/denial, "
        "operations queue lifecycle/href/HEAD, leaderboard played lanes/privacy/current season, "
        "statistics ALL/REGULAR/AUCTION/reversals/closed season/validation, "
        "records profile/search/HEAD/privacy/Riot normalization/ambiguity/game detail/reversal/season, "
        "guild/public Riot suggestions/privacy/limits/HEAD, "
        "full-data export/archive/tenant isolation/secret exclusions/audit/CSRF, "
        "manual tournament finish/preview/HEAD/replay/no champion/nullable notification, "
        "recruitment create/replay/detail/join/leave/"
        "cancel/restore/close/reopen/entry/edit/reset/preferred-lanes/bind/DM/series/list/detail/"
        "pickban/games/result/revert/complete/delete, notification claim/ack/typed-ack/nack/"
        "heartbeat, catalog/settings/seasons/rotation/replay, "
        "auction recruitment 12 operations/typed ACK/replay, "
        "entry-to-auction conversion/replay, tournament creation/list/detail/9 draft commands/"
        "Rust reset grant failure, bracket/lineup/game/MMR/result/early-complete/revert/cancel/"
        "force-cancel/replay, new route registration/flag denial, OAuth denial/replay, "
        "CSRF/logout passed."
    )


if __name__ == "__main__":
    asyncio.run(main())
