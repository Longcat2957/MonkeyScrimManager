import uuid
from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

import monkeybot_api.repositories.auctions.tournament_queries as sql
from monkeybot_api.core.errors import ApiError
from monkeybot_api.db.queries import params
from monkeybot_api.domain.auctions.tournament_errors import error
from monkeybot_api.repositories.auctions.recruitments import rows


async def change(c: AsyncConnection, query: str, *values: Any, count: int | None = 1) -> None:
    result = await c.execute(text(query), params(*values))
    if count is not None and result.rowcount != count:
        raise ApiError.from_status(500)


def current(candidates: list[dict[str, Any]], lots: list[dict[str, Any]]) -> dict[str, Any] | None:
    active = [r for r in candidates if r["state"] == "ACTIVE"]
    opened = [r for r in lots if r["status"] == "ACTIVE"]
    if not active and not opened:
        return None
    if len(active) != 1 or len(opened) != 1 or active[0]["user_id"] != opened[0]["target_user_id"]:
        raise ApiError.from_status(500)
    return opened[0]


def validate(
    teams: list[dict[str, Any]],
    members: list[dict[str, Any]],
    candidates: list[dict[str, Any]],
    lots: list[dict[str, Any]],
) -> None:
    current(candidates, lots)
    users = {r["user_id"] for r in members}
    slots = {(r["team_id"], r["roster_slot"]) for r in members}
    if (
        len(users) != len(members)
        or len(slots) != len(members)
        or any(not 1 <= r["roster_slot"] <= 5 for r in members)
    ):
        raise ApiError.from_status(500)
    for team in teams:
        if (
            team["current_points"] < 0
            or team["point_revision"] < 0
            or sum(r["team_id"] == team["id"] for r in members) > 5
        ):
            raise ApiError.from_status(500)
    for candidate in candidates:
        state = candidate["state"]
        if state not in {"QUEUED", "ACTIVE", "UNSOLD", "ASSIGNED"} or (
            (state == "ASSIGNED") != (candidate["user_id"] in users)
        ):
            raise ApiError.from_status(500)


def find(rows: list[dict[str, Any]], field: str, value: Any, code: str) -> dict[str, Any]:
    for row in rows:
        if row[field] == value:
            return row
    raise error(code)


def next_sequence(rows: list[dict[str, Any]], field: str) -> int:
    value = max((r[field] for r in rows), default=0) + 1
    if value >= 2**63:
        raise ApiError.from_status(500)
    return value


async def randomize(c: AsyncConnection, source: list[str]) -> list[str]:
    if len(source) <= 1:
        return source[:]
    result = [r["user_id"] for r in await rows(c, sql.RANDOMIZE, source)]
    if sorted(result) != sorted(source):
        raise ApiError.from_status(500)
    if result == source:
        result = result[1:] + result[:1]
    return result


async def shuffle(
    c: AsyncConnection, p: dict[str, Any], candidates: list[dict[str, Any]], reset: bool = False
) -> None:
    chosen = [r for r in candidates if reset or r["state"] == "QUEUED"]
    if not chosen or (not reset and len(chosen) <= 1):
        return
    order = await randomize(c, [r["user_id"] for r in chosen])
    positions = [r["queue_position"] for r in chosen]
    offset = max(positions) + len(chosen) + 1
    if offset >= 2**63:
        raise ApiError.from_status(500)
    await change(c, sql.QUEUE_OFFSET, p["guild"], p["id"], offset, order, count=len(chosen))
    await change(
        c,
        sql.QUEUE_RESET if reset else sql.QUEUE_ASSIGN,
        p["guild"],
        p["id"],
        order,
        positions,
        p["now"],
        count=len(chosen),
    )


async def action(
    c: AsyncConnection,
    p: dict[str, Any],
    actions: list[dict[str, Any]],
    kind: str,
    user: str,
    team: uuid.UUID | None = None,
    lot: uuid.UUID | None = None,
    points: int | None = None,
    original: dict[str, Any] | None = None,
) -> None:
    await change(
        c,
        sql.ACTION,
        p["guild"],
        p["id"],
        p["action_id"],
        next_sequence(actions, "sequence"),
        kind,
        user,
        team,
        lot,
        points,
        original["id"] if original else None,
        original["kind"] if original else None,
        p["actor_id"],
        p["now"],
    )


async def lot(c: AsyncConnection, p: dict[str, Any], lots: list[dict[str, Any]], user: str) -> None:
    await change(
        c,
        sql.LOT,
        p["guild"],
        p["id"],
        p["lot_id"],
        next_sequence(lots, "draw_sequence"),
        user,
        p["actor_id"],
        p["now"],
    )


async def transition(
    c: AsyncConnection, p: dict[str, Any], user: str, before: str, after: str
) -> None:
    await change(c, sql.TRANSITION, after, p["now"], p["guild"], p["id"], user, before)


async def close_lot(
    c: AsyncConnection, p: dict[str, Any], identifier: uuid.UUID, status: str
) -> None:
    await change(c, sql.CLOSE_LOT, status, p["actor_id"], p["now"], p["guild"], p["id"], identifier)


async def points(
    c: AsyncConnection,
    p: dict[str, Any],
    team: dict[str, Any],
    after: int,
    debit: dict[str, Any] | None = None,
) -> None:
    revision = team["point_revision"] + 1
    await change(
        c,
        sql.POINTS,
        after,
        revision,
        p["guild"],
        p["id"],
        team["id"],
        team["current_points"],
        team["point_revision"],
    )
    values = [
        p["guild"],
        p["id"],
        p["ledger_id"],
        team["id"],
        revision,
        p["action_id"],
        team["current_points"],
        after,
        after - team["current_points"],
    ]
    if debit is not None:
        values.append(debit["id"])
    await change(c, sql.REVERSAL if debit else sql.DEBIT, *values, p["actor_id"], p["now"])
