"""Reusable fixtures and request builders; no test module imports."""

from sqlalchemy import text

from tests.support.guilds.context import GUILD_A
from tests.support.recruitments.creation import START
from tests.support.recruitments.lifecycle import call, fixture

BODY = {"expectedRevision": 1, "expectedStartAt": START}


async def setup(owner, client, assigned=10):
    path, headers = await fixture(owner, client)
    assert (await call(client, path, headers, "close")).status_code == 200
    async with owner.begin() as c:
        await c.execute(
            text("INSERT INTO guild_counters VALUES (:guild,'AUCTION_RECRUITMENT',1)"),
            {"guild": GUILD_A},
        )
        await c.execute(
            text("DELETE FROM recruitment_entry_assignments WHERE user_id > :last"),
            {"last": str(1000 + assigned)},
        )
        await c.execute(
            text("""
            INSERT INTO user_lane_mmr
                (guild_id,season_id,user_id,lane,mmr,games_played,wins,revision,updated_at)
            SELECT guild_id,season_id,'1001','TOP',1732.5,12,8,3,now() FROM recruitments
        """)
        )
    return path, headers


async def convert(client, path, headers, body=None, key="convert-1"):
    return await client.post(
        path + "/auction-recruitment",
        json=BODY if body is None else body,
        headers={**headers, "idempotency-key": key},
    )
