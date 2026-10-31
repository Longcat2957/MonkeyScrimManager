"""Auction read use cases: access checks, snapshot isolation, and public projections."""

import logging
import uuid
from typing import Any

from sqlalchemy import text

from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.pagination import decode_cursor, encode_cursor
from monkeybot_api.core.serialization import mapped
from monkeybot_api.core.time import timestamp
from monkeybot_api.db.database import Database
from monkeybot_api.repositories.auctions import (
    bracket,
    cancellation,
    listings,
    matches,
    recruitments,
    results,
    tournaments,
)
from monkeybot_api.repositories.auctions.recruitments import rules, shape
from monkeybot_api.repositories.guild_access import require_guild_access

logger = logging.getLogger(__name__)


def listing_row(r: dict[str, Any], guild: str) -> dict[str, Any]:
    count, teams = shape(r["format"])
    next_match = champion = None
    match_values = [
        r[k]
        for k in (
            "next_match_id",
            "next_match_key",
            "next_match_status",
            "next_match_confirmed_game_count",
        )
    ]
    if any(v is not None for v in match_values):
        if (
            any(v is None for v in match_values)
            or r["next_match_key"] not in {"SEMIFINAL_A", "SEMIFINAL_B", "FINAL"}
            or r["next_match_status"] not in {"LINEUP_PENDING", "READY", "IN_PROGRESS"}
        ):
            raise ApiError.from_status(500)
        next_match = {
            "id": str(r["next_match_id"]),
            "matchKey": r["next_match_key"],
            "label": {"SEMIFINAL_A": "준결승 A", "SEMIFINAL_B": "준결승 B", "FINAL": "결승"}[
                r["next_match_key"]
            ],
            "status": r["next_match_status"],
            "confirmedGameCount": r["next_match_confirmed_game_count"],
        }
    if r["champion_team_id"] is not None or r["champion_team_name"] is not None:
        if r["champion_team_id"] is None or r["champion_team_name"] is None:
            raise ApiError.from_status(500)
        champion = {"id": str(r["champion_team_id"]), "name": r["champion_team_name"]}
    status = r["status"]
    if status not in {"BIDDING", "BRACKET_SETUP", "IN_GAME", "COMPLETED", "CANCELLED"}:
        raise ApiError.from_status(500)
    suffix = (
        "draft"
        if status == "BIDDING"
        else "result"
        if status in {"COMPLETED", "CANCELLED"}
        else "bracket"
    )
    href = f"/g/{guild}/auction/tournaments/{r['id']}/{suffix}"
    if status == "IN_GAME" and next_match and next_match["status"] in {"READY", "IN_PROGRESS"}:
        href = f"/g/{guild}/auction/matches/{next_match['id']}"
    return {
        **mapped(
            r,
            "id display_number title format status revision active_member_count "
            "bid_member_count incomplete_match_count",
        ),
        "participantCount": count,
        "teamCount": teams,
        "nextMatch": next_match,
        "championTeam": champion,
        "updatedAt": timestamp(r["updated_at"]),
        "endedAt": timestamp(r["ended_at"]) if r["ended_at"] else None,
        "href": href,
    }


async def recruitment_detail(
    database: Database, guild: str, identifier: uuid.UUID, viewer: str, *, draft: bool
) -> dict[str, Any]:
    async with database.connection(transaction=True) as c:
        await c.execute(text("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY"))
        await require_guild_access(c, viewer, guild)
        try:
            return await (recruitments.setup_snapshot if draft else recruitments.snapshot)(
                c, guild, identifier, viewer
            )
        except ApiError as exc:
            if exc.status == 404:
                raise ApiError(404, "not_found", exc.message) from None
            raise


async def tournament_detail(
    database: Database, guild: str, identifier: uuid.UUID, viewer: str
) -> dict[str, Any]:
    async with database.connection(transaction=True) as c:
        await c.execute(text("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY"))
        await require_guild_access(c, viewer, guild)
        try:
            return await tournaments.snapshot(c, guild, identifier, viewer)
        except ApiError as exc:
            if exc.status == 404:
                raise ApiError(404, "not_found", exc.message) from None
            raise


async def bracket_detail(
    database: Database, guild: str, identifier: uuid.UUID, viewer: str
) -> dict[str, Any]:
    async with database.connection(transaction=True) as c:
        await c.execute(text("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY"))
        try:
            await require_guild_access(c, viewer, guild)
            return await bracket.snapshot(c, guild, identifier, viewer)
        except ApiError as exc:
            if exc.status == 404:
                raise ApiError(404, "not_found", "요청한 경매 내전을 찾을 수 없습니다.") from None
            raise


async def match_detail(
    database: Database, guild: str, identifier: uuid.UUID, viewer: str, request_id: str
) -> dict[str, Any]:
    async with database.connection(transaction=True) as c:
        # Preserve the original access gate before repository-error remapping.
        await require_guild_access(c, viewer, guild)
    async with database.connection(transaction=True) as c:
        await c.execute(text("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY"))
        try:
            return await matches.snapshot(c, guild, identifier, viewer)
        except ApiError as exc:
            if exc.code in {"auction_match_not_found", "not_found"}:
                raise ApiError(404, "not_found", "요청한 경매 경기를 찾을 수 없습니다.") from None
            if exc.code == "auction_match_teams_pending":
                raise ApiError(
                    409, exc.code, "이 경기는 아직 양 팀이 확정되지 않았습니다."
                ) from None
            logger.error(
                "auction_match_snapshot_failed",
                extra={"request_id": request_id, "reason": exc.code},
            )
            raise ApiError(
                500, "internal_error", "경매 경기 정보를 불러오는 중 내부 오류가 발생했습니다."
            ) from None


async def tournament_result(
    database: Database, guild: str, identifier: uuid.UUID, viewer: str, request_id: str
) -> dict[str, Any]:
    async with database.connection(transaction=True) as c:
        await c.execute(text("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY"))
        await require_guild_access(c, viewer, guild)
        try:
            return await results.snapshot(c, guild, identifier, viewer)
        except ApiError as exc:
            if exc.status == 500:
                logger.exception(
                    "Auction result snapshot invariant failed request_id=%s tournament_id=%s",
                    request_id,
                    identifier,
                )
                raise results.invariant() from exc
            raise


async def cancel_preview(
    database: Database, guild: str, identifier: uuid.UUID, viewer: str | None
) -> dict[str, Any]:
    async with database.connection(transaction=True) as c:
        await c.execute(text("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY"))
        return await cancellation.preview(c, guild, identifier, viewer)


async def recruitment_list(
    database: Database, guild_id: str, viewer: str, query: dict[str, str]
) -> dict[str, Any]:
    limit = int(query.get("limit", "50"))
    if not 1 <= limit <= 100:
        raise ApiError(400, "invalid_limit", "limit은 1 이상 100 이하이어야 합니다.")
    cursor_time, cursor_id = decode_cursor(query["cursor"]) if "cursor" in query else (None, None)
    async with database.connection() as c:
        await require_guild_access(c, viewer, guild_id)
        page = await listings.recruitments(c, viewer, guild_id, cursor_time, cursor_id, limit + 1)
    result = []
    for row in page[:limit]:
        if row["status"] not in {"OPEN", "CLOSED"}:
            raise ApiError(
                500,
                "invalid_auction_recruitment_projection",
                "경매 모집 상태를 처리하지 못했습니다.",
            )
        _, teams = shape(row["format"])
        result.append(
            {
                "id": str(row["id"]),
                "displayNumber": row["display_number"],
                "title": row["title"],
                "format": row["format"],
                "rules": rules(row),
                "participantCount": row["target_count"],
                "joinedParticipantCount": row["participant_count"],
                "teamCount": teams,
                "status": row["status"],
                "revision": row["revision"],
                "updatedAt": timestamp(row["updated_at"]),
                "href": f"/g/{guild_id}/auction/recruitments/{row['id']}"
                + ("/draft" if row["status"] == "CLOSED" else ""),
            }
        )
    cursor = (
        encode_cursor(page[limit - 1]["updated_at"], page[limit - 1]["id"])
        if len(page) > limit
        else None
    )
    return {"recruitments": result, "nextCursor": cursor}


async def tournament_list(
    database: Database, guild_id: str, viewer: str | None, q: dict[str, str]
) -> dict[str, Any]:
    limit = int(q.get("limit", "50"))
    if not 1 <= limit <= 100:
        raise ApiError(400, "invalid_limit", "limit은 1 이상 100 이하이어야 합니다.")
    cursor_time, cursor_id = decode_cursor(q["cursor"]) if "cursor" in q else (None, None)
    try:
        season = uuid.UUID(q["seasonId"]) if "seasonId" in q else None
    except ValueError:
        raise ApiError(400, "invalid_season_id", "seasonId 형식이 올바르지 않습니다.") from None
    async with database.connection() as c:
        if viewer is not None:
            await require_guild_access(c, viewer, guild_id)
        found = await listings.tournaments(
            c, viewer, guild_id, q["state"], season, cursor_time, cursor_id, limit + 1
        )
    page = found[:limit]
    return {
        "tournaments": [listing_row(r, guild_id) for r in page],
        "nextCursor": encode_cursor(page[-1]["updated_at"], page[-1]["id"])
        if len(found) > limit
        else None,
    }
