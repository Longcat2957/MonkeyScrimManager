"""Small persisted workflow graphs for pending operation reads."""

import uuid

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncEngine

from tests.support.guilds.context import GUILD_A, USER_ID

PATH = f"/api/guilds/{GUILD_A}/operations"


async def series_fixture(owner: AsyncEngine, recruitment: uuid.UUID, best_of: int = 3) -> uuid.UUID:
    series = uuid.uuid7()
    async with owner.begin() as connection:
        await connection.execute(
            text("""
            INSERT INTO series (guild_id, id, season_id, source_recruitment_id, display_number,
                                title, best_of, status, revision, started_at, updated_at)
            SELECT guild_id, :series, season_id, id, display_number, title, :best_of,
                   'IN_PROGRESS', 0, now(), updated_at FROM recruitments WHERE id = :recruitment
            """),
            {"series": series, "recruitment": recruitment, "best_of": best_of},
        )
    return series


async def game_fixture(
    owner: AsyncEngine, series: uuid.UUID, number: int, *, voided: bool = False
) -> None:
    params = {
        "series": series,
        "input": uuid.uuid7(),
        "game": uuid.uuid7(),
        "number": number,
        "voided": voided,
        "actor": USER_ID,
    }
    async with owner.begin() as connection:
        await connection.execute(
            text("""
            INSERT INTO game_inputs
                (guild_id, id, series_id, set_number, side_preset, winning_side, status, updated_at)
            SELECT guild_id, :input, id, :number, 'DEFAULT', 'BLUE',
                   CASE WHEN :voided THEN 'SUPERSEDED' ELSE 'SUBMITTED' END, now()
            FROM series WHERE id = :series
            """),
            params,
        )
        await connection.execute(
            text("""
            INSERT INTO games
                (guild_id, id, season_id, series_id, source_input_id, game_number,
                 side_preset, winning_side, status, mmr_status, played_at,
                 confirmed_by, confirmed_by_type, confirmed_at,
                 voided_by, voided_by_type, voided_at, void_reason)
            SELECT guild_id, :game, season_id, id, :input, :number, 'DEFAULT', 'BLUE',
                   CASE WHEN :voided THEN 'VOIDED' ELSE 'CONFIRMED' END, 'LEGACY_NOT_APPLIED',
                   now(), :actor, 'USER', now(), CASE WHEN :voided THEN :actor ELSE NULL END,
                   CASE WHEN :voided THEN 'USER' ELSE NULL END,
                   CASE WHEN :voided THEN now() ELSE NULL END,
                   CASE WHEN :voided THEN 'test reversal' ELSE NULL END
            FROM series WHERE id = :series
            """),
            params,
        )


async def auction_fixture(
    owner: AsyncEngine, number: int, *, twenty: bool = False, tournament_status: str | None = None
) -> tuple[uuid.UUID, uuid.UUID | None]:
    recruitment, tournament = uuid.UUID(int=number), uuid.uuid7()
    async with owner.begin() as connection:
        await connection.execute(
            text("""
            INSERT INTO auction_recruitments
                (guild_id, id, season_id, display_number, title, format, target_count,
                 semifinal_best_of, status, revision, created_by, created_at, updated_at)
            SELECT :guild, :id, id, :number, '경매 모집',
                   CASE WHEN :twenty THEN 'TWENTY_PERSON' ELSE 'TEN_PERSON' END,
                   CASE WHEN :twenty THEN 20 ELSE 10 END,
                   CASE WHEN :twenty THEN 3 ELSE NULL END,
                   'CLOSED', 0, :user, now(), '2026-09-14T01:02:03Z'
            FROM seasons WHERE guild_id = :guild AND status = 'ACTIVE'
            """),
            {
                "guild": GUILD_A,
                "id": recruitment,
                "number": number,
                "twenty": twenty,
                "user": USER_ID,
            },
        )
        await connection.execute(
            text("""
            INSERT INTO auction_recruitment_participants
                (guild_id, recruitment_id, user_id, status, snapshot_origin, joined_at)
            VALUES (:guild, :id, :user, 'JOINED', 'UNAVAILABLE', now())
            """),
            {"guild": GUILD_A, "id": recruitment, "user": USER_ID},
        )
        if tournament_status is not None:
            await connection.execute(
                text("""
                INSERT INTO auction_tournaments
                    (guild_id, id, season_id, source_recruitment_id, display_number,
                     title, format, semifinal_best_of, status, revision,
                     started_at, created_by, updated_at)
                SELECT guild_id, :tournament, season_id, id, display_number, title, format,
                       semifinal_best_of, :status, 0, now(), created_by, updated_at
                FROM auction_recruitments WHERE id = :id
                """),
                {"tournament": tournament, "id": recruitment, "status": tournament_status},
            )
    return recruitment, tournament if tournament_status is not None else None


async def match_fixture(
    owner: AsyncEngine, tournament: uuid.UUID, key: str, status: str, *, deleted: bool = False
) -> uuid.UUID:
    match = uuid.uuid7()
    async with owner.begin() as connection:
        await connection.execute(
            text("""
            INSERT INTO auction_matches
                (guild_id, tournament_id, id, season_id, match_key, best_of,
                 status, revision, updated_at, deleted_at, ended_at,
                 lineups_locked_at, lineups_locked_by)
            SELECT guild_id, id, :match, season_id, :key, 3, :status, 0, now(),
                   CASE WHEN :deleted THEN now() ELSE NULL END,
                   CASE WHEN :status = 'CANCELLED' THEN now() ELSE NULL END,
                   CASE WHEN :status IN ('READY', 'IN_PROGRESS') THEN now() ELSE NULL END,
                   CASE WHEN :status IN ('READY', 'IN_PROGRESS') THEN :actor ELSE NULL END
            FROM auction_tournaments WHERE id = :tournament
            """),
            {
                "match": match,
                "key": key,
                "status": status,
                "deleted": deleted,
                "tournament": tournament,
                "actor": USER_ID,
            },
        )
    return match
