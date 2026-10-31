"""Reusable fixtures and request builders; no test module imports."""

from sqlalchemy import text

from tests.support.guilds.context import GUILD_A, USER_ID
from tests.support.recruitments.creation import BOT
from tests.support.recruitments.lifecycle import call, fixture

ACTOR = {"id": USER_ID, "globalDisplayName": "Creator", "displayName": "Creator", "roleIds": []}


BIND = {"channelId": "789", "messageId": "987"}


async def bind(client, path, actor=None, key="10000001", body=None):
    return await client.put(
        path + "/message",
        json={"actor": actor or ACTOR, **(body or BIND)},
        headers={"authorization": f"Bearer {BOT}", "idempotency-key": key},
    )


async def dm(client, path, headers, key="dm-1"):
    return await client.post(
        path + "/participant-confirmation-dms", json={}, headers={**headers, "idempotency-key": key}
    )


async def series(client, path, headers, key="series-1", revision=1):
    return await client.post(
        f"/api/guilds/{GUILD_A}/series",
        json={"sourceRecruitmentId": path.split("/")[-1], "expectedRevision": revision},
        headers={**headers, "idempotency-key": key},
    )


async def setup_series(owner, client, *, entry_channel=True):
    path, headers = await fixture(owner, client)
    await call(client, path, headers, "close")
    async with owner.begin() as c:
        await c.execute(
            text(
                "INSERT INTO guild_counters (guild_id, counter_key, next_value) "
                "VALUES (:guild, 'SERIES', 1)"
            ),
            {"guild": GUILD_A},
        )
        if entry_channel:
            await c.execute(text("UPDATE guild_config SET entry_channel_id = '999'"))
        await c.execute(
            text("""
            INSERT INTO user_lane_mmr
                (guild_id, season_id, user_id, lane, mmr, games_played, wins, revision, updated_at)
            SELECT guild_id, season_id, '1001', 'TOP', 1732.5, 12, 8, 3, now() FROM recruitments
        """)
        )
    return path, headers
