import uuid
from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api import auction_queries as sql
from monkeybot_api.auction_contracts import validate_rules
from monkeybot_api.auction_errors import error
from monkeybot_api.context import timestamp
from monkeybot_api.errors import ApiError
from monkeybot_api.guild_admin import trim
from monkeybot_api.notification_delivery import one, params


def shape(format: str) -> tuple[int, int]:
    if format not in {"TEN_PERSON", "TWENTY_PERSON"}:
        raise ApiError.from_status(500)
    return (10, 2) if format == "TEN_PERSON" else (20, 4)


def rules(row: dict[str, Any]) -> dict[str, Any]:
    values = {
        "hardFearless": row["hard_fearless"],
        "semifinalBestOf": row["semifinal_best_of"],
        "finalBestOf": row["final_best_of"],
    }
    try:
        validate_rules(row["format"], values)
    except ApiError:
        raise ApiError.from_status(500) from None
    return values


async def rows(c: AsyncConnection, query: str, *values: Any) -> list[dict[str, Any]]:
    return [dict(row) for row in (await c.execute(text(query), params(*values))).mappings()]


async def participants(
    c: AsyncConnection, guild: str, identifier: uuid.UUID
) -> list[dict[str, Any]]:
    return [
        {"id": r["id"], "displayName": r["display_name"], "declaredLane": r["declared_lane"]}
        for r in await rows(c, sql.PARTICIPANTS, guild, identifier)
    ]


async def snapshot(
    c: AsyncConnection, guild: str, identifier: uuid.UUID, viewer: str
) -> dict[str, Any]:
    row = await one(c, sql.SNAPSHOT, guild, identifier)
    if row is None:
        raise error("auction_recruitment_not_found")
    count, teams = shape(row["format"])
    if row["target_count"] != count:
        raise ApiError.from_status(500)
    is_operator = await c.scalar(text(sql.OPERATOR), params(guild, viewer))
    status = row["status"]
    href = f"/g/{guild}/auction/recruitments/{identifier}"
    if status == "CONVERTED":
        tournament = await c.scalar(text(sql.LINKED_TOURNAMENT), params(guild, identifier))
        if tournament is None:
            raise ApiError.from_status(500)
        href = f"/g/{guild}/auction/tournaments/{tournament}/draft"
    manager = is_operator or viewer == row["created_by"]
    return {
        "recruitment": {
            "id": str(identifier),
            "displayNumber": row["display_number"],
            "title": row["title"],
            "format": row["format"],
            "rules": rules(row),
            "participantCount": count,
            "joinedParticipantCount": row["joined_participant_count"],
            "teamCount": teams,
            "status": status,
            "revision": row["revision"],
            "channelId": row["channel_id"],
            "messageId": row["message_id"],
            "createdById": row["created_by"],
            "updatedAt": timestamp(row["updated_at"]),
            "href": href,
        },
        "participants": await participants(c, guild, identifier),
        "links": {"draft": href + "/draft" if status == "CLOSED" else None},
        "permissions": {
            "canManageMembers": status == "OPEN" and bool(is_operator),
            "canClose": status == "OPEN" and bool(is_operator),
            "canReopen": status == "CLOSED" and bool(is_operator),
            "canRestore": status == "CANCELLED" and bool(manager),
            "canSendParticipantConfirmationDms": status == "CLOSED" and bool(is_operator),
            "canCancel": status in {"OPEN", "CLOSED"} and bool(manager),
        },
    }


def normalize_teams(
    teams: list[dict[str, Any]], count: int, joined: set[str]
) -> list[dict[str, Any]]:
    if len(teams) != count:
        raise error(
            "invalid_auction_setup_team_count",
            {"expectedTeamCount": count, "actualTeamCount": len(teams)},
            mode="setup",
        )
    captains = set()
    result = []
    for i, team in enumerate(teams, 1):
        if team["teamIndex"] != i:
            raise error(
                "invalid_auction_setup_team_index",
                {"expectedTeamIndex": i, "actualTeamIndex": team["teamIndex"]},
                mode="setup",
            )
        name = trim(team["teamName"])
        if not name or len(name.encode()) > 64:
            raise error("invalid_auction_setup_team_name", mode="setup")
        if team["initialPoints"] <= 0:
            raise error("invalid_auction_setup_initial_points", mode="setup")
        captain = team["captainUserId"]
        if captain in captains:
            raise error("duplicate_auction_setup_captain", mode="setup")
        captains.add(captain)
        if captain not in joined:
            raise error("auction_setup_captain_not_joined", mode="setup")
        result.append({**team, "teamName": name})
    return result


async def setup_snapshot(
    c: AsyncConnection, guild: str, identifier: uuid.UUID, viewer: str
) -> dict[str, Any]:
    row = await one(c, sql.DRAFT_RECRUITMENT, guild, identifier)
    if row is None or row["status"] != "CLOSED":
        raise error("auction_setup_draft_not_found", mode="setup")
    count, team_count = shape(row["format"])
    if row["target_count"] != count:
        raise ApiError.from_status(500)
    revision = await c.scalar(text(sql.DRAFT_REVISION), params(guild, identifier))
    if revision is None:
        raise ApiError.from_status(500)
    teams = [
        {
            "teamIndex": r["team_index"],
            "teamName": r["team_name"],
            "captainUserId": r["captain_user_id"],
            "initialPoints": r["initial_points"],
        }
        for r in await rows(c, sql.TEAMS, guild, identifier)
    ]
    roster = await participants(c, guild, identifier)
    operator = bool(await c.scalar(text(sql.OPERATOR), params(guild, viewer)))
    complete = len(roster) == count
    try:
        normalize_teams(teams, team_count, {r["id"] for r in roster})
    except ApiError:
        complete = False
    return {
        "draft": {
            "recruitmentId": str(identifier),
            "revision": revision,
            "teamCount": team_count,
            "teams": teams,
        },
        "participants": roster,
        "permissions": {"canEdit": operator, "canCreateTournament": operator and complete},
    }
