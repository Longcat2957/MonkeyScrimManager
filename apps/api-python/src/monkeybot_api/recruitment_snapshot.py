import uuid
from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.context import timestamp
from monkeybot_api.errors import ApiError


async def recruitment_snapshot(
    connection: AsyncConnection, guild_id: str, record_id: uuid.UUID
) -> dict[str, Any]:
    params = {"guild_id": guild_id, "id": record_id}
    row = (
        (
            await connection.execute(
                text("""
        SELECT id, display_number, title, status, target_count, best_of, hard_fearless,
               revision, channel_id, message_id, created_by, updated_at
        FROM recruitments WHERE guild_id = :guild_id AND id = :id
    """),
                params,
            )
        )
        .mappings()
        .one_or_none()
    )
    if row is None:
        raise ApiError(404, "not_found", "요청한 모집을 찾을 수 없습니다.")
    participant_rows = (
        (
            await connection.execute(
                text("""
        SELECT participant.user_id AS id, member.display_name
        FROM recruitment_participants participant
        JOIN guild_members member ON member.guild_id = participant.guild_id
          AND member.user_id = participant.user_id
        WHERE participant.guild_id = :guild_id AND participant.recruitment_id = :id
          AND participant.status = 'JOINED'
        ORDER BY participant.joined_at, participant.user_id
    """),
                params,
            )
        )
        .mappings()
        .all()
    )
    lane_rows = await connection.execute(
        text("""
        SELECT preferred.user_id, preferred.lane
        FROM recruitment_preferred_lanes preferred
        JOIN recruitment_participants participant
          ON participant.guild_id = preferred.guild_id
         AND participant.recruitment_id = preferred.recruitment_id
         AND participant.user_id = preferred.user_id AND participant.status = 'JOINED'
        WHERE preferred.guild_id = :guild_id AND preferred.recruitment_id = :id
        ORDER BY preferred.user_id, preferred.priority
    """),
        params,
    )
    lanes: dict[str, list[str]] = {}
    for user_id, lane in lane_rows:
        lanes.setdefault(user_id, []).append(lane)
    snapshot = {
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
            "channelId": row["channel_id"],
            "messageId": row["message_id"],
            "createdById": row["created_by"],
        },
        "participants": [
            {
                "id": item["id"],
                "displayName": item["display_name"],
                "preferredLanes": lanes.get(item["id"], []),
            }
            for item in participant_rows
        ],
        "links": {"entry": f"/g/{guild_id}/recruitments/{record_id}/entry"},
    }
    return snapshot
