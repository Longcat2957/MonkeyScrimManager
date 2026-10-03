"""Shared authorization and command header contract for development-only routes."""

import re
from typing import Any

from fastapi import Request

from monkeybot_api.core.config import Settings
from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.security import require_csrf


def authorize(principal: dict[str, Any], settings: Settings) -> str:
    if not settings.dummy_users_enabled:
        raise ApiError(
            403, "dummy_users_disabled", "개발용 더미 사용자 기능이 비활성화되어 있습니다."
        )
    if principal["discord_id"] not in settings.admin_ids:
        raise ApiError(403, "service_admin_required", "서비스 관리자 권한이 필요합니다.")
    return str(principal["user_id"])


def authorize_mutation(request: Request, principal: dict[str, Any], settings: Settings) -> str:
    actor = authorize(principal, settings)
    require_csrf(request, settings, principal["csrf_secret_hash"])
    return actor


def command_key(request: Request) -> str:
    values = request.headers.getlist("idempotency-key")
    key = values[0] if len(values) == 1 else ""
    if not re.fullmatch(r"[\x21-\x7e]{1,128}", key):
        raise ApiError(400, "invalid_idempotency_key", "Idempotency-Key 헤더가 필요합니다.")
    return key
