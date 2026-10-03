"""Keep the independent Riot router aligned with the frozen Rust route inventory."""

import json
from pathlib import Path
from typing import cast

from pydantic import SecretStr

from monkeybot_riot_api.config import Settings
from monkeybot_riot_api.http import create_app
from monkeybot_riot_api.provider import RiotProvider


def test_riot_router_matches_frozen_baseline() -> None:
    baseline_path = Path(__file__).resolve().parents[3] / "docs/migration/fastapi-baseline.json"
    baseline = json.loads(baseline_path.read_text())
    operations = [item for item in baseline["operations"] if item["service"] == "riot-api"]
    expected = {(item["method"], item["path"]) for item in operations}
    assert len(expected) == len(operations) == 5

    settings = Settings(
        api_key=SecretStr("RGAPI-test"),
        internal_token=SecretStr("0123456789abcdef0123456789abcdef"),
        season_start_unix_seconds=1735689600,
    )
    routes = create_app(settings, provider=cast(RiotProvider, object())).routes
    actual = {
        (method, route.path)
        for route in routes
        for method in (route.methods or set())
        if method != "HEAD"
    }
    assert actual == expected
    actual_head_paths = {route.path for route in routes if "HEAD" in (route.methods or set())}
    expected_head_paths = {item["path"] for item in operations if item["implicitHead"]}
    assert actual_head_paths == expected_head_paths
