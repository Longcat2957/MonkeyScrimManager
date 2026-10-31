"""Prepare auction commands and own their transaction/retry boundaries."""

import hashlib
import json
import uuid
from datetime import UTC, datetime, timedelta
from typing import Any

from monkeybot_api.core.errors import ApiError
from monkeybot_api.db.database import Database
from monkeybot_api.db.transactions import mutation
from monkeybot_api.schemas.auctions import recruitments as contracts
from monkeybot_api.schemas.bot import BOT_ID, Actor
from monkeybot_api.services.auctions import (
    bracket,
    cancellation,
    games,
    lineups,
    recruitments,
    tournaments,
)


async def bracket_command(
    database: Database,
    guild_id: str,
    identifier: uuid.UUID | None,
    actor_id: str,
    body: dict[str, Any],
    key: str,
    request_id: str,
) -> dict[str, Any]:
    now = datetime.now(UTC)
    p = {
        "guild": guild_id,
        "id": identifier,
        "actor_id": actor_id,
        "method": "POST",
        "route": "/api/guilds/{guildId}/auction-tournaments/{id}/bracket/commands",
        "principal_type": "USER",
        "principal": actor_id,
        "key": key,
        "hash": hashlib.sha256(
            json.dumps([guild_id, body], ensure_ascii=False, separators=(",", ":")).encode()
        ).digest(),
        "now": now,
        "expires": now + timedelta(hours=24),
        "request_id": request_id,
        "idem_id": uuid.uuid7(),
        "audit_id": uuid.uuid7(),
        "match_ids": [uuid.uuid7() for _ in range(3)],
    }
    result = await mutation(database, lambda c: bracket.mutate(c, p, body))
    return result


async def lineup(
    database: Database,
    guild_id: str,
    identifier: uuid.UUID | None,
    actor_id: str,
    kind: str,
    parsed: dict[str, Any],
    key: str,
    request_id: str,
) -> dict[str, Any]:
    body = {"kind": kind, **parsed}
    now = datetime.now(UTC)
    suffix = "" if kind == "ASSIGN" else "/" + kind.lower()
    p: dict[str, Any] = {
        "guild": guild_id,
        "id": identifier,
        "actor_id": actor_id,
        "method": "PATCH" if kind == "ASSIGN" else "POST",
        "route": "/api/guilds/{guildId}/auction-matches/{matchId}/lineup" + suffix,
        "principal_type": "USER",
        "principal": actor_id,
        "key": key,
        "hash": hashlib.sha256(
            json.dumps(
                [guild_id, {"match_id": str(identifier), "command": body}],
                ensure_ascii=False,
                separators=(",", ":"),
            ).encode()
        ).digest(),
        "now": now,
        "expires": now + timedelta(hours=24),
        "request_id": request_id,
        "idem_id": uuid.uuid7(),
        "audit_id": uuid.uuid7(),
    }
    try:
        result = await mutation(database, lambda c: lineups.mutate(c, p, body))
    except ApiError as exc:
        if exc.code == "auction_tournament_not_found":
            raise ApiError(404, "not_found", exc.message) from None
        raise
    return result


async def game(
    database: Database,
    guild_id: str,
    identifier: uuid.UUID | None,
    actor_id: str,
    action: str,
    body: dict[str, Any],
    key: str,
    method: str,
    request_id: str,
) -> dict[str, Any]:
    kind = {"submit": "SUBMIT", "last": "REVERT", "complete": "COMPLETE_FINAL_AS_BO1"}[action]
    command = {"kind": kind, "expectedRevision": body["expectedRevision"]}
    if action == "submit":
        command.update(
            setNumber=body["setNumber"],
            sidePreset=body["sidePreset"],
            winner=body["winner"],
            picks=dict(sorted(body["picks"].items())),
            bans={"BLUE": body["bans"]["BLUE"], "RED": body["bans"]["RED"]},
            patchVersion=body["patchVersion"],
        )
    suffix = {"submit": "/games", "last": "/games/last", "complete": "/complete"}[action]
    now = datetime.now(UTC)
    p = {
        "guild": guild_id,
        "id": identifier,
        "actor_id": actor_id,
        "principal": actor_id,
        "principal_type": "USER",
        "request_id": request_id,
        "method": method,
        "route": "/api/guilds/{guildId}/auction-matches/{matchId}" + suffix,
        "key": key,
        "hash": hashlib.sha256(
            json.dumps(
                [guild_id, {"match_id": str(identifier), "command": command}],
                ensure_ascii=False,
                separators=(",", ":"),
            ).encode()
        ).digest(),
        "now": now,
        "expires": now + timedelta(hours=24),
        "idem_id": uuid.uuid7(),
        "audit_id": uuid.uuid7(),
    }
    result = await mutation(database, lambda c: games.mutate(c, p, action, body))
    return result


async def recruitment(
    database: Database,
    *,
    guild: str,
    record: uuid.UUID | None,
    action: str,
    target: str | None,
    actor: Actor | None,
    participant: Actor | None,
    fields: dict[str, Any],
    bot: bool,
    actor_id: str,
    key: str,
    method: str,
    request_id: str,
) -> dict[str, Any]:
    if action == "create":
        contracts.validate_rules(fields["format"], fields["rules"])
        command = {
            "format": fields["format"],
            "rules": fields["rules"],
            "expectedStartAt": fields.pop("canonical"),
        }
        record = uuid.uuid7()
    elif action in {"join", "leave"}:
        if actor is not None:
            command = {"type": action.upper(), "actor": actor.model_dump()}
            if action == "join":
                assert participant is not None
                command["participant"] = participant.model_dump()
            command["recruitment_id"] = str(record)
            command["declared_lane" if action == "join" else "user_id"] = (
                fields.get("lane") if action == "join" else target
            )
        else:
            payload = {"recruitment_id": str(record), "member_id": target}
            if action == "join":
                payload["declared_lane"] = fields["lane"]
            command = {"AddMember" if action == "join" else "RemoveMember": payload}
    elif action == "setup":
        command = {
            "recruitmentId": str(record),
            "expectedRevision": fields["expected"],
            "teams": fields["teams"],
        }
    elif action == "dm":
        command = {"recruitment_id": str(record)}
    else:
        payload = {"recruitment_id": str(record)}
        if "expected" in fields:
            payload["expected_revision"] = fields["expected"]
        command = {action.capitalize(): payload}
    suffix = {
        "create": "",
        "join": "/members",
        "leave": "/members/{userId}" if bot else "/members/{memberId}",
        "cancel": "",
        "setup": "/setup-draft",
        "dm": "/participant-confirmation-dms",
    }.get(action, "/" + action)
    route = (
        "/api/guilds/{guildId}/auction-recruitments"
        + ("" if action == "create" else "/{id}")
        + suffix
    )
    now = datetime.now(UTC)
    p = {
        **fields,
        "guild": guild,
        "id": record,
        "action": action,
        "mode": "bot"
        if bot
        else "setup"
        if action == "setup"
        else "dm"
        if action == "dm"
        else "web",
        "target": target,
        "actor_id": actor_id,
        "method": method,
        "route": route,
        "principal_type": "BOT" if bot else "USER",
        "principal": BOT_ID if bot else actor_id,
        "key": key,
        "hash": hashlib.sha256(
            json.dumps([guild, command], ensure_ascii=False, separators=(",", ":")).encode()
        ).digest(),
        "now": now,
        "expires": now + timedelta(hours=24),
        "idem_id": uuid.uuid7(),
        "audit_id": uuid.uuid7(),
        "event_id": uuid.uuid7(),
        "request_id": request_id,
    }
    result = await mutation(database, lambda c: recruitments.mutate(c, p, actor, participant))
    return result


async def tournament(
    database: Database,
    guild: str,
    record: uuid.UUID | None,
    actor_id: str,
    body: dict[str, Any],
    creating: bool,
    key: str,
    request_id: str,
) -> dict[str, Any]:
    now = datetime.now(UTC)
    p = {
        "guild": guild,
        "id": record or uuid.uuid7(),
        "actor_id": actor_id,
        "method": "POST",
        "route": "/api/guilds/{guildId}/auction-tournaments"
        + ("" if creating else "/{id}/commands"),
        "principal_type": "USER",
        "principal": actor_id,
        "key": key,
        "hash": hashlib.sha256(
            json.dumps([guild, body], ensure_ascii=False, separators=(",", ":")).encode()
        ).digest(),
        "now": now,
        "expires": now + timedelta(hours=24),
        "request_id": request_id,
        **{
            name: uuid.uuid7()
            for name in ("idem_id", "audit_id", "action_id", "lot_id", "member_id", "ledger_id")
        },
        **{
            name: [uuid.uuid7() for _ in range(4)]
            for name in ("team_ids", "captain_ids", "initial_ledger_ids")
        },
    }
    result = await mutation(database, lambda c: tournaments.mutate(c, p, body, creating))
    return result


async def cancel(
    database: Database,
    *,
    guild: str,
    record: uuid.UUID | None,
    force: bool,
    bot: bool,
    fields: dict[str, Any],
    actor: Actor | None,
    actor_id: str,
    key: str,
    request_id: str,
) -> dict[str, Any]:
    command = {"actor": actor.model_dump()} if actor is not None else {}
    if not force:
        command.update(expectedRevision=fields["expected"], reason=fields["reason"])
    payload = {"recruitment_id" if force else "tournament_id": str(record), "command": command}
    now = datetime.now(UTC)
    p = {
        **fields,
        "guild": guild,
        "id": record,
        "force": force,
        "bot": bot,
        "actor_id": actor_id,
        "principal_type": "BOT" if bot else "USER",
        "principal": BOT_ID if bot else actor_id,
        "method": "POST",
        "route": "/api/guilds/{guildId}/auction-"
        + ("recruitments/{id}/force-cancel" if force else "tournaments/{id}/cancel"),
        "key": key,
        "hash": hashlib.sha256(
            json.dumps([guild, payload], ensure_ascii=False, separators=(",", ":")).encode()
        ).digest(),
        "now": now,
        "expires": now + timedelta(hours=24),
        "idem_id": uuid.uuid7(),
        "audit_id": uuid.uuid7(),
        "request_id": request_id,
    }
    return await mutation(database, lambda c: cancellation.mutate(c, p, actor))
