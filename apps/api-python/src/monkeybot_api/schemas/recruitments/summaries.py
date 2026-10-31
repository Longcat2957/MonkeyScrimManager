from typing import Any

from monkeybot_api.core.time import timestamp


def recruitment_summary(row: dict[str, Any]) -> dict[str, Any]:
    return {
        "id": str(row["id"]),
        "displayNumber": row["display_number"],
        "title": row["title"],
        "status": row["status"],
        "targetCount": row["target_count"],
        "bestOf": row["best_of"],
        "hardFearless": row["hard_fearless"],
        "revision": row["revision"],
        "updatedAt": timestamp(row["updated_at"]),
    }
