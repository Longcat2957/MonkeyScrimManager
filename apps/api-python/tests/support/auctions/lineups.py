"""Reusable fixtures and request builders; no test module imports."""

import uuid

from monkeybot_api.schemas.auctions.lineups import ERRORS
from monkeybot_api.schemas.users import LANES
from tests.support.assertions import assert_failure
from tests.support.auctions.bracket import assigned
from tests.support.auctions.bracket import run as bracket_command
from tests.support.guilds.context import GUILD_A

PATH = f"/api/guilds/{GUILD_A}/auction-matches"


async def seeded(owner, client, count=10):
    h, bracket = await assigned(owner, client, count)
    bracket = await bracket_command(client, h, bracket, "LOCK_BRACKET")
    response = await client.get(f"{PATH}/{bracket['bracket']['matches'][0]['id']}")
    assert response.status_code == 200, response.text
    return h, bracket, response.json()["data"]


async def command(client, headers, state, kind, key=None, **extra):
    suffix = "" if kind == "ASSIGN" else "/" + kind.lower()
    return await client.request(
        "PATCH" if kind == "ASSIGN" else "POST",
        f"{PATH}/{state['match']['id']}/lineup{suffix}",
        json={"expectedRevision": state["match"]["revision"], **extra},
        headers={**headers, "idempotency-key": key or str(uuid.uuid7())},
    )


async def run(client, headers, state, kind, key=None, **extra):
    response = await command(client, headers, state, kind, key, **extra)
    assert response.status_code == 200, response.text
    return response.json()["data"]


async def fill(client, headers, state):
    for team in state["teams"]:
        for lane, member in zip(LANES, team["members"], strict=True):
            state = await run(
                client,
                headers,
                state,
                "ASSIGN",
                teamId=team["id"],
                lane=lane,
                teamMemberId=member["teamMemberId"],
            )
    return state


def denied(response, code):
    status, message = ERRORS[code]
    assert_failure(response, status, code, message)
