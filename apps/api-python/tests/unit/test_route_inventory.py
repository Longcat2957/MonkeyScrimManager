"""The migration candidate keeps every frozen API operation registered."""

import json
import re
from pathlib import Path
from typing import Any

from monkeybot_api.app import create_app
from monkeybot_api.core.config import Settings

BASELINE = Path(__file__).resolve().parents[4] / "docs/migration/fastapi-baseline.json"


def routes(router: Any) -> list[Any]:
    result = []
    for route in router.routes:
        if hasattr(route, "original_router"):
            result.extend(routes(route.original_router))
        else:
            result.append(route)
    return result


def path_pattern(path: str) -> str:
    return re.sub(r"\{[^{}]+\}", "{}", path)


def test_every_frozen_api_operation_is_registered() -> None:
    operations = json.loads(BASELINE.read_text())["operations"]
    expected = {
        (operation["method"], path_pattern(operation["path"]))
        for operation in operations
        if operation["service"] == "api"
    }
    app = create_app(
        Settings(env="test", public_origin="http://localhost:5173", dummy_users_enabled=True)
    )
    actual = {
        (method, path_pattern(route.path))
        for route in routes(app.router)
        for method in (getattr(route, "methods", None) or (["GET"] if route.path == "/ws" else []))
        if method not in {"HEAD", "OPTIONS"}
    }
    assert len(expected) == 137
    assert actual == expected
