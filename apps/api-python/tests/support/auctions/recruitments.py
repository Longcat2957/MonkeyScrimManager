"""Reusable fixtures and request builders; no test module imports."""

import asyncio
import uuid
from typing import Any

import httpx
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncEngine

from tests.support.auth.crypto import rust_auth
from tests.support.guilds.context import GUILD_A, ROLE_A, USER_ID
from tests.support.recruitments.creation import BOT, START
from tests.support.recruitments.creation import setup as base_setup

PATH = f"/api/guilds/{GUILD_A}/auction-recruitments"


BODY = {"format": "TEN_PERSON", "expectedStartAt": START}


ROSTER = [str(1000 + i) for i in range(20)]


LANES = ["TOP", "JUNGLE", "MID", "BOTTOM", "SUPPORT"]


async def setup(owner: AsyncEngine, client: httpx.AsyncClient) -> dict[str, str]:
    headers = await base_setup(owner, client)
    async with owner.begin() as c:
        await c.execute(
            text("INSERT INTO guild_counters VALUES (:g,'AUCTION_RECRUITMENT',1)"), {"g": GUILD_A}
        )
        await c.execute(
            text("INSERT INTO guild_member_discord_roles VALUES (:g,:u,:r,now())"),
            {"g": GUILD_A, "u": USER_ID, "r": ROLE_A},
        )
        await c.execute(
            text("""INSERT INTO users (user_id,identity_kind,display_name,created_at,updated_at)
            SELECT n::text,'DISCORD','선수 '||n,now(),now()
            FROM generate_series(1000,1019) n""")
        )
        await c.execute(
            text("""INSERT INTO user_discord_identities
            (user_id,discord_id,created_at,updated_at)
            SELECT n::text,n::text,now(),now() FROM generate_series(1000,1019) n""")
        )
        await c.execute(
            text("""INSERT INTO guild_members (guild_id,user_id,display_name,
                status,main_lane,first_seen_at)
            SELECT :g,n::text,'선수 '||n,'ACTIVE','MID',now() FROM generate_series(1000,1019) n"""),
            {"g": GUILD_A},
        )
    return headers


async def call(
    client: httpx.AsyncClient,
    headers: dict[str, str],
    action: str,
    identifier: str | None = None,
    body: dict[str, Any] | None = None,
    key: str | None = None,
    bot: bool = False,
) -> dict[str, Any]:
    body = body or {}
    suffix = {
        "create": "",
        "cancel": "",
        "join": "/members",
        "leave": "/members/" + str(body.get("memberId")),
        "setup": "/setup-draft",
        "dm": "/participant-confirmation-dms",
    }.get(action, "/" + action)
    method = "DELETE" if action in {"cancel", "leave"} else "PATCH" if action == "setup" else "POST"
    sent = {k: v for k, v in body.items() if action != "leave" or k != "memberId"}
    response = await client.request(
        method,
        PATH + ("/" + identifier if identifier else "") + suffix,
        json=sent,
        headers={
            **headers,
            "idempotency-key": key or str(uuid.uuid7()),
            **({"authorization": f"Bearer {BOT}"} if bot else {}),
        },
    )
    assert response.status_code == (201 if action == "create" else 200), response.text
    return response.json()["data"]


async def rust(
    action: str, identifier: str | None, body: dict[str, Any], key: str, bot: bool = False
) -> dict[str, Any]:
    return await asyncio.to_thread(
        rust_auth,
        {
            "operation": "auction_recruitment",
            "guild_id": GUILD_A,
            "user_id": USER_ID,
            "recruitment_id": identifier,
            "action": action,
            "body": body,
            "key": key,
            "bot": bot,
        },
    )


async def filled(
    owner: AsyncEngine, client: httpx.AsyncClient, *, count: int = 10
) -> tuple[dict[str, str], str]:
    headers = await setup(owner, client)
    created = await call(
        client,
        headers,
        "create",
        body={**BODY, "format": "TEN_PERSON" if count == 10 else "TWENTY_PERSON"},
    )
    identifier = created["recruitment"]["id"]
    for i, user in enumerate(ROSTER[:count]):
        await call(
            client, headers, "join", identifier, {"memberId": user, "declaredLane": LANES[i % 5]}
        )
    return headers, identifier


def teams(count: int = 2) -> list[dict[str, Any]]:
    return [
        {
            "teamIndex": i + 1,
            "teamName": f"  팀장 {i + 1}\u3000",
            "captainUserId": ROSTER[i],
            "initialPoints": 1000 + i,
        }
        for i in range(count)
    ]
