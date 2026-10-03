"""Statistics fixtures reuse real auction lifecycle builders and a minimal regular parent."""

import uuid
from typing import Any

import httpx
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection, AsyncEngine

from tests.support.auctions.bidding import PATH as TOURNAMENTS
from tests.support.auctions.bidding import prepared
from tests.support.auctions.bidding import request as draft
from tests.support.auctions.bracket import run as bracket_command
from tests.support.auctions.games import changed, play
from tests.support.auctions.lineups import PATH as MATCHES
from tests.support.auctions.lineups import fill
from tests.support.auctions.lineups import run as lineup_command
from tests.support.guilds.context import GUILD_A, USER_ID

PATH = f"/api/guilds/{GUILD_A}/statistics"


async def seeded(owner: AsyncEngine, client: httpx.AsyncClient) -> dict[str, Any]:
    headers, source = await prepared(owner, client)
    state = await draft(client, headers, None, source)
    tournament = state["tournament"]["id"]
    # Freeze the draw order so winner and tie assertions are deterministic.
    async with owner.begin() as connection:
        await connection.execute(
            text("UPDATE auction_bid_candidates SET queue_position=queue_position+100")
        )
        await connection.execute(
            text("UPDATE auction_bid_candidates SET queue_position=user_id::integer-1001")
        )
    bidder = None
    for team in state["teams"]:
        for _ in range(4):
            state = await draft(
                client,
                headers,
                tournament,
                {
                    "kind": "DRAW_NEXT",
                    "expectedRevision": state["tournament"]["revision"],
                },
            )
            member = state["draft"]["currentTarget"]["userId"]
            paid = bidder is None
            state = await draft(
                client,
                headers,
                tournament,
                {
                    "kind": "AWARD_BID" if paid else "ASSIGN_CURRENT",
                    "expectedRevision": state["tournament"]["revision"],
                    "memberId": member,
                    "teamId": team["id"],
                    **({"points": 700} if paid else {}),
                },
            )
            if paid:
                bidder = member
    bracket = (await client.get(f"{TOURNAMENTS}/{tournament}/bracket")).json()["data"]
    for slot, team in zip(("A_0", "A_1"), bracket["teams"], strict=True):
        bracket = await bracket_command(
            client, headers, bracket, "ASSIGN_TEAM", slotKey=slot, teamId=team["id"]
        )
    bracket = await bracket_command(client, headers, bracket, "LOCK_BRACKET")
    match = bracket["bracket"]["matches"][0]["id"]
    state = (await client.get(f"{MATCHES}/{match}")).json()["data"]
    state = await fill(client, headers, state)
    state = await lineup_command(client, headers, state, "LOCK")
    picks = {
        row["userId"]: ("Ahri" if row["userId"] == "1000" else None) for row in state["lineups"]
    }
    state = await play(
        client,
        headers,
        state,
        sidePreset="SWAPPED",
        winner="RED",
        picks=picks,
        bans={"BLUE": ["Jax", None, None, None, None], "RED": [None] * 5},
    )
    await changed(client, headers, state, "complete")
    async with owner.begin() as connection:
        season = await connection.scalar(
            text("SELECT id FROM seasons WHERE guild_id=:g"), {"g": GUILD_A}
        )
        game = await connection.scalar(text("SELECT id FROM games"))
        await connection.execute(text("UPDATE games SET duration_seconds=1200"))
        await connection.execute(
            text("""
            INSERT INTO game_participant_stats (guild_id,game_id,user_id,kills,deaths,assists,cs)
            VALUES (:guild,:game,'1000',4,1,9,180)
        """),
            {"guild": GUILD_A, "game": game},
        )
        parent = await regular_parent(connection, season)
        first = await regular_game(connection, season, parent, game, 1, duration=1800)
        second = await regular_game(connection, season, parent, game, 2, side="SWAPPED", win="RED")
        await regular_game(connection, season, parent, game, 3, voided=True)
        cancelled = await regular_parent(connection, season, number=2, cancelled=True)
        await regular_game(connection, season, cancelled, game, 1, duration=2400)
    return {
        "season": season,
        "tournament": tournament,
        "match": match,
        "game": game,
        "series": parent,
        "first": first,
        "second": second,
        "bidder": bidder,
    }


async def regular_parent(
    connection: AsyncConnection,
    season: uuid.UUID,
    *,
    number: int = 1,
    cancelled: bool = False,
) -> uuid.UUID:
    recruitment, parent = uuid.uuid7(), uuid.uuid7()
    values = {
        "guild": GUILD_A,
        "season": season,
        "recruitment": recruitment,
        "series": parent,
        "number": number,
        "actor": USER_ID,
        "status": "CANCELLED" if cancelled else "COMPLETED",
        "winner": None if cancelled else "TEAM_1",
        "reason": "CANCELLED" if cancelled else "SCORE_REACHED",
    }
    await connection.execute(
        text("""
        INSERT INTO recruitments (guild_id,id,season_id,display_number,title,target_count,status,
                                  revision,created_by,created_at,updated_by,updated_at)
        VALUES (:guild,:recruitment,:season,:number,'일반 내전',10,'CONVERTED',1,
                :actor,now(),:actor,now())
    """),
        values,
    )
    await connection.execute(
        text("""
        INSERT INTO series (guild_id,id,season_id,source_recruitment_id,display_number,
                            title,best_of,
                            status,winning_team,terminal_reason,revision,started_at,ended_at,
                            created_by,updated_by,updated_at)
        VALUES (:guild,:series,:season,:recruitment,:number,'완료 일반 내전',
                3,:status,:winner,:reason,
                1,now(),now(),:actor,:actor,now())
    """),
        values,
    )
    return parent


async def regular_game(
    connection: AsyncConnection,
    season: uuid.UUID,
    parent: uuid.UUID,
    template: uuid.UUID,
    number: int,
    *,
    duration: int | None = None,
    side: str = "DEFAULT",
    win: str = "BLUE",
    voided: bool = False,
) -> uuid.UUID:
    game, source = uuid.uuid7(), uuid.uuid7()
    values = {
        "guild": GUILD_A,
        "season": season,
        "series": parent,
        "template": template,
        "game": game,
        "input": source,
        "number": number,
        "duration": duration,
        "side": side,
        "win": win,
        "actor": USER_ID,
        "status": "VOIDED" if voided else "CONFIRMED",
        "mmr": "REVERSED" if voided else "APPLIED",
        "voided": voided,
        "input_status": "SUPERSEDED" if voided else "SUBMITTED",
    }
    await connection.execute(
        text("""
        INSERT INTO game_inputs (guild_id,id,series_id,set_number,side_preset,winning_side,
                                 status,updated_by,updated_at)
        VALUES (:guild,:input,:series,:number,:side,:win,:input_status,:actor,now())
    """),
        values,
    )
    await connection.execute(
        text("""
        INSERT INTO games (guild_id,id,season_id,series_id,source_input_id,game_number,side_preset,
                           winning_side,duration_seconds,status,mmr_status,played_at,confirmed_by,
                           confirmed_by_type,confirmed_at,voided_by,voided_by_type,voided_at,void_reason)
        VALUES (:guild,:game,:season,:series,:input,:number,:side,:win,:duration,:status,:mmr,now(),
                :actor,'USER',now(),CASE WHEN :voided THEN :actor END,
                CASE WHEN :voided THEN 'USER' END, CASE WHEN :voided THEN now() END,
                CASE WHEN :voided THEN 'fixture void' END)
    """),
        values,
    )
    await connection.execute(
        text("""
        INSERT INTO game_participants (guild_id,game_id,user_id,team,lane,champion_id)
        SELECT guild_id,:game,user_id,team,lane,champion_id
        FROM game_participants WHERE guild_id=:guild AND game_id=:template
    """),
        values,
    )
    await connection.execute(
        text("""
        INSERT INTO game_bans (guild_id,game_id,side,slot,champion_id)
        SELECT guild_id,:game,side,slot,champion_id FROM game_bans
        WHERE guild_id=:guild AND game_id=:template
    """),
        values,
    )
    return game
