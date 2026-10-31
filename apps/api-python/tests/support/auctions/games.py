"""Reusable fixtures and request builders; no test module imports."""

import uuid

from monkeybot_api.schemas.auctions.games import MESSAGES
from tests.support.assertions import assert_failure
from tests.support.auctions.lineups import fill, run, seeded
from tests.support.guilds.context import GUILD_A

PATH = f"/api/guilds/{GUILD_A}/auction-matches"


async def prepared(owner, client, count=10):
    h, bracket, state = await seeded(owner, client, count)
    state = await fill(client, h, state)
    state = await run(client, h, state, "LOCK")
    return h, bracket, state


def body(state, **updates):
    return {
        "expectedRevision": state["match"]["revision"],
        "setNumber": len(state["games"]) + 1,
        "sidePreset": "DEFAULT",
        "winner": "BLUE",
        "picks": {r["userId"]: None for r in state["lineups"]},
        "bans": {"BLUE": [None] * 5, "RED": [None] * 5},
        "patchVersion": None,
        **updates,
    }


async def command(client, h, state, action="submit", *, key=None, payload=None, **updates):
    method, suffix = {
        "submit": ("POST", "/games"),
        "last": ("DELETE", "/games/last"),
        "complete": ("POST", "/complete"),
    }[action]
    if payload is None:
        payload = (
            body(state, **updates)
            if action == "submit"
            else {
                "expectedRevision": state["match"]["revision"],
                **updates,
            }
        )
    return await client.request(
        method,
        f"{PATH}/{state['match']['id']}{suffix}",
        json=payload,
        headers={**h, "idempotency-key": key or str(uuid.uuid7())},
    )


async def play(client, h, state, **updates):
    response = await command(client, h, state, **updates)
    assert response.status_code == 201, response.text
    return response.json()["data"]


async def changed(client, h, state, action, **updates):
    response = await command(client, h, state, action, **updates)
    assert response.status_code == 200, response.text
    return response.json()["data"]


def failure(response, code, status=409):
    assert_failure(response, status, code, MESSAGES[code])
