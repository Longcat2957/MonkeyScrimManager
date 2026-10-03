from typing import Any

from sqlalchemy.ext.asyncio import AsyncConnection

import monkeybot_api.repositories.recruitments.assignments as repository
from monkeybot_api.core.errors import ApiError
from monkeybot_api.schemas.users import LANES


async def apply_move(connection: AsyncConnection, params: dict[str, Any]) -> None:
    operation = params["operation"]
    values = {**params, "member": operation["participantId"]}
    joined = await repository.is_joined(connection, values)
    if not joined:
        raise ApiError(422, "not_participant", "현재 모집 참가자가 아닙니다.")
    previous = await repository.lock_assignment(connection, values)
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
        occupant = await repository.lock_slot_occupant(connection, values)

    await repository.remove_assignment(connection, values)
    if slot is None:
        return
    if occupant is not None and occupant != values["member"]:
        if previous is None:
            await repository.remove_assignment(connection, {**values, "member": occupant})
        else:
            await repository.move_assignment(connection, {**values, "member": occupant, **previous})
    await repository.insert_assignment(connection, values)
