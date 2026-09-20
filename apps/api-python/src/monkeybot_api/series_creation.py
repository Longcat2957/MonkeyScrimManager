from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.delivery_effects import audit, invalidate, message_update, notification
from monkeybot_api.entry_queries import ASSIGNMENTS
from monkeybot_api.errors import ApiError
from monkeybot_api.profile import LANES
from monkeybot_api.recruitment_snapshot import recruitment_snapshot


async def create_series(c: AsyncConnection, p: dict[str, Any], row: Any) -> dict[str, Any]:
    if row["revision"] != p["expected"]:
        raise ApiError(
            409,
            "recruitment_revision_conflict",
            "모집 상태가 변경되었습니다. 최신 상태를 확인해 주세요.",
            {"expectedRevision": p["expected"], "actualRevision": row["revision"]},
        )
    if row["status"] != "CLOSED":
        raise ApiError(
            409,
            "series_source_not_ready",
            "모집 마감 후 엔트리 편성이 완료된 상태에서만 시리즈를 시작할 수 있습니다.",
            {"currentStatus": row["status"]},
        )
    draft = await c.scalar(
        text(
            "SELECT revision FROM recruitment_entry_drafts "
            "WHERE guild_id = :guild AND recruitment_id = :id FOR UPDATE"
        ),
        p,
    )
    if draft is None:
        raise ApiError.from_status(500)
    joined = await c.scalar(
        text(
            "SELECT count(*) FROM recruitment_participants "
            "WHERE guild_id = :guild AND recruitment_id = :id AND status = 'JOINED'"
        ),
        p,
    )
    assignments = (
        (await c.execute(text(ASSIGNMENTS + " FOR UPDATE OF assignment"), p)).mappings().all()
    )
    if (
        row["target_count"] != 10
        or joined != 10
        or len(assignments) != 10
        or len({a["participant_id"] for a in assignments}) != 10
        or {(a["team"], a["lane"]) for a in assignments}
        != {(t, lane) for t in ("TEAM_1", "TEAM_2") for lane in LANES}
    ):
        raise ApiError(
            422,
            "entry_draft_incomplete",
            "참가자 10명을 양 팀의 모든 라인에 배치한 뒤 시리즈를 시작할 수 있습니다.",
            {
                "joinedCount": joined,
                "assignedCount": len(assignments),
                "targetCount": row["target_count"],
            },
        )
    values = {
        **p,
        "season": row["season_id"],
        "best_of": row["best_of"],
        "hard_fearless": row["hard_fearless"],
    }
    mmr = {}
    ordered = sorted(assignments, key=lambda a: (a["participant_id"], a["lane"]))
    for assignment in ordered:
        v = {**values, "user": assignment["participant_id"], "lane": assignment["lane"]}
        await c.execute(
            text("""
            INSERT INTO user_lane_mmr
                (guild_id, season_id, user_id, lane, mmr, games_played, wins, revision, updated_at)
            VALUES (:guild, :season, :user, :lane, 1500, 0, 0, 0, :now)
            ON CONFLICT (guild_id, season_id, user_id, lane) DO NOTHING
        """),
            v,
        )
    for assignment in ordered:
        v = {**values, "user": assignment["participant_id"], "lane": assignment["lane"]}
        mmr[assignment["participant_id"]] = await c.scalar(
            text(
                "SELECT mmr FROM user_lane_mmr "
                "WHERE guild_id = :guild AND season_id = :season AND user_id = :user "
                "AND lane = :lane FOR UPDATE"
            ),
            v,
        )
    number = await c.scalar(
        text(
            "UPDATE guild_counters SET next_value = next_value + 1 "
            "WHERE guild_id = :guild AND counter_key = 'SERIES' RETURNING next_value - 1"
        ),
        p,
    )
    if number is None:
        raise ApiError.from_status(500)
    title = f"일반 내전 #{number}"
    values.update(number=number, title=title)
    await c.execute(
        text("""
        INSERT INTO series (guild_id, id, season_id, source_recruitment_id, display_number,
            title, best_of, hard_fearless, status, revision, started_at, created_by,
            updated_by, updated_at)
        VALUES (:guild, :series_id, :season, :id, :number, :title, :best_of, :hard_fearless,
            'IN_PROGRESS', 0, :now, :actor_id, :actor_id, :now)
    """),
        values,
    )
    for a in assignments:
        await c.execute(
            text("""
            INSERT INTO series_participants
                (guild_id, series_id, user_id, team, lane, mmr_at_lock, mmr_snapshot_origin)
            VALUES (:guild, :series_id, :user, :team, :lane, :mmr, 'CAPTURED')
        """),
            {
                **values,
                "user": a["participant_id"],
                "team": a["team"],
                "lane": a["lane"],
                "mmr": mmr[a["participant_id"]],
            },
        )
    await c.execute(
        text(
            "UPDATE recruitments SET status = 'CONVERTED', revision = revision + 1, "
            "updated_by = :actor_id, updated_at = :now, converted_at = :now "
            "WHERE guild_id = :guild AND id = :id"
        ),
        p,
    )
    snapshot = await recruitment_snapshot(c, p["guild"], p["id"])
    await message_update(c, p, snapshot)
    identifier = str(p["series_id"])
    href = f"/g/{p['guild']}/series/{identifier}/pick-ban"
    if p["entry_channel"] is not None:
        names = {
            user: name
            for user, name in (
                (
                    await c.execute(
                        text(
                            "SELECT user_id, display_name FROM guild_members "
                            "WHERE guild_id = :guild AND user_id = ANY(:users)"
                        ),
                        {**p, "users": [a["participant_id"] for a in assignments]},
                    )
                ).all()
            )
        }
        entry = {
            "series": {
                "id": identifier,
                "displayNumber": number,
                "title": title,
                "bestOf": row["best_of"],
                "hardFearless": row["hard_fearless"],
            },
            "teams": [
                {
                    "team": team,
                    "name": f"{index}팀",
                    "players": [
                        {
                            "id": a["participant_id"],
                            "displayName": names[a["participant_id"]],
                            "lane": a["lane"],
                        }
                        for a in assignments
                        if a["team"] == team
                    ],
                }
                for index, team in enumerate(("TEAM_1", "TEAM_2"), 1)
            ],
            "links": {"pickBan": href},
        }
        await notification(
            c,
            p,
            "ENTRY_CONFIRMATION_CREATE",
            p["entry_channel"],
            f"series:{identifier}:entry-confirmation",
            {"schemaVersion": 1, "seriesId": identifier, "snapshot": entry},
        )
    result = {"series": {"id": identifier, "href": href}}
    await audit(
        c,
        p,
        "SERIES_CREATED",
        "SERIES",
        identifier,
        {
            "sourceRecruitmentId": str(p["id"]),
            "sourceRecruitmentRevision": row["revision"],
            "assignmentCount": len(assignments),
        },
        result,
    )
    await invalidate(
        c, p, "RECRUITMENT", str(p["id"]), row["revision"] + 1, "RECRUITMENT_CONVERTED"
    )
    await invalidate(c, p, "SERIES", identifier, 0, "SERIES_CREATED")
    return result
