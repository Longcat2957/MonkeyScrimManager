#!/usr/bin/env python3
"""Record/check the Rust router and SQLx migration baseline; never connects to a database."""

import argparse
import hashlib
import json
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "apps/api-python/src/monkeybot_api/sqlx-manifest.json"
INVENTORY = ROOT / "docs/migration/fastapi-baseline.json"
ROUTERS = {
    "apps/api/src/http/mod.rs": ("api", "always"),
    "apps/api/src/http/notification_routes.rs": ("api", "always"),
    "apps/api/src/http/service_admin_reset_routes.rs": (
        "api",
        "registered; handler restricts environment/service admin",
    ),
    "apps/api/src/http/dummy_user_routes.rs": ("api", "MONKEYBOT_DUMMY_USERS_ENABLED"),
    "apps/api/src/http/dummy_recruitment_routes.rs": (
        "api",
        "MONKEYBOT_DUMMY_USERS_ENABLED",
    ),
    "apps/riot-api/src/http.rs": ("riot-api", "always"),
}


def router_operations(
    path: Path, service: str, condition: str
) -> list[dict[str, object]]:
    source = path.read_text().split("#[cfg(test)]", 1)[0]
    operations = []
    for match in re.finditer(r'\.route\(\s*"([^"\n]+)"\s*,', source):
        start, depth, end = match.end(), 1, match.end()
        while depth:
            if end >= len(source):
                raise ValueError(f"unbalanced route: {path}:{match.start()}")
            depth += (source[end] == "(") - (source[end] == ")")
            end += 1
        handler = source[start : end - 1]
        methods = re.findall(
            r"\b(get|post|put|patch|delete|head|options)\(\s*([\w:]+)", handler
        )
        if not methods:
            raise ValueError(f"unrecognized route method: {path}:{match.start()}")
        for method, function in methods:
            operations.append(
                {
                    "service": service,
                    "method": method.upper(),
                    "path": match[1],
                    "handler": function,
                    "source": str(path.relative_to(ROOT)),
                    "line": source.count("\n", 0, match.start()) + 1,
                    "condition": condition,
                    "implicitHead": method == "get",
                    "transport": "websocket-upgrade" if match[1] == "/ws" else "http",
                }
            )
    if source.count(".route(") != len(list(re.finditer(r'\.route\(\s*"', source))):
        raise ValueError(
            f"non-literal route requires explicit inventory support: {path}"
        )
    return operations


def artifacts() -> dict[Path, dict[str, object]]:
    migrations = [
        {
            "version": int(path.name.split("_", 1)[0]),
            "file": path.name,
            "sha384": hashlib.sha384(path.read_bytes()).hexdigest(),
        }
        for path in sorted((ROOT / "apps/api/migrations").glob("*.sql"))
    ]
    operations = [
        operation
        for path, (service, condition) in ROUTERS.items()
        for operation in router_operations(ROOT / path, service, condition)
    ]
    identities = {(op["service"], op["method"], op["path"]) for op in operations}
    if len(identities) != len(operations):
        raise ValueError("duplicate operation identity")
    files = sorted(
        [
            *ROOT.glob("apps/api/src/**/*.rs"),
            *ROOT.glob("apps/riot-api/src/**/*.rs"),
            ROOT / "openapi/monkeybot.yaml",
        ]
    )
    return {
        MANIFEST: {"formatVersion": 1, "algorithm": "sha384", "migrations": migrations},
        INVENTORY: {
            "formatVersion": 1,
            "baseCommit": subprocess.check_output(
                ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
            ).strip(),
            "basis": "working tree; hashes include uncommitted source changes",
            "operationCount": len(operations),
            "notes": [
                "GET implicitly supports HEAD; "
                "HEAD is not counted as a separate explicit operation.",
                "Fallback 404/405 and flags must also be covered by contract tests.",
                "Router extraction is static; consumer/auth/DTO/test mapping remains P0 work.",
            ],
            "sourceSha256": {
                str(path.relative_to(ROOT)): hashlib.sha256(
                    path.read_bytes()
                ).hexdigest()
                for path in files
            },
            "operations": operations,
        },
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--write", action="store_true", help="explicitly accept current source baseline"
    )
    args = parser.parse_args()
    changed = []
    for path, value in artifacts().items():
        if args.write:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")
        elif not path.exists():
            changed.append(str(path.relative_to(ROOT)))
        else:
            existing = json.loads(path.read_text())
            # The reference commit is historical metadata, not a requirement that HEAD never moves.
            if path == INVENTORY:
                value["baseCommit"] = existing["baseCommit"]
            if existing != value:
                changed.append(str(path.relative_to(ROOT)))
    if changed:
        raise SystemExit("Baseline drift; review before --write: " + ", ".join(changed))
    print(
        "FastAPI source baseline written."
        if args.write
        else "FastAPI source baseline matches."
    )


if __name__ == "__main__":
    main()
