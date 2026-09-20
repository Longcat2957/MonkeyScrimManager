from typing import Any, Literal

from pydantic import BaseModel, ConfigDict, field_validator
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.errors import ApiError
from monkeybot_api.profile import LANES
from monkeybot_api.recruitment_members import user_id


class MoveParticipant(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid")
    type: Literal["MOVE_PARTICIPANT"]
    participantId: str
    slot: str | None = None

    @field_validator("participantId")
    @classmethod
    def validate_id(cls, value: str) -> str:
        return user_id(value)


async def apply_move(connection: AsyncConnection, params: dict[str, Any]) -> None:
    operation = params["operation"]
    values = {**params, "member": operation["participantId"]}
    joined = await connection.scalar(
        text(
            "SELECT EXISTS (SELECT 1 FROM recruitment_participants WHERE guild_id = :guild "
            "AND recruitment_id = :id AND user_id = :member AND status = 'JOINED')"
        ),
        values,
    )
    if not joined:
        raise ApiError(422, "not_participant", "현재 모집 참가자가 아닙니다.")
    previous = (
        (
            await connection.execute(
                text(
                    "SELECT team, lane FROM recruitment_entry_assignments WHERE guild_id = :guild "
                    "AND recruitment_id = :id AND user_id = :member FOR UPDATE"
                ),
                values,
            )
        )
        .mappings()
        .one_or_none()
    )
    slot = operation["slot"]
    team, lane = None, None
    if slot is not None:
        for prefix in ("TEAM_1", "TEAM_2"):
            if slot.startswith(prefix + "_") and slot[len(prefix) + 1 :] in LANES:
                team, lane = prefix, slot[len(prefix) + 1 :]
                break
        if team is None:
            raise ApiError(422, "invalid_entry_slot", "엔트리 배치 위치가 올바르지 않습니다.")
    values.update(team=team, lane=lane)
    occupant = None
    if slot is not None:
        occupant = await connection.scalar(
            text(
                "SELECT user_id FROM recruitment_entry_assignments WHERE guild_id = :guild "
                "AND recruitment_id = :id AND team = :team AND lane = :lane FOR UPDATE"
            ),
            values,
        )
    delete = text(
        "DELETE FROM recruitment_entry_assignments WHERE guild_id = :guild "
        "AND recruitment_id = :id AND user_id = :member"
    )
    await connection.execute(delete, values)
    if slot is None:
        return
    if occupant is not None and occupant != values["member"]:
        if previous is None:
            await connection.execute(delete, {**values, "member": occupant})
        else:
            await connection.execute(
                text(
                    "UPDATE recruitment_entry_assignments SET team = :team, lane = :lane "
                    "WHERE guild_id = :guild AND recruitment_id = :id AND user_id = :member"
                ),
                {**values, "member": occupant, **previous},
            )
    await connection.execute(
        text(
            "INSERT INTO recruitment_entry_assignments "
            "(guild_id, recruitment_id, user_id, team, lane) "
            "VALUES (:guild, :id, :member, :team, :lane)"
        ),
        values,
    )
