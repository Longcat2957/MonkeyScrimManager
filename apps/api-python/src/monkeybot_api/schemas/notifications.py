"""Notification transport models and request validation."""

import logging
import re
import uuid
from typing import Annotated, Literal

from pydantic import AfterValidator, Field

from monkeybot_api.core.config import is_snowflake
from monkeybot_api.core.errors import ApiError
from monkeybot_api.schemas.series import Strict

logger = logging.getLogger(__name__)


def valid_uuid(value: str) -> str:
    canonical = r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}"
    if (
        re.fullmatch(
            rf"(?:[0-9a-fA-F]{{32}}|{canonical}|\{{{canonical}\}}|urn:uuid:{canonical})", value
        )
        is None
    ):
        raise ValueError("invalid UUID format")
    return str(uuid.UUID(value))


def valid_message(value: str) -> str:
    if not is_snowflake(value):
        raise ValueError("invalid message ID")
    return value


Identifier = Annotated[str, AfterValidator(valid_uuid)]


class Claim(Strict):
    instanceId: str
    limit: int | None = Field(default=None, ge=0, le=2**32 - 1)


class Lease(Strict):
    instanceId: str
    notificationId: Identifier
    leaseToken: Identifier


class Ack(Lease):
    discordMessageId: Annotated[str, AfterValidator(valid_message)]


class Failure(Strict):
    kind: Literal["HTTP", "TRANSPORT", "DELIVERY_UNKNOWN"]
    status: int | None = Field(default=None, ge=0, le=65535)
    retryAfterMs: int | None = Field(default=None, ge=0, le=2**64 - 1)
    message: str


class Nack(Lease):
    failure: Failure


class Heartbeat(Strict):
    instanceId: str
    version: str
    commitSha: str | None = None


ERRORS = {
    "invalid_notification_request": (400, "Bot 알림 전송 요청 값이 올바르지 않습니다."),
    "notification_lease_conflict": (
        409,
        "알림 전송 lease가 만료되었거나 다른 Bot 인스턴스로 이전되었습니다.",
    ),
    "notification_type_mismatch": (422, "이 알림은 모집 메시지 확인으로 처리할 수 없습니다."),
    "recruitment_message_create_requires_typed_ack": (
        422,
        "모집 메시지 생성 알림은 전용 확인 경로로 처리해야 합니다.",
    ),
    "recruitment_message_bind_conflict": (
        409,
        "모집 메시지를 현재 모집 상태에 연결할 수 없습니다.",
    ),
    "notification_outbox_invariant_failed": (500, "저장된 알림 전송 상태를 처리할 수 없습니다."),
    "invalid_heartbeat": (422, "Bot heartbeat 요청 값이 올바르지 않습니다."),
}


def error(code: str) -> ApiError:
    status, message = ERRORS[code]
    if status == 500:
        logger.error("notification_outbox_invariant_failed", stack_info=True)
    return ApiError(status, code, message)


def owner_valid(value: str) -> bool:
    return re.fullmatch(r"[A-Za-z0-9._:-]{1,128}", value) is not None


def validate_owner(value: str) -> None:
    if not owner_valid(value):
        raise error("invalid_notification_request")


def validate_failure(failure: Failure) -> None:
    invalid = not failure.message.strip() or len(failure.message.encode()) > 1024
    if failure.kind == "HTTP":
        invalid |= failure.status is None or not 400 <= failure.status <= 599
        if failure.retryAfterMs is not None:
            invalid |= failure.status != 429 or not 1 <= failure.retryAfterMs <= 3_600_000
    else:
        invalid |= failure.status is not None or failure.retryAfterMs is not None
    if invalid:
        raise error("invalid_notification_request")


def validate_heartbeat(body: Heartbeat) -> None:
    if (
        not owner_valid(body.instanceId)
        or not body.instanceId[0].isascii()
        or not body.instanceId[0].isalnum()
        or re.fullmatch(r"[\x21-\x7e]{1,100}", body.version) is None
        or (
            body.commitSha is not None
            and re.fullmatch(r"[A-Za-z0-9._-]{1,128}", body.commitSha) is None
        )
    ):
        raise error("invalid_heartbeat")
