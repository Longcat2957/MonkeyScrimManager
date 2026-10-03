"""SQL operations for auction games workflows."""

from typing import Any

from sqlalchemy import text
from sqlalchemy.engine import CursorResult
from sqlalchemy.ext.asyncio import AsyncConnection

import monkeybot_api.repositories.auctions.recruitment_queries as auction_queries
import monkeybot_api.repositories.auctions.tournament_queries as tournament_queries
from monkeybot_api.repositories.auctions.lineup_commands import LINEUPS
from monkeybot_api.repositories.auctions.recruitments import rows as auction_rows
from monkeybot_api.repositories.series.snapshots import rows


async def lock_draft_input(c: AsyncConnection, *values: Any) -> list[dict[str, Any]]:
    return await rows(
        c,
        "SELECT * FROM game_inputs WHERE guild_id=:guild "
        "AND auction_match_id=:id AND set_number=:number AND status='DRAFT' FOR UPDATE",
        *values,
    )


async def draft_picks(c: AsyncConnection, *values: Any) -> list[dict[str, Any]]:
    return await rows(
        c,
        """SELECT pick.champion_id FROM game_input_picks pick
        JOIN game_inputs i ON i.guild_id=pick.guild_id AND i.id=pick.game_input_id
        WHERE i.guild_id=:guild AND i.auction_match_id=:id AND i.status='DRAFT'
        ORDER BY pick.champion_id,pick.user_id""",
        *values,
    )


async def draft_bans(c: AsyncConnection, *values: Any) -> list[dict[str, Any]]:
    return await rows(
        c,
        """SELECT ban.champion_id FROM game_input_bans ban
        JOIN game_inputs i ON i.guild_id=ban.guild_id AND i.id=ban.game_input_id
        WHERE i.guild_id=:guild AND i.auction_match_id=:id AND i.status='DRAFT'
        ORDER BY ban.champion_id,ban.side,ban.slot""",
        *values,
    )


async def create_confirmed_game(c: AsyncConnection, values: dict[str, Any]) -> CursorResult[Any]:
    return await c.execute(
        text("""INSERT INTO games
        (guild_id,id,season_id,auction_match_id,source_input_id,game_number,side_preset,winning_side,
         patch_version,status,mmr_status,played_at,confirmed_by,confirmed_by_type,confirmed_at)
        VALUES (:guild,:game,:season,:id,:input,:number,:sidePreset,:winner,:patchVersion,
                'CONFIRMED','APPLIED',:now,:actor_id,'USER',:now)"""),
        values,
    )


async def submit_input(c: AsyncConnection, values: dict[str, Any]) -> CursorResult[Any]:
    return await c.execute(
        text(
            "UPDATE game_inputs SET status='SUBMITTED',updated_by=:actor_id, "
            "updated_at=:now WHERE guild_id=:guild AND id=:input AND status='DRAFT'"
        ),
        values,
    )


async def update_match_result(c: AsyncConnection, values: dict[str, Any]) -> CursorResult[Any]:
    return await c.execute(
        text("""UPDATE auction_matches SET status=:status,winner_team_id=:winning,
        started_at=COALESCE(started_at,:now),ended_at=:ended,revision=revision+1,
        updated_by=:actor_id,updated_at=:now WHERE guild_id=:guild AND id=:id"""),
        values,
    )


async def create_draft_input(c: AsyncConnection, *values: Any) -> list[dict[str, Any]]:
    return await rows(
        c,
        """INSERT INTO game_inputs
            (guild_id,id,auction_match_id,set_number,side_preset,side_selected,status,updated_by,updated_at)
            VALUES (:guild,:input,:id,:number,'DEFAULT',false,'DRAFT',:actor_id,:now)
            RETURNING *""",
        *values,
    )


async def add_game_participant(c: AsyncConnection, values: dict[str, Any]) -> CursorResult[Any]:
    return await c.execute(
        text("""INSERT INTO game_participants
            (guild_id,game_id,user_id,team,lane,champion_id)
            VALUES (:guild,:game,:user_id,:team,:lane,:champion)"""),
        values,
    )


async def confirmed_champions(c: AsyncConnection, *values: Any) -> list[dict[str, Any]]:
    return await rows(
        c,
        """SELECT DISTINCT gp.champion_id
            FROM game_participants gp JOIN games g ON g.guild_id=gp.guild_id AND g.id=gp.game_id
            WHERE g.guild_id=:guild AND g.auction_match_id=:id AND g.status='CONFIRMED'
            AND gp.champion_id IS NOT NULL""",
        *values,
    )


async def add_game_ban(c: AsyncConnection, values: dict[str, Any]) -> CursorResult[Any]:
    return await c.execute(
        text("""INSERT INTO game_bans
                    (guild_id,game_id,side,slot,champion_id)
                    VALUES (:guild,:game,:side,:slot,:champion)"""),
        values,
    )


async def count_final_lineups(c: AsyncConnection, values: dict[str, Any]) -> Any:
    return await c.scalar(
        text(
            "SELECT count(*) FROM auction_match_lineups "
            "WHERE guild_id=:guild AND tournament_id=:tournament AND match_id=:final "
            "AND team_id=:winning"
        ),
        values,
    )


async def copy_winner_lineups(c: AsyncConnection, values: dict[str, Any]) -> CursorResult[Any]:
    return await c.execute(
        text("""INSERT INTO auction_match_lineups
        (guild_id,tournament_id,match_id,team_id,lane,team_member_id,user_id)
        SELECT guild_id,tournament_id,:final,team_id,lane,team_member_id,user_id
        FROM auction_match_lineups WHERE guild_id=:guild AND tournament_id=:tournament
        AND match_id=:id AND team_id=:winning"""),
        values,
    )


async def set_final_team(
    c: AsyncConnection, values: dict[str, Any], *, column: str
) -> CursorResult[Any]:
    return await c.execute(
        text(
            f"UPDATE auction_matches SET {column}=:winning,revision=revision+1, "
            "updated_by=:actor_id,updated_at=:now WHERE guild_id=:guild AND id=:final"
        ),
        values,
    )


async def remove_final_lineups(c: AsyncConnection, values: dict[str, Any]) -> CursorResult[Any]:
    return await c.execute(
        text(
            "DELETE FROM auction_match_lineups WHERE guild_id=:guild "
            "AND tournament_id=:tournament AND match_id=:final AND team_id=:winning"
        ),
        values,
    )


async def clear_final_team(
    c: AsyncConnection, values: dict[str, Any], *, column: str
) -> CursorResult[Any]:
    return await c.execute(
        text(
            f"UPDATE auction_matches SET {column}=NULL,winner_team_id=NULL, "
            "ended_at=NULL,revision=revision+1,updated_by=:actor_id,updated_at=:now "
            "WHERE guild_id=:guild AND id=:final"
        ),
        values,
    )


async def lock_guild(c: AsyncConnection, values: dict[str, Any]) -> Any:
    return await c.scalar(text(auction_queries.LOCK_GUILD), values)


async def lock_config(c: AsyncConnection, values: dict[str, Any]) -> Any:
    return await c.scalar(text(auction_queries.LOCK_CONFIG), values)


async def lock_member_status(c: AsyncConnection, values: dict[str, Any]) -> Any:
    return await c.scalar(
        text(
            "SELECT status FROM guild_members WHERE guild_id=:guild "
            "AND user_id=:actor_id FOR UPDATE"
        ),
        values,
    )


async def find_tournament_id(c: AsyncConnection, values: dict[str, Any]) -> Any:
    return await c.scalar(
        text(
            "SELECT tournament_id FROM auction_matches "
            "WHERE guild_id=:guild AND id=:id AND deleted_at IS NULL"
        ),
        values,
    )


async def lock_tournament(c: AsyncConnection, *values: Any) -> list[dict[str, Any]]:
    return await rows(
        c,
        "SELECT * FROM auction_tournaments WHERE guild_id=:guild "
        "AND id=:tournament AND deleted_at IS NULL FOR UPDATE",
        *values,
    )


async def lock_match(c: AsyncConnection, *values: Any) -> list[dict[str, Any]]:
    return await rows(
        c,
        "SELECT * FROM auction_matches WHERE guild_id=:guild "
        "AND id=:id AND deleted_at IS NULL FOR UPDATE",
        *values,
    )


async def lock_members(c: AsyncConnection, *values: Any) -> list[dict[str, Any]]:
    return await auction_rows(c, tournament_queries.LOCK_MEMBERS, *values)


async def lock_lineups(c: AsyncConnection, *values: Any) -> list[dict[str, Any]]:
    return await auction_rows(c, LINEUPS, *values)


async def lock_source_recruitment(c: AsyncConnection, *values: Any) -> list[dict[str, Any]]:
    return await rows(
        c,
        "SELECT * FROM auction_recruitments WHERE guild_id=:guild AND id=:source FOR UPDATE",
        *values,
    )


async def lock_active_matches(c: AsyncConnection, *values: Any) -> list[dict[str, Any]]:
    return await rows(
        c,
        "SELECT id FROM auction_matches WHERE guild_id=:guild "
        "AND tournament_id=:tournament AND deleted_at IS NULL "
        "AND status<>'CANCELLED' ORDER BY id FOR UPDATE",
        *values,
    )


async def lock_latest_confirmed_game(c: AsyncConnection, *values: Any) -> list[dict[str, Any]]:
    return await rows(
        c,
        "SELECT * FROM games WHERE guild_id=:guild AND auction_match_id=:id "
        "AND status='CONFIRMED' ORDER BY game_number DESC LIMIT 1 FOR UPDATE",
        *values,
    )


async def reopen_match(c: AsyncConnection, values: dict[str, Any]) -> CursorResult[Any]:
    return await c.execute(
        text("""UPDATE auction_matches SET status=:status,winner_team_id=NULL,
            ended_at=NULL,revision=revision+1,updated_by=:actor_id,updated_at=:now
            WHERE guild_id=:guild AND id=:id"""),
        values,
    )


async def set_source_final_best_of_one(c: AsyncConnection, values: dict[str, Any]) -> Any:
    return await c.scalar(
        text("""UPDATE auction_recruitments SET final_best_of=1,
            revision=revision+1,updated_by=:actor_id,updated_at=:now
            WHERE guild_id=:guild AND id=:source RETURNING revision"""),
        values,
    )


async def complete_match_as_best_of_one(
    c: AsyncConnection, values: dict[str, Any]
) -> CursorResult[Any]:
    return await c.execute(
        text("""UPDATE auction_matches SET best_of=1,status='COMPLETED',
            winner_team_id=:winning,ended_at=:now,revision=revision+1,
            updated_by=:actor_id,updated_at=:now WHERE guild_id=:guild AND id=:id"""),
        values,
    )


async def complete_tournament_as_best_of_one(c: AsyncConnection, values: dict[str, Any]) -> Any:
    return await c.scalar(
        text("""UPDATE auction_tournaments SET final_best_of=1,
            status='COMPLETED',champion_team_id=:winning,ended_at=:now,revision=revision+1,
            updated_by=:actor_id,updated_at=:now WHERE guild_id=:guild
            AND id=:tournament RETURNING revision"""),
        values,
    )


async def complete_tournament(c: AsyncConnection, values: dict[str, Any]) -> Any:
    return await c.scalar(
        text("""UPDATE auction_tournaments SET
                    status='COMPLETED',champion_team_id=:winning,ended_at=:now,revision=revision+1,
                    updated_by=:actor_id,updated_at=:now WHERE guild_id=:guild AND id=:tournament
                    AND status='IN_GAME' RETURNING revision"""),
        values,
    )


async def reopen_tournament(c: AsyncConnection, values: dict[str, Any]) -> CursorResult[Any]:
    return await c.execute(
        text("""UPDATE auction_tournaments SET status='IN_GAME',
                    champion_team_id=NULL,ended_at=NULL,revision=revision+1,
                    updated_by=:actor_id,updated_at=:now WHERE guild_id=:guild
                    AND id=:tournament AND status='COMPLETED'"""),
        values,
    )
