import uuid
from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.repositories.series.snapshots import rows


async def bump(
    c: AsyncConnection,
    p: dict[str, Any],
    status: str,
    winning: str | None = None,
    reason: str | None = None,
) -> int:
    return int(
        await c.scalar(
            text(
                """
        UPDATE series SET status=:status,winning_team=:winning,
        terminal_reason=:reason,ended_at=:ended,revision=revision+1,
        updated_by=:actor_id,updated_at=:now WHERE guild_id=:guild AND id=:id RETURNING
        revision
    """
            ),
            {
                **p,
                "status": status,
                "winning": winning,
                "reason": reason,
                "ended": p["now"] if status != "IN_PROGRESS" else None,
            },
        )
        or 0
    )


async def result_channel(c: AsyncConnection, p: dict[str, Any]) -> str | None:
    value: str | None = await c.scalar(
        text("SELECT result_channel_id FROM guild_config WHERE guild_id=:guild"), p
    )
    return value


async def result_roster(c: AsyncConnection, p: dict[str, Any]) -> list[dict[str, Any]]:
    return await rows(
        c,
        """
        SELECT sp.user_id AS id,sp.team,sp.lane,m.display_name FROM series_participants sp
        JOIN guild_members m ON m.guild_id=sp.guild_id AND m.user_id=sp.user_id WHERE
        sp.guild_id=:guild AND sp.series_id=:id
    """,
        p,
    )


async def game_participants(c: AsyncConnection, p: dict[str, Any]) -> list[dict[str, Any]]:
    return await rows(
        c, "SELECT * FROM game_participants WHERE guild_id=:guild AND game_id=:game", p
    )


async def game_bans(c: AsyncConnection, p: dict[str, Any]) -> list[dict[str, Any]]:
    return await rows(c, "SELECT * FROM game_bans WHERE guild_id=:guild AND game_id=:game", p)


async def series_state(c: AsyncConnection, p: dict[str, Any]) -> list[dict[str, Any]]:
    return await rows(c, "SELECT * FROM series WHERE guild_id=:guild AND id=:id", p)


async def lock_guild_status(c: AsyncConnection, p: dict[str, Any]) -> str | None:
    value: str | None = await c.scalar(
        text("""SELECT status FROM guilds WHERE guild_id=:guild FOR UPDATE"""), p
    )
    return value


async def lock_operator_role(c: AsyncConnection, p: dict[str, Any]) -> str | None:
    value: str | None = await c.scalar(
        text("""SELECT operator_role_id FROM guild_config WHERE guild_id=:guild FOR UPDATE"""), p
    )
    return value


async def lock_member_status(c: AsyncConnection, p: dict[str, Any]) -> str | None:
    value: str | None = await c.scalar(
        text(
            """SELECT status FROM guild_members
            WHERE guild_id=:guild AND user_id=:actor_id FOR UPDATE"""
        ),
        p,
    )
    return value


async def source_recruitment(c: AsyncConnection, p: dict[str, Any]) -> uuid.UUID | None:
    value: uuid.UUID | None = await c.scalar(
        text(
            """SELECT source_recruitment_id FROM series
            WHERE guild_id=:guild AND id=:id AND deleted_at IS NULL"""
        ),
        p,
    )
    return value


async def lock_recruitment(c: AsyncConnection, p: dict[str, Any]) -> list[dict[str, Any]]:
    return await rows(
        c, "SELECT * FROM recruitments WHERE guild_id=:guild AND id=:source FOR UPDATE", p
    )


async def lock_series(c: AsyncConnection, p: dict[str, Any]) -> list[dict[str, Any]]:
    return await rows(
        c,
        """
        SELECT * FROM series WHERE guild_id=:guild AND id=:id AND deleted_at IS NULL FOR
        UPDATE
    """,
        p,
    )


async def restore_recruitment(c: AsyncConnection, p: dict[str, Any]) -> None:
    await c.execute(
        text("""
        UPDATE recruitments SET status=:status,revision=revision+1,updated_by=:actor_id,
        updated_at=:now,converted_at=NULL,cancelled_at=:cancelled WHERE guild_id=:guild AND
        id=:source
    """),
        p,
    )


async def lock_roster(c: AsyncConnection, p: dict[str, Any]) -> list[dict[str, Any]]:
    return await rows(
        c,
        """
        SELECT user_id,team,lane FROM series_participants WHERE guild_id=:guild AND
        series_id=:id ORDER BY user_id,lane FOR UPDATE
    """,
        p,
    )


async def touch_input(c: AsyncConnection, p: dict[str, Any]) -> None:
    await c.execute(
        text("""
        UPDATE game_inputs SET updated_by=:actor_id,updated_at=:now WHERE guild_id=:guild
        AND id=:input
    """),
        p,
    )


async def game_input(c: AsyncConnection, p: dict[str, Any]) -> list[dict[str, Any]]:
    return await rows(c, "SELECT * FROM game_inputs WHERE guild_id=:guild AND id=:input", p)


async def insert_game(c: AsyncConnection, p: dict[str, Any]) -> None:
    await c.execute(
        text("""
        INSERT INTO games (guild_id,id,season_id,series_id,source_input_id,
        game_number,side_preset,winning_side,patch_version,status,mmr_status,played_at,confirmed_by,
        confirmed_by_type,confirmed_at) VALUES
        (:guild,:game,:season,:id,:input,:number,:sidePreset,
        :winner,:patchVersion,'CONFIRMED','APPLIED',:now,:actor_id,'USER',:now)
    """),
        p,
    )


async def insert_game_participant(c: AsyncConnection, p: dict[str, Any]) -> None:
    await c.execute(
        text("""
        INSERT INTO game_participants (guild_id,game_id,user_id,team,lane,champion_id)
        VALUES (:guild,:game,:user_id,:team,:lane,:champion)
    """),
        p,
    )


async def insert_game_ban(c: AsyncConnection, p: dict[str, Any]) -> None:
    await c.execute(
        text("""
        INSERT INTO game_bans (guild_id,game_id,side,slot,champion_id) VALUES
        (:guild,:game,:side,:slot,:champion)
    """),
        p,
    )


async def submit_input(c: AsyncConnection, p: dict[str, Any]) -> None:
    await c.execute(
        text("""
        UPDATE game_inputs SET status='SUBMITTED' WHERE guild_id=:guild AND id=:input
    """),
        p,
    )
