"""Reusable fixtures and request builders; no test module imports."""

from sqlalchemy import text

from tests.support.guilds.context import GUILD_A
from tests.support.recruitments.delivery import series, setup_series
from tests.support.recruitments.entries import reference


async def setup(owner, client, best_of=3, fearless=True):
    path, headers = await setup_series(owner, client)
    async with owner.begin() as c:
        await c.execute(
            text("UPDATE recruitments SET best_of=:best,hard_fearless=:fearless"),
            {"best": best_of, "fearless": fearless},
        )
        await c.execute(text("UPDATE guild_config SET result_channel_id='999'"))
    created = await series(client, path, headers)
    assert created.status_code == 201
    return f"/api/guilds/{GUILD_A}/series/{created.json()['data']['series']['id']}", headers, path


async def command(client, path, headers, action, revision, *, body=None, key=None):
    suffix = {
        "pickban": "/pickban",
        "submit": "/games",
        "last": "/games/last",
        "complete": "/complete",
        "revert": "/revert",
        "delete": "",
    }[action]
    method = (
        "PATCH" if action == "pickban" else "DELETE" if action in {"last", "delete"} else "POST"
    )
    data = {"expectedRevision": revision, **(body or {})}
    return await client.request(
        method,
        path + suffix,
        json=data,
        headers={**headers, "idempotency-key": key or f"{action}-{revision}"},
    )


def game(number=1, side="DEFAULT", win="BLUE", picks=None):
    return {
        "setNumber": number,
        "sidePreset": side,
        "winner": win,
        "picks": picks,
        "bans": {"BLUE": [None] * 5, "RED": [None] * 5},
        "patchVersion": "26.18",
    }


async def rust(path, action, rev=0, body=None, key=None):
    return await reference(
        path, "series_" + action, rev, key or f"{action}-{rev}", entry_operation=body
    )
