import uuid
from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api import entry_queries as sql
from monkeybot_api.context import timestamp
from monkeybot_api.errors import ApiError
from monkeybot_api.profile import LANES


def compact_tier(tier: str | None, division: str | None) -> str | None:
    prefixes = {
        "IRON": "I",
        "BRONZE": "B",
        "SILVER": "S",
        "GOLD": "G",
        "PLATINUM": "P",
        "EMERALD": "E",
        "DIAMOND": "D",
        "MASTER": "M",
        "GRANDMASTER": "GM",
        "CHALLENGER": "C",
    }
    # Rust uses to_ascii_uppercase; Unicode case folding would accept other tier spellings.
    normalized = (
        (tier or "")
        .strip()
        .translate(str.maketrans("abcdefghijklmnopqrstuvwxyz", "ABCDEFGHIJKLMNOPQRSTUVWXYZ"))
    )
    prefix = prefixes.get(normalized)
    suffix = {"": "", "I": "1", "II": "2", "III": "3", "IV": "4"}.get((division or "").strip())
    return prefix + suffix if prefix is not None and suffix is not None else None


async def entry_snapshot(
    connection: AsyncConnection, guild: str, record_id: uuid.UUID, viewer: str
) -> dict[str, Any]:
    params = {"guild": guild, "id": record_id}
    row = (await connection.execute(text(sql.RECRUITMENT), params)).mappings().one_or_none()
    if row is None:
        raise ApiError(404, "not_found", "요청한 모집을 찾을 수 없습니다.")
    participants = (await connection.execute(text(sql.PARTICIPANTS), params)).mappings().all()
    lanes: dict[str, list[str]] = {}
    for user, lane in await connection.execute(text(sql.LANES), params):
        lanes.setdefault(user, []).append(lane)
    champions: dict[str, list[str]] = {}
    for user, champion in await connection.execute(text(sql.CHAMPIONS), params):
        champions.setdefault(user, []).append(champion)
    assignments = (await connection.execute(text(sql.ASSIGNMENTS), params)).mappings().all()
    history = (
        (await connection.execute(text(sql.HEAD_TO_HEAD), {**params, "season": row["season_id"]}))
        .mappings()
        .all()
    )
    operator = await connection.scalar(
        text("""
        SELECT EXISTS (
            SELECT 1 FROM guild_members member
            JOIN guild_config config ON config.guild_id = member.guild_id
            JOIN guild_member_discord_roles role ON role.guild_id = member.guild_id
              AND role.user_id = member.user_id AND role.role_id = config.operator_role_id
            WHERE member.guild_id = :guild AND member.user_id = :viewer AND member.status = 'ACTIVE'
        )
    """),
        {**params, "viewer": viewer},
    )
    full = row["target_count"] == 10 and len(participants) == 10
    complete = (
        full
        and len(assignments) == 10
        and len({item["participant_id"] for item in assignments}) == 10
        and {(item["team"], item["lane"]) for item in assignments}
        == {(team, lane) for team in ("TEAM_1", "TEAM_2") for lane in LANES}
    )
    closed_operator = row["status"] == "CLOSED" and bool(operator)
    active_series = False
    if closed_operator and (complete or full):
        active_series = bool(
            await connection.scalar(
                text("""
            SELECT EXISTS (SELECT 1 FROM series WHERE guild_id = :guild
                AND source_recruitment_id = :id AND status <> 'CANCELLED' AND deleted_at IS NULL)
        """),
                params,
            )
        )
    editor = None
    if row["draft_updated_by"] is not None and row["draft_updated_by_display_name"] is not None:
        editor = {
            "id": row["draft_updated_by"],
            "displayName": row["draft_updated_by_display_name"],
        }
    elif row["draft_updated_by"] is not None or row["draft_updated_by_display_name"] is not None:
        raise ApiError.from_status(500)
    if any(item["team"] not in {"TEAM_1", "TEAM_2"} for item in assignments):
        raise ApiError.from_status(500)
    return {
        "recruitment": {
            "id": str(row["id"]),
            "displayNumber": row["display_number"],
            "title": row["title"],
            "status": row["status"],
            "targetCount": row["target_count"],
            "bestOf": row["best_of"],
            "hardFearless": row["hard_fearless"],
            "revision": row["revision"],
            "updatedAt": timestamp(row["updated_at"]),
        },
        "draft": {
            "revision": row["draft_revision"],
            "updatedAt": timestamp(row["draft_updated_at"]),
            "updatedBy": editor,
            "participants": [
                {
                    "id": item["id"],
                    "displayName": item["display_name"],
                    "preferredLanes": lanes.get(item["id"], []),
                    "mainLane": item["main_lane"],
                    "mmr": item["mmr"],
                    "games": item["games"],
                    "wins": item["wins"],
                    "riotTier": compact_tier(item["solo_rank_tier"], item["solo_rank_division"]),
                    "profileIconId": item["profile_icon_id"],
                    "championIds": champions.get(item["id"], []),
                }
                for item in participants
            ],
            "assignments": [
                {
                    "participantId": item["participant_id"],
                    "team": item["team"],
                    "lane": item["lane"],
                }
                for item in assignments
            ],
        },
        "headToHead": [
            {
                "lane": item["lane"],
                "userAId": item["user_a_id"],
                "userBId": item["user_b_id"],
                "plays": item["plays"],
                "userAWins": item["user_a_wins"],
                "userBWins": item["user_b_wins"],
            }
            for item in history
        ],
        "capabilities": {
            "canEdit": closed_operator,
            "canReset": closed_operator,
            "canCreateSeries": closed_operator and complete,
            "canCreateAuctionRecruitment": closed_operator
            and full
            and row["converted_at"] is None
            and not active_series,
            "canSendParticipantConfirmationDms": closed_operator,
            "canReopen": closed_operator,
            "canCancel": row["status"] in {"OPEN", "CLOSED"}
            and (bool(operator) or row["created_by"] == viewer),
        },
    }
