"""Reusable fixtures and request builders; no test module imports."""

import uuid

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncEngine

from tests.support.guilds.context import GUILD_A, USER_ID

PATH = f"/api/guilds/{GUILD_A}/recruitments"


async def recruitment_fixture(owner: AsyncEngine, number: int, status: str = "OPEN") -> uuid.UUID:
    record_id = uuid.UUID(int=number)
    async with owner.begin() as connection:
        await connection.execute(
            text("""
            INSERT INTO recruitments
                (guild_id, id, season_id, display_number, title, status, revision,
                 created_by, created_at, updated_at, best_of, hard_fearless)
            SELECT :guild, :id, season.id, :number, '일반 모집', :status, 0, :user, now(),
                   '2026-09-13T01:02:03.123456Z', 3, true
            FROM seasons season WHERE season.guild_id = :guild AND season.status = 'ACTIVE'
        """),
            {
                "guild": GUILD_A,
                "id": record_id,
                "number": number,
                "status": status,
                "user": USER_ID,
            },
        )
    return record_id
