import hashlib
import json
import re
import uuid
from datetime import UTC, datetime, timedelta
from typing import Any

from fastapi import APIRouter, Request
from pydantic import ValidationError
from sqlalchemy import text
from starlette.responses import Response

from monkeybot_api import tournament_queries as sql
from monkeybot_api.auction_routes import path
from monkeybot_api.auction_snapshot import rows, shape
from monkeybot_api.auth import require_csrf
from monkeybot_api.config import Settings
from monkeybot_api.context import timestamp
from monkeybot_api.database import Database
from monkeybot_api.errors import ApiError, success
from monkeybot_api.guild_admin import mutation
from monkeybot_api.json_body import read_json
from monkeybot_api.recruitment_create import command_json, command_key, command_principal
from monkeybot_api.session import resolve_session
from monkeybot_api.tournament_contracts import COMMAND, Create, invalid
from monkeybot_api.tournament_mutations import mutate
from monkeybot_api.tournament_snapshot import mapped, snapshot
from monkeybot_api.workflow import decode_cursor, encode_cursor, require_guild_access

BASE = "/api/guilds/{guild_id}/auction-tournaments"


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


def router(settings: Settings, database: Database) -> APIRouter:
    routes = APIRouter()

    async def write(request: Request, guild: str, identifier: str | None) -> Response:
        record = path(guild, identifier)
        creating = identifier is None
        principal = await resolve_session(request, database, settings)
        body: dict[str, Any]
        if creating:
            parsed = await read_json(request, Create)
            # Serde serializes sourceRecruitmentId before expectedRevision.
            body = {
                "sourceRecruitmentId": parsed.sourceRecruitmentId,
                "expectedRevision": parsed.expectedRevision,
            }
        else:
            raw = await command_json(request)
            try:
                parsed_command = COMMAND.validate_python(raw).model_dump()
            except ValidationError:
                raise invalid() from None
            body = {
                key: parsed_command[key]
                for key in ("kind", "expectedRevision", "memberId", "teamId", "points")
                if key in parsed_command
            }
        if body["expectedRevision"] < 0:
            raise invalid(creating)
        require_csrf(request, settings, principal["csrf_secret_hash"])
        key = command_key(request, False)
        now = datetime.now(UTC)
        p = {
            "guild": guild,
            "id": record or uuid.uuid7(),
            "actor_id": principal["user_id"],
            "method": "POST",
            "route": "/api/guilds/{guildId}/auction-tournaments"
            + ("" if creating else "/{id}/commands"),
            "principal_type": "USER",
            "principal": principal["user_id"],
            "key": key,
            "hash": hashlib.sha256(
                json.dumps([guild, body], ensure_ascii=False, separators=(",", ":")).encode()
            ).digest(),
            "now": now,
            "expires": now + timedelta(hours=24),
            "request_id": request.state.request_id,
            **{
                name: uuid.uuid7()
                for name in ("idem_id", "audit_id", "action_id", "lot_id", "member_id", "ledger_id")
            },
            **{
                name: [uuid.uuid7() for _ in range(4)]
                for name in ("team_ids", "captain_ids", "initial_ledger_ids")
            },
        }
        result = await mutation(database, lambda c: mutate(c, p, body, creating))
        response = success(request, result)
        response.status_code = 201 if creating else 200
        return response

    @routes.post(BASE)
    async def create(request: Request, guild_id: str) -> Response:
        return await write(request, guild_id, None)

    @routes.post(BASE + "/{tournament_id}/commands")
    async def commands(request: Request, guild_id: str, tournament_id: str) -> Response:
        return await write(request, guild_id, tournament_id)

    @routes.api_route(BASE + "/{tournament_id}", methods=["GET", "HEAD"])
    async def detail(request: Request, guild_id: str, tournament_id: str) -> Response:
        identifier = path(guild_id, tournament_id)
        assert identifier is not None
        principal = await resolve_session(request, database, settings)
        async with database.connection(transaction=True) as c:
            await c.execute(text("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY"))
            await require_guild_access(c, principal["user_id"], guild_id)
            try:
                result = await snapshot(c, guild_id, identifier, principal["user_id"])
            except ApiError as exc:
                if exc.status == 404:
                    raise ApiError(404, "not_found", exc.message) from None
                raise
        return success(request, result)

    @routes.api_route(BASE, methods=["GET", "HEAD"])
    async def listing(request: Request, guild_id: str) -> Response:
        path(guild_id)
        pairs = list(request.query_params.multi_items())
        q = dict(pairs)
        if (
            len(q) != len(pairs)
            or set(q) - {"state", "seasonId", "cursor", "limit"}
            or q.get("state") not in {"active", "completed"}
        ):
            raise ApiError.from_status(400)
        if "limit" in q and (not re.fullmatch(r"\+?[0-9]+", q["limit"]) or int(q["limit"]) > 65535):
            raise ApiError.from_status(400)
        bot, principal = await command_principal(request, settings, database)
        limit = int(q.get("limit", "50"))
        if not 1 <= limit <= 100:
            raise ApiError(400, "invalid_limit", "limit은 1 이상 100 이하이어야 합니다.")
        cursor_time, cursor_id = decode_cursor(q["cursor"]) if "cursor" in q else (None, None)
        try:
            season = uuid.UUID(q["seasonId"]) if "seasonId" in q else None
        except ValueError:
            raise ApiError(400, "invalid_season_id", "seasonId 형식이 올바르지 않습니다.") from None
        viewer = None if bot else principal["user_id"] if principal else None
        async with database.connection() as c:
            if viewer is not None:
                await require_guild_access(c, viewer, guild_id)
            found = await rows(
                c, sql.LIST, viewer, guild_id, q["state"], season, cursor_time, cursor_id, limit + 1
            )
        page = found[:limit]
        return success(
            request,
            {
                "tournaments": [listing_row(r, guild_id) for r in page],
                "nextCursor": encode_cursor(page[-1]["updated_at"], page[-1]["id"])
                if len(found) > limit
                else None,
            },
        )

    return routes
