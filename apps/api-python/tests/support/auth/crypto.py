"""Reusable fixtures and request builders; no test module imports."""

import base64
import json
import os
import subprocess

import pytest

ZERO_KEY = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"


def rust_auth(payload: dict) -> dict:
    executable = os.environ.get("FASTAPI_AUTH_COMPAT_EXE")
    if not executable:
        pytest.skip("scripts/test-fastapi.sh builds the Rust auth interop oracle")
    result = subprocess.run(
        [executable],
        input=json.dumps(payload),
        text=True,
        capture_output=True,
        check=True,
        timeout=10,
    )
    return json.loads(result.stdout)


def decode(value: str) -> bytes:
    return base64.urlsafe_b64decode(value + "=" * (-len(value) % 4))
