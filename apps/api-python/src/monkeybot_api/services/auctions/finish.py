"""Finish a tournament atomically without reversing its games or MMR."""

import hashlib
import json
import uuid
from datetime import UTC, datetime, timedelta
from typing import Any

from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.text import trim
from monkeybot_api.db.database import Database
from monkeybot_api.db.queries import params
from monkeybot_api.db.transactions import mutation
from monkeybot_api.repositories.auctions import cancellation as preview_storage
from monkeybot_api.repositories.auctions import cancellation_commands as locks
from monkeybot_api.repositories.auctions import finish as storage
from monkeybot_api.repositories.auctions import game_commands
from monkeybot_api.repositories.effects import audit, complete, invalidate
from monkeybot_api.repositories.guild_access import require_operator
from monkeybot_api.repositories.idempotency import acquire
from monkeybot_api.schemas.auctions.finish import ERRORS, error
from monkeybot_api.services.auctions.notifications import result_notification


async def preview(
    database: Database, guild: str, identifier: uuid.UUID, actor: str
) -> dict[str, Any]:
    async with database.connection(transaction=True) as c:
        try:
            return await preview_storage.preview(c, guild, identifier, actor)
        except ApiError as exc:
            if exc.code in ERRORS:
                raise error(exc.code, exc.details) from None
            raise


async def finish(
    database: Database,
    guild: str,
    identifier: uuid.UUID,
    actor: str,
    expected: int,
    reason: str,
    key: str,
    request_id: str,
) -> dict[str, Any]:
    reason = trim(reason)
    # Struct field ordering and normalized reason match the Rust replay hash.
    payload = {
        "tournament_id": str(identifier),
        "command": {"expectedRevision": expected, "reason": reason},
    }
    now = datetime.now(UTC)
    p = {
        "guild": guild,
        "id": identifier,
        "tournament": identifier,
        "actor_id": actor,
        "principal_type": "USER",
        "principal": actor,
        "method": "POST",
        "route": "/api/guilds/{guildId}/auction-tournaments/{id}/finish",
        "key": key,
        "hash": hashlib.sha256(
            json.dumps([guild, payload], ensure_ascii=False, separators=(",", ":")).encode()
        ).digest(),
        "now": now,
        "expires": now + timedelta(hours=24),
        "idem_id": uuid.uuid7(),
        "audit_id": uuid.uuid7(),
        "request_id": request_id,
        "expected": expected,
        "reason": reason,
    }
    return await mutation(database, lambda c: mutate(c, p))


async def mutate(c: AsyncConnection, p: dict[str, Any]) -> dict[str, Any]:
    state = await locks.lock_guild(c, params(p["guild"]))
    if state is None:
        raise error("not_found")
    if state != "ACTIVE":
        raise error("guild_inactive", {"currentStatus": state})
    role = await locks.lock_config(c, params(p["guild"]))
    if role is None:
        raise error("guild_setup_required")
    if await locks.lock_member_status(c, p) != "ACTIVE":
        raise error("guild_membership_required")
    # A stored reply does not bypass a revoked membership/operator role.
    try:
        await require_operator(c, p, role, None)
        replay = await acquire(c, p, False)
    except ApiError as exc:
        raise error(exc.code, exc.details) from None
    if replay is not None:
        return replay
    tournament = await locks.lock_tournament(c, p["guild"], p["tournament"])
    if tournament is None:
        raise error("auction_tournament_not_found")
    if tournament["revision"] != p["expected"]:
        raise error(
            "auction_tournament_revision_conflict",
            {"expectedRevision": p["expected"], "actualRevision": tournament["revision"]},
        )
    if tournament["status"] not in {"BIDDING", "BRACKET_SETUP", "IN_GAME"}:
        raise error(
            "auction_tournament_finish_unavailable", {"currentStatus": tournament["status"]}
        )
    # Every game/lineup/bracket mutation takes the tournament lock first.
    match_ids = (await locks.lock_active_matches(c, p)).scalars().all()
    champion = None
    for match_id in match_ids:
        m = (await game_commands.lock_match(c, {**p, "id": match_id}))[0]
        left_score, right_score = await storage.match_score(c, {**p, "match": match_id})
        winner = (
            m["left_team_id"]
            if left_score > right_score
            else m["right_team_id"]
            if right_score > left_score
            else None
        )
        if m["match_key"] == "FINAL":
            champion = winner
        revision = await storage.finish_match(
            c, {**p, "match": match_id, "winner": winner}, completed=m["status"] == "COMPLETED"
        )
        await invalidate(
            c, p, "AUCTION_MATCH", str(match_id), revision, "AUCTION_TOURNAMENT_FINISHED"
        )
    revision = await storage.finish_tournament(c, {**p, "champion": champion})
    result = {
        "tournamentId": str(p["tournament"]),
        "tournamentStatus": "COMPLETED",
        "tournamentRevision": revision,
        "championTeamId": str(champion) if champion is not None else None,
    }
    await audit(
        c,
        p,
        "AUCTION_TOURNAMENT_FINISHED",
        "AUCTION_TOURNAMENT",
        str(p["tournament"]),
        {"tournamentStatus": tournament["status"], "tournamentRevision": tournament["revision"]},
        {key: value for key, value in result.items() if key != "tournamentId"}
        | {"endedEarly": True, "reason": p["reason"]},
    )
    await invalidate(
        c, p, "AUCTION_TOURNAMENT", str(p["tournament"]), revision, "AUCTION_TOURNAMENT_FINISHED"
    )
    await result_notification(c, p, revision)
    await complete(c, p, result, 200)
    return result
