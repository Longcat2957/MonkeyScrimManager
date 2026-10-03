"""Auction cancellation transactions, matching the frozen Rust lifecycle and effect order."""

import uuid
from typing import Any

from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError
from monkeybot_api.db.queries import params
from monkeybot_api.repositories.auctions import cancellation_commands as storage
from monkeybot_api.repositories.auctions.recruitments import snapshot
from monkeybot_api.repositories.effects import audit, complete, invalidate, notification
from monkeybot_api.repositories.guild_access import require_operator
from monkeybot_api.repositories.idempotency import acquire
from monkeybot_api.repositories.members import sync_actor
from monkeybot_api.schemas.auctions.cancellation import error
from monkeybot_api.schemas.bot import Actor
from monkeybot_api.services.auctions.mmr import void_game


async def operator(c: AsyncConnection, p: dict[str, Any], role: str, actor: Actor | None) -> None:
    status = await storage.lock_member_status(c, p)
    if status != "ACTIVE":
        raise error("member_inactive" if p["bot"] else "guild_membership_required", bot=p["bot"])
    try:
        await require_operator(c, p, role, actor)
    except ApiError as exc:
        raise error(exc.code, exc.details, bot=p["bot"]) from None


async def cancel_source(
    c: AsyncConnection, p: dict[str, Any], identifier: uuid.UUID, *, converted: bool
) -> int:
    revision = await storage.cancel_source(c, {**p, "source": identifier}, converted=converted)
    if revision is None:
        raise ApiError.from_status(500)
    result = await snapshot(c, p["guild"], identifier, p["actor_id"])
    record = result["recruitment"]
    if record["status"] != "CANCELLED" or record["revision"] != revision:
        raise ApiError.from_status(500)
    if record["channelId"] is not None and record["messageId"] is not None:
        await notification(
            c,
            p,
            "AUCTION_RECRUITMENT_MESSAGE_DELETE",
            record["channelId"],
            f"auction-recruitment:{identifier}:message:delete:{revision}",
            {"schemaVersion": 2, "auctionRecruitmentId": str(identifier), "snapshot": result},
        )
    return int(revision)


async def cancel_tournament(
    c: AsyncConnection, p: dict[str, Any], identifier: uuid.UUID, expected: int | None
) -> tuple[dict[str, Any], dict[str, Any]]:
    tournament = await storage.lock_tournament(c, p["guild"], identifier)
    if tournament is None:
        raise error("auction_tournament_not_found", bot=p["bot"])
    if expected is not None and expected != tournament["revision"]:
        raise error(
            "auction_tournament_revision_conflict",
            {"expectedRevision": expected, "actualRevision": tournament["revision"]},
            bot=p["bot"],
        )
    if tournament["status"] not in {"BIDDING", "BRACKET_SETUP", "IN_GAME"}:
        raise error(
            "auction_tournament_cancel_unavailable", {"currentStatus": tournament["status"]}
        )
    source = tournament["source_recruitment_id"]
    recruitment = await storage.lock_source_recruitment(c, p["guild"], source)
    if recruitment is None:
        raise error("not_found", bot=p["bot"])
    if recruitment["status"] != "CONVERTED":
        raise error("invalid_auction_recruitment_state", {"currentStatus": recruitment["status"]})
    await storage.lock_active_matches(c, {**p, "tournament": identifier})
    before = {
        "tournamentStatus": tournament["status"],
        "tournamentRevision": tournament["revision"],
        "sourceRecruitmentId": str(source),
        "sourceRecruitmentStatus": recruitment["status"],
    }
    reversed_count = 0
    while game := await storage.lock_latest_confirmed_game(c, p["guild"], identifier):
        try:
            await void_game(c, p, game, actor_type="BOT", void_reason="TOURNAMENT_CANCELLED")
        except ApiError as exc:
            if exc.code == "auction_match_game_reversal_not_safe":
                raise error("auction_tournament_cancel_mmr_history_advanced") from None
            raise
        reversed_count += 1
    await storage.cancel_active_matches(c, {**p, "tournament": identifier})
    revision = await storage.cancel_tournament(c, {**p, "tournament": identifier})
    if revision is None:
        raise error("auction_tournament_cancel_unavailable", {"currentStatus": "CANCELLED"})
    source_revision = await cancel_source(c, p, source, converted=True)
    return before, {
        "tournamentId": str(identifier),
        "tournamentStatus": "CANCELLED",
        "tournamentRevision": revision,
        "sourceRecruitmentId": str(source),
        "sourceRecruitmentRevision": source_revision,
        "reversedGameCount": reversed_count,
    }


async def mutate(c: AsyncConnection, p: dict[str, Any], actor: Actor | None) -> dict[str, Any]:
    status = await storage.lock_guild(c, params(p["guild"]))
    if status is None:
        raise error("not_found", bot=p["bot"])
    if status != "ACTIVE":
        raise error("guild_inactive", {"currentStatus": status}, bot=p["bot"])
    role = await storage.lock_config(c, params(p["guild"]))
    if role is None:
        raise error("guild_setup_required", bot=p["bot"])
    # Browser authorization is checked before replay; Bot replay precedes actor synchronization.
    if not p["bot"]:
        await operator(c, p, role, None)
    try:
        replay = await acquire(c, p, p["bot"])
    except ApiError as exc:
        raise error(exc.code, exc.details, bot=p["bot"]) from None
    if replay is not None:
        return replay
    if actor is not None:
        await sync_actor(c, actor, p)
        await operator(c, p, role, actor)
    result: dict[str, Any]
    if p["force"]:
        recruitment = await storage.lock_recruitment(c, p["guild"], p["id"])
        if recruitment is None:
            raise error("not_found", bot=True)
        status = recruitment["status"]
        if status in {"OPEN", "CLOSED"}:
            before = await snapshot(c, p["guild"], p["id"], p["actor_id"])
            revision = await cancel_source(c, p, p["id"], converted=False)
            result = {
                "recruitmentId": str(p["id"]),
                "recruitmentStatus": "CANCELLED",
                "recruitmentRevision": revision,
                "tournamentId": None,
                "tournamentStatus": None,
                "tournamentRevision": None,
                "reversedGameCount": 0,
            }
        elif status == "CONVERTED":
            identifier = await storage.find_tournament_id(c, p)
            if identifier is None:
                raise error("auction_tournament_not_found", bot=True)
            before, cancelled = await cancel_tournament(c, p, identifier, None)
            if cancelled["sourceRecruitmentId"] != str(p["id"]):
                raise ApiError.from_status(500)
            result = {
                "recruitmentId": str(p["id"]),
                "recruitmentStatus": "CANCELLED",
                "recruitmentRevision": cancelled["sourceRecruitmentRevision"],
                **{
                    key: cancelled[key]
                    for key in (
                        "tournamentId",
                        "tournamentStatus",
                        "tournamentRevision",
                        "reversedGameCount",
                    )
                },
            }
        else:
            raise error("invalid_recruitment_state", {"currentStatus": status}, bot=True)
        await audit(
            c,
            p,
            "AUCTION_RECRUITMENT_FORCE_CANCELLED",
            "AUCTION_RECRUITMENT",
            str(p["id"]),
            before,
            {key: value for key, value in result.items() if key != "recruitmentId"}
            | {"source": "DISCORD_FORCE_CANCEL"},
        )
        source_id, source_revision = result["recruitmentId"], result["recruitmentRevision"]
    else:
        before, result = await cancel_tournament(c, p, p["id"], p["expected"])
        await audit(
            c,
            p,
            "AUCTION_TOURNAMENT_CANCELLED",
            "AUCTION_TOURNAMENT",
            str(p["id"]),
            before,
            {key: value for key, value in result.items() if key != "tournamentId"}
            | {"reason": p["reason"]},
        )
        source_id = result["sourceRecruitmentId"]
        source_revision = result["sourceRecruitmentRevision"]
    if result["tournamentId"] is not None:
        await invalidate(
            c,
            p,
            "AUCTION_TOURNAMENT",
            result["tournamentId"],
            result["tournamentRevision"],
            "AUCTION_TOURNAMENT_CANCELLED",
        )
    await invalidate(
        c, p, "AUCTION_RECRUITMENT", source_id, source_revision, "AUCTION_RECRUITMENT_CANCELLED"
    )
    await complete(c, p, result, 200)
    return result
