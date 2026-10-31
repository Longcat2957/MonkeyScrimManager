"""Reusable fixtures and request builders; no test module imports."""

import uuid

from sqlalchemy import text

from tests.support.auctions.bidding import PATH, prepared
from tests.support.auctions.bidding import request as draft

SLOTS = ("A_0", "A_1", "B_0", "B_1")


async def ready(owner, client, count=10):
    headers, source = await prepared(owner, client, count)
    async with owner.begin() as c:
        await c.execute(text("UPDATE auction_recruitments SET final_best_of=5,hard_fearless=false"))
        if count == 20:
            await c.execute(text("UPDATE auction_recruitments SET semifinal_best_of=1"))
    state = await draft(client, headers, None, source)
    identifier = state["tournament"]["id"]
    for team in state["teams"]:
        for _ in range(4):
            state = await draft(
                client,
                headers,
                identifier,
                {"kind": "DRAW_NEXT", "expectedRevision": state["tournament"]["revision"]},
            )
            state = await draft(
                client,
                headers,
                identifier,
                {
                    "kind": "ASSIGN_CURRENT",
                    "expectedRevision": state["tournament"]["revision"],
                    "memberId": state["draft"]["currentTarget"]["userId"],
                    "teamId": team["id"],
                },
            )
    response = await client.get(f"{PATH}/{identifier}/bracket")
    assert response.status_code == 200, response.text
    return headers, response.json()["data"]


async def command(client, headers, state, kind, key=None, **extra):
    return await client.post(
        f"{PATH}/{state['tournament']['id']}/bracket/commands",
        json={"kind": kind, "expectedRevision": state["tournament"]["revision"], **extra},
        headers={**headers, "idempotency-key": key or str(uuid.uuid7())},
    )


async def run(client, headers, state, kind, **extra):
    response = await command(client, headers, state, kind, **extra)
    assert response.status_code == 200, response.text
    return response.json()["data"]


async def assigned(owner, client, count=10):
    h, state = await ready(owner, client, count)
    for slot, team in zip(SLOTS, state["teams"], strict=False):
        state = await run(client, h, state, "ASSIGN_TEAM", slotKey=slot, teamId=team["id"])
    return h, state
