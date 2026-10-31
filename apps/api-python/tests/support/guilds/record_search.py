"""Minimal completed record scopes with ten reusable participant identities."""

import uuid
from datetime import UTC, datetime, timedelta
from typing import Any

import httpx
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection, AsyncEngine

from tests.support.guilds.context import GUILD_A, USER_ID, authenticated_setup
from tests.support.guilds.statistics import regular_parent

PATH = f"/api/guilds/{GUILD_A}/records/search"
LANES = ("TOP", "JUNGLE", "MID", "BOTTOM", "SUPPORT")
PLAYERS = [USER_ID, *[str(4000 + index) for index in range(1, 10)]]
WHEN = datetime(2026, 9, 10, 12, 0, 0, 123456, tzinfo=UTC)


async def seeded(owner: AsyncEngine, client: httpx.AsyncClient) -> dict[str, Any]:
    await authenticated_setup(owner, client)
    async with owner.begin() as c:
        season = await c.scalar(text("SELECT id FROM seasons WHERE guild_id=:g"), {"g": GUILD_A})
        for player in PLAYERS[1:]:
            await c.execute(
                text("""
                INSERT INTO users (user_id,display_name,created_at,updated_at)
                VALUES (:u,:u,now(),now())
            """),
                {"u": player},
            )
            await c.execute(
                text("""
                INSERT INTO guild_members (guild_id,user_id,display_name,status,first_seen_at)
                VALUES (:g,:u,:u,'ACTIVE',now())
            """),
                {"g": GUILD_A, "u": player},
            )
        parent = await regular_parent(c, season)
        await c.execute(
            text("""
            INSERT INTO series_participants
                (guild_id,series_id,user_id,team,lane,mmr_at_lock,mmr_snapshot_origin)
            VALUES (:g,:p,:u,'TEAM_1','TOP',1500,'CAPTURED')
        """),
            {"g": GUILD_A, "p": parent, "u": USER_ID},
        )
        regular = await game(c, season, parent, 0, regular=True, ledger=True)
        cancelled = await regular_parent(c, season, number=2, cancelled=True)
        await game(c, season, cancelled, 1, regular=True)
        ten = await auction(c, season, 10)
        twenty = await auction(c, season, 20)
        await c.execute(
            text("""
            INSERT INTO user_lane_mmr
                (guild_id,season_id,user_id,lane,mmr,games_played,wins,revision,updated_at)
            VALUES (:g,:s,:u,'TOP',1515,4,3,1,now())
        """),
            {"g": GUILD_A, "s": season, "u": USER_ID},
        )
        return {
            "season": season,
            "series": parent,
            "regular": regular,
            "ten": ten,
            "twenty": twenty,
        }


async def game(
    c: AsyncConnection,
    season: uuid.UUID,
    parent: uuid.UUID,
    offset: int,
    *,
    regular: bool = False,
    ledger: bool = False,
    swapped: bool = False,
) -> uuid.UUID:
    identifier, source = uuid.uuid7(), uuid.uuid7()
    params = {
        "g": GUILD_A,
        "s": season,
        "p": parent,
        "u": USER_ID,
        "id": identifier,
        "input": source,
        "series": parent if regular else None,
        "match": None if regular else parent,
        "preset": "SWAPPED" if swapped else "DEFAULT",
        "win": "RED" if swapped else "BLUE",
        "when": WHEN + timedelta(days=offset),
        "mmr": "APPLIED" if ledger else "LEGACY_NOT_APPLIED",
    }
    await c.execute(
        text("""
        INSERT INTO game_inputs
            (guild_id,id,series_id,auction_match_id,set_number,side_preset,winning_side,
             status,updated_by,updated_at)
        VALUES (:g,:input,:series,:match,1,:preset,:win,'SUBMITTED',:u,now())
    """),
        params,
    )
    await c.execute(
        text("""
        INSERT INTO games
            (guild_id,id,season_id,series_id,auction_match_id,source_input_id,game_number,
             side_preset,winning_side,status,mmr_status,played_at,confirmed_by,
             confirmed_by_type,confirmed_at)
        VALUES (:g,:id,:s,:series,:match,:input,1,:preset,:win,'CONFIRMED',
                :mmr,:when,:u,'USER',now())
    """),
        params,
    )
    for index, player in enumerate(PLAYERS):
        await c.execute(
            text("""
            INSERT INTO game_participants (guild_id,game_id,user_id,team,lane,champion_id)
            VALUES (:g,:game,:player,:team,:lane,:champion)
        """),
            {
                "g": GUILD_A,
                "game": identifier,
                "player": player,
                "team": "TEAM_1" if index < 5 else "TEAM_2",
                "lane": LANES[index % 5],
                "champion": "Ahri" if player == USER_ID else None,
            },
        )
    await c.execute(
        text("""
        INSERT INTO game_bans (guild_id,game_id,side,slot,champion_id)
        VALUES (:g,:id,'BLUE',1,'Jax'),(:g,:id,'RED',2,'Vi')
    """),
        params,
    )
    if ledger:
        await c.execute(
            text("""
            INSERT INTO mmr_ledger_entries
                (guild_id,id,season_id,user_id,lane,lane_revision,source_type,source_id,
                 game_id,opponent_user_id,mmr_before,mmr_after,delta,games_played_delta,
                 wins_delta,actor_type,actor_id,request_id,created_at)
            VALUES (:g,:ledger,:s,:u,'TOP',1,'GAME_RESULT',:id,:id,:opponent,
                    1500,1515,15,1,1,'USER',:u,'record-search-test',now())
        """),
            {**params, "ledger": uuid.uuid7(), "opponent": PLAYERS[5]},
        )
    return identifier


async def auction(c: AsyncConnection, season: uuid.UUID, count: int) -> dict[str, Any]:
    tournament, recruitment = uuid.uuid7(), uuid.uuid7()
    left, right = uuid.uuid7(), uuid.uuid7()
    params = {
        "g": GUILD_A,
        "s": season,
        "u": USER_ID,
        "t": tournament,
        "r": recruitment,
        "count": count,
        "display_number": count,
        "format": "TEN_PERSON" if count == 10 else "TWENTY_PERSON",
        "semifinal": None if count == 10 else 3,
        "title": f"{count}인 경매",
        "left": left,
        "right": right,
    }
    await c.execute(
        text("""
        INSERT INTO auction_recruitments
            (guild_id,id,season_id,display_number,title,format,target_count,semifinal_best_of,
             status,revision,created_by,created_at,updated_by,updated_at)
        VALUES (:g,:r,:s,:display_number,:title,:format,:count,:semifinal,
                'CONVERTED',1,:u,now(),:u,now())
    """),
        params,
    )
    await c.execute(
        text("""
        INSERT INTO auction_tournaments
            (guild_id,id,season_id,source_recruitment_id,display_number,title,format,
             semifinal_best_of,status,revision,bracket_locked_at,bracket_locked_by,
             started_at,created_by,updated_by,updated_at)
        VALUES (:g,:t,:s,:r,:display_number,:title,:format,:semifinal,
                'IN_GAME',1,now(),:u,now(),:u,:u,now())
    """),
        params,
    )
    for index, team in enumerate((left, right), 1):
        await c.execute(
            text("""
            INSERT INTO auction_teams
                (guild_id,tournament_id,id,team_index,team_name,initial_points,current_points,
                 point_revision,created_at)
            VALUES (:g,:t,:team,:index,:name,1000,700,1,now())
        """),
            {**params, "team": team, "index": index, "name": f"팀 {index}"},
        )
    if count == 20:
        await roster(c, tournament, left)
    matches, games = [], []
    for stage, offset, swapped in (
        [("FINAL", 2, False)] if count == 10 else [("SEMIFINAL_A", 1, True), ("FINAL", 3, False)]
    ):
        match = uuid.uuid7()
        await c.execute(
            text("""
            INSERT INTO auction_matches
                (guild_id,tournament_id,id,season_id,match_key,best_of,left_team_id,right_team_id,
                 status,winner_team_id,revision,lineups_locked_at,lineups_locked_by,
                 started_at,ended_at,updated_by,updated_at)
            VALUES (:g,:t,:id,:s,:stage,1,:left,:right,'COMPLETED',:left,1,
                    now(),:u,now(),now(),:u,now())
        """),
            {**params, "id": match, "stage": stage},
        )
        matches.append(match)
        games.append(await game(c, season, match, offset, swapped=swapped))
    await c.execute(
        text("""
        UPDATE auction_tournaments SET status='COMPLETED', champion_team_id=:left, ended_at=now()
        WHERE guild_id=:g AND id=:t
    """),
        params,
    )
    return {"id": tournament, "team": left, "matches": matches, "games": games}


async def roster(c: AsyncConnection, tournament: uuid.UUID, team: uuid.UUID) -> None:
    for index, player in enumerate(PLAYERS[:5]):
        action = None
        params = {
            "g": GUILD_A,
            "t": tournament,
            "team": team,
            "player": player,
            "actor": USER_ID,
            "slot": index + 1,
            "lane": LANES[index],
        }
        if index:
            action, lot = uuid.uuid7(), uuid.uuid7()
            await c.execute(
                text("""
                INSERT INTO auction_bid_candidates
                    (guild_id,tournament_id,user_id,declared_lane,mmr_at_start,snapshot_origin,
                     queue_position,state,draw_count,updated_at)
                VALUES (:g,:t,:player,:lane,1500,'CAPTURED',:slot,'ASSIGNED',1,now())
            """),
                params,
            )
            await c.execute(
                text("""
                INSERT INTO auction_lots
                    (guild_id,tournament_id,id,draw_sequence,target_user_id,status,opened_by_type,
                     opened_by_id,opened_at,closed_by_type,closed_by_id,closed_at)
                VALUES (:g,:t,:lot,:slot,:player,'ASSIGNED','USER',:actor,now(),'USER',:actor,now())
            """),
                {**params, "lot": lot},
            )
            await c.execute(
                text("""
                INSERT INTO auction_actions
                    (guild_id,tournament_id,id,sequence,kind,target_user_id,team_id,lot_id,
                     actor_type,actor_id,created_at)
                VALUES (:g,:t,:action,:slot,'ASSIGN_CURRENT',:player,:team,:lot,'USER',:actor,now())
            """),
                {**params, "action": action, "lot": lot},
            )
        await c.execute(
            text("""
            INSERT INTO auction_team_members
                (guild_id,tournament_id,team_id,id,user_id,roster_slot,acquired_via,declared_lane,
                 mmr_at_acquisition,snapshot_origin,acquisition_action_id,acquisition_action_kind,
                 joined_at)
            VALUES (:g,:t,:team,:id,:player,:slot,:via,:lane,1500,'CAPTURED',:action,:kind,now())
        """),
            {
                **params,
                "id": uuid.uuid7(),
                "via": "CAPTAIN" if index == 0 else "MANUAL",
                "action": action,
                "kind": "ASSIGN_CURRENT" if action else None,
            },
        )
