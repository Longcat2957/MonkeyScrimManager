from typing import Any

from sqlalchemy.ext.asyncio import AsyncConnection

import monkeybot_api.repositories.series.creation as repository
from monkeybot_api.core.errors import ApiError
from monkeybot_api.repositories.effects import audit, invalidate, message_update, notification
from monkeybot_api.repositories.recruitments.snapshots import recruitment_snapshot
from monkeybot_api.schemas.users import LANES


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
    draft = await repository.lock_entry_revision(c, p)
    if draft is None:
        raise ApiError.from_status(500)
    joined = await repository.joined_count(c, p)
    assignments = await repository.lock_assignments(c, p)
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
        await repository.ensure_lane_mmr(c, v)
    for assignment in ordered:
        v = {**values, "user": assignment["participant_id"], "lane": assignment["lane"]}
        mmr[assignment["participant_id"]] = await repository.lock_lane_mmr(c, v)
    number = await repository.next_series_number(c, p)
    if number is None:
        raise ApiError.from_status(500)
    title = f"일반 내전 #{number}"
    values.update(number=number, title=title)
    await repository.insert_series(c, values)
    for a in assignments:
        await repository.insert_participant(
            c,
            {
                **values,
                "user": a["participant_id"],
                "team": a["team"],
                "lane": a["lane"],
                "mmr": mmr[a["participant_id"]],
            },
        )
    await repository.mark_converted(c, p)
    snapshot = await recruitment_snapshot(c, p["guild"], p["id"])
    await message_update(c, p, snapshot)
    identifier = str(p["series_id"])
    href = f"/g/{p['guild']}/series/{identifier}/pick-ban"
    if p["entry_channel"] is not None:
        names = {
            user: name
            for user, name in (
                await repository.member_names(
                    c, {**p, "users": [a["participant_id"] for a in assignments]}
                )
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
