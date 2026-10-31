"""Guard the HTTP boundary and request-scoped dependency isolation."""

import ast
import uuid
from contextlib import asynccontextmanager
from datetime import UTC, datetime
from pathlib import Path

import httpx
from fastapi import FastAPI, Request

from monkeybot_api.api.dependencies import get_database, get_settings
from monkeybot_api.api.routers.notifications import router
from monkeybot_api.core.config import Settings
from monkeybot_api.core.errors import ApiError, api_error_handler

SOURCE = Path(__file__).parents[2] / "src" / "monkeybot_api"


def test_lower_layers_do_not_import_http_boundary():
    violations = []
    for layer in ("core", "db", "domain", "schemas", "repositories", "services", "integrations"):
        for path in (SOURCE / layer).rglob("*.py"):
            for node in ast.walk(ast.parse(path.read_text())):
                modules = []
                if isinstance(node, ast.ImportFrom):
                    modules = [node.module or ""]
                    if node.module == "monkeybot_api":
                        modules.extend(f"monkeybot_api.{name.name}" for name in node.names)
                elif isinstance(node, ast.Import):
                    modules = [name.name for name in node.names]
                if any(
                    m == "monkeybot_api.api" or m.startswith("monkeybot_api.api.") for m in modules
                ):
                    violations.append(f"{path.relative_to(SOURCE)}:{node.lineno}")
    assert not violations, "Lower layers depend on HTTP modules: " + ", ".join(violations)


def test_router_modules_export_static_router_without_factories():
    violations = []
    for path in (SOURCE / "api" / "routers").rglob("*.py"):
        if path.name == "__init__.py":
            continue
        tree = ast.parse(path.read_text())
        has_router = any(
            isinstance(node, ast.Assign)
            and any(
                isinstance(target, ast.Name) and target.id == "router" for target in node.targets
            )
            and isinstance(node.value, ast.Call)
            and isinstance(node.value.func, ast.Name)
            and node.value.func.id == "APIRouter"
            for node in tree.body
        )
        factories = any(
            isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and node.name == "router"
            for node in tree.body
        )
        if not has_router or factories:
            violations.append(str(path.relative_to(SOURCE)))
    assert not violations, "Router modules must export APIRouter instances: " + ", ".join(
        violations
    )


async def test_shared_router_resolves_overridden_resources_per_application():
    class FakeDatabase:
        def __init__(self, second):
            self.now = datetime(2026, 1, 1, 0, 0, second, tzinfo=UTC)
            self.calls = []

        @asynccontextmanager
        async def connection(self, *, transaction):
            assert transaction is True
            yield self

        async def scalar(self, query, values):
            self.calls.append(values["id"])
            return self.now

    def app_for(settings, database):
        app = FastAPI()
        app.include_router(router)
        app.add_exception_handler(ApiError, api_error_handler)
        app.dependency_overrides[get_settings] = lambda: settings
        app.dependency_overrides[get_database] = lambda: database

        @app.middleware("http")
        async def request_id(request: Request, call_next):
            request.state.request_id = str(uuid.uuid7())
            return await call_next(request)

        return app

    first_db, second_db = FakeDatabase(1), FakeDatabase(2)
    first_token, second_token = "a" * 32, "b" * 32
    first = app_for(
        Settings(env="test", public_origin="http://first", bot_service_token=first_token), first_db
    )
    second = app_for(
        Settings(env="test", public_origin="http://second", bot_service_token=second_token),
        second_db,
    )
    async with (
        httpx.AsyncClient(transport=httpx.ASGITransport(app=first), base_url="http://first") as a,
        httpx.AsyncClient(transport=httpx.ASGITransport(app=second), base_url="http://second") as b,
    ):
        for client, token, instance, expected in (
            (a, first_token, "first", "2026-01-01T00:00:01Z"),
            (b, second_token, "second", "2026-01-01T00:00:02Z"),
            (a, first_token, "first-again", "2026-01-01T00:00:01Z"),
        ):
            response = await client.post(
                "/internal/heartbeat",
                json={"instanceId": instance, "version": "test"},
                headers={"authorization": f"Bearer {token}"},
            )
            assert response.status_code == 200, response.text
            assert response.json()["data"] == {"accepted": True, "lastHeartbeatAt": expected}
        denied = await a.post(
            "/internal/heartbeat",
            json={"instanceId": "other", "version": "test"},
            headers={"authorization": f"Bearer {second_token}"},
        )
        assert denied.status_code == 401 and denied.json()["error"]["code"] == "unauthorized"
    assert first_db.calls == ["first", "first-again"]
    assert second_db.calls == ["second"]
