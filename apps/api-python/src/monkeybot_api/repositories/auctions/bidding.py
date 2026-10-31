import uuid
from datetime import datetime
from typing import Any

from sqlalchemy.ext.asyncio import AsyncConnection

import monkeybot_api.repositories.auctions.tournament_queries as sql
from monkeybot_api.db.queries import one
from monkeybot_api.repositories.auctions.state import change


async def lock_released_member(
    connection: AsyncConnection, guild: str, identifier: uuid.UUID, action_id: uuid.UUID, user: str
) -> dict[str, Any] | None:
    return await one(connection, sql.RELEASE_MEMBER_READ, guild, identifier, action_id, user)


async def release_member(
    connection: AsyncConnection,
    occurred_at: datetime,
    guild: str,
    identifier: uuid.UUID,
    member_id: uuid.UUID,
) -> None:
    await change(connection, sql.RELEASE_MEMBER, occurred_at, guild, identifier, member_id)


async def revert_lot(
    connection: AsyncConnection, guild: str, identifier: uuid.UUID, old_lot: uuid.UUID
) -> None:
    await change(connection, sql.REVERT_LOT, guild, identifier, old_lot)


async def clear_slots(
    connection: AsyncConnection,
    actor_id: str,
    occurred_at: datetime,
    guild: str,
    identifier: uuid.UUID,
) -> None:
    await change(connection, sql.CLEAR_SLOTS, actor_id, occurred_at, guild, identifier, count=None)


async def draw_candidate(
    connection: AsyncConnection, occurred_at: datetime, guild: str, identifier: uuid.UUID, user: str
) -> None:
    await change(connection, sql.DRAW, occurred_at, guild, identifier, user)


async def insert_member(
    connection: AsyncConnection,
    guild: str,
    identifier: uuid.UUID,
    team_id: uuid.UUID,
    member_id: uuid.UUID,
    user: str,
    slot: int,
    acquired_via: str,
    declared_lane: str | None,
    mmr_at_start: float | None,
    snapshot_origin: str,
    action_id: uuid.UUID,
    kind: str,
    occurred_at: datetime,
) -> None:
    await change(
        connection,
        sql.MEMBER,
        guild,
        identifier,
        team_id,
        member_id,
        user,
        slot,
        acquired_via,
        declared_lane,
        mmr_at_start,
        snapshot_origin,
        action_id,
        kind,
        occurred_at,
    )


async def assign_unsold_lot(
    connection: AsyncConnection, guild: str, identifier: uuid.UUID, lot_id: uuid.UUID
) -> None:
    await change(
        connection,
        "UPDATE auction_lots SET status='ASSIGNED' WHERE guild_id=:p1 "
        "AND tournament_id=:p2 AND id=:p3 AND status='UNSOLD'",
        guild,
        identifier,
        lot_id,
    )


async def reset_bidding(connection: AsyncConnection, guild: str, identifier: uuid.UUID) -> None:
    for query in (
        sql.RESET_REVERSALS,
        sql.RESET_DEBITS,
        sql.RESET_MEMBERS,
        sql.RESET_REVERSAL_ACTIONS,
        sql.RESET_ACTIONS,
        sql.RESET_LOTS,
        sql.RESET_POINTS,
    ):
        await change(connection, query, guild, identifier, count=None)
