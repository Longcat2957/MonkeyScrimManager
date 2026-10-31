"""Small manual-finish fixtures built from the existing auction HTTP workflow."""

import uuid

from sqlalchemy import text

from tests.support.auctions.bidding import PATH
from tests.support.auctions.cancellation import tournament
from tests.support.auctions.games import PATH as MATCHES
from tests.support.auctions.games import play, prepared
from tests.support.auctions.lineups import fill, run, seeded
from tests.support.guilds.context import USER_ID


async def scenario(owner, client, kind):
    if kind == "bidding":
        h, record = await tournament(owner, client)
        return h, record, []
    if kind == "bracket":
        h, bracket, _ = await seeded(owner, client)
        return h, bracket["tournament"], []
    h, bracket, first = await prepared(owner, client, 20 if kind.startswith("twenty") else 10)
    if kind.startswith("twenty"):
        other_id = bracket["bracket"]["matches"][1]["id"]
        other = (await client.get(f"{MATCHES}/{other_id}")).json()["data"]
        other = await fill(client, h, other)
        other = await run(client, h, other, "LOCK")
        if kind == "twenty-semifinals":
            # One semifinal completes; the other retains its partial score and no final opponent.
            async with owner.begin() as c:
                await c.execute(
                    text("UPDATE auction_matches SET best_of=3 WHERE match_key<>'FINAL'")
                )
                await c.execute(text("UPDATE auction_tournaments SET semifinal_best_of=3"))
                await c.execute(text("UPDATE auction_recruitments SET semifinal_best_of=3"))
            first = await play(client, h, first)
            first = await play(client, h, first)
            other = await play(client, h, other)
            return h, bracket["tournament"], [first, other]
        first = await play(client, h, first)
        other = await play(client, h, other)
        final_id = bracket["bracket"]["matches"][-1]["id"]
        final = (await client.get(f"{MATCHES}/{final_id}")).json()["data"]
        final = await run(client, h, final, "LOCK")
        final = await play(client, h, final, winner="RED")
        final = await play(client, h, final, winner="RED")
        return h, bracket["tournament"], [first, other, final]
    first = await play(client, h, first)
    first = await play(client, h, first, winner="RED" if kind == "tied" else "BLUE")
    return h, bracket["tournament"], [first]


async def finish(client, headers, record, *, key="finish", reason="남은 경기 진행 불가"):
    return await client.post(
        f"{PATH}/{record['id']}/finish",
        headers={**headers, "idempotency-key": key},
        json={"expectedRevision": record["revision"], "reason": reason},
    )


async def preserved(owner):
    async with owner.connect() as c:
        return {
            table: await c.scalar(
                text(f"SELECT jsonb_agg(to_jsonb(row) ORDER BY {order}) FROM {table} AS row")
            )
            for table, order in (
                ("games", "id"),
                ("game_inputs", "id"),
                ("game_participants", "game_id,user_id"),
                ("mmr_ledger_entries", "id"),
                ("user_lane_mmr", "user_id,lane"),
            )
        }


async def later_mmr_checkpoint(owner):
    async with owner.begin() as c:
        entry = uuid.uuid7()
        await c.execute(
            text("""INSERT INTO mmr_ledger_entries
            (guild_id,id,season_id,user_id,lane,lane_revision,source_type,source_id,
             mmr_before,mmr_after,delta,games_played_delta,wins_delta,actor_type,
             actor_id,request_id,created_at)
            SELECT guild_id,:entry,season_id,user_id,lane,revision+1,'DATA_IMPORT_SEED',:entry,
                   mmr,mmr,0,0,0,'USER',:actor,'later-import-checkpoint',now()
            FROM user_lane_mmr WHERE games_played>0 ORDER BY user_id,lane LIMIT 1"""),
            {"entry": entry, "actor": USER_ID},
        )
        await c.execute(
            text("""UPDATE user_lane_mmr mmr SET revision=mmr.revision+1
            FROM mmr_ledger_entries ledger WHERE ledger.id=:entry
             AND mmr.guild_id=ledger.guild_id AND mmr.season_id=ledger.season_id
             AND mmr.user_id=ledger.user_id AND mmr.lane=ledger.lane"""),
            {"entry": entry},
        )
