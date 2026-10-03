"""Reusable fixtures and request builders; no test module imports."""

import asyncio

from tests.support.auth.crypto import rust_auth
from tests.support.guilds.context import GUILD_A, USER_ID


def move(member: str = "1001", slot: str | None = "TEAM_2_TOP") -> dict:
    return {"type": "MOVE_PARTICIPANT", "participantId": member, "slot": slot}


async def mutate(client, path, headers, revision, operation=None, key=None):
    return await client.request(
        "PATCH" if operation is not None else "POST",
        path + "/entry-draft" + ("" if operation is not None else "/reset"),
        json={"expectedRevision": revision, **({"operation": operation} if operation else {})},
        headers={**headers, "idempotency-key": key or f"entry-{revision}"},
    )


async def reference(path, action, revision=0, key=None, **extra):
    return await asyncio.to_thread(
        rust_auth,
        {
            "operation": "recruitment_lifecycle",
            "guild_id": GUILD_A,
            "user_id": USER_ID,
            "recruitment_id": path.split("/")[-1],
            "action": action,
            "key": key or f"entry-{revision}",
            "expected_revision": revision,
            **extra,
        },
    )
