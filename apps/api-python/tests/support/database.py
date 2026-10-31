"""Isolated PostgreSQL URL guard shared by fixtures and grant assertions."""

import os

import pytest
from sqlalchemy.engine import make_url


def isolated_database_url(name: str) -> str:
    value = os.environ.get(name)
    if not value:
        pytest.skip("Run scripts/test-fastapi.sh for isolated PostgreSQL integration tests")
    url = make_url(value)
    # Tests delete fixture data. Never accept a development/production database here.
    if url.database != "monkeybot_fastapi_test" or url.host != "127.0.0.1":
        pytest.fail("FastAPI tests require the isolated loopback monkeybot_fastapi_test database")
    return value
