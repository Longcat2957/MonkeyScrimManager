import pytest
from pydantic import ValidationError

from monkeybot_api.schemas.notifications import Ack

CANONICAL = "67e55044-10b1-426f-9247-bb680e5fe0c8"
BODY = {
    "instanceId": "bot",
    "notificationId": CANONICAL,
    "leaseToken": CANONICAL,
    "discordMessageId": "987",
}


@pytest.mark.parametrize(
    "identifier",
    [
        CANONICAL.upper(),
        CANONICAL.replace("-", ""),
        "{" + CANONICAL + "}",
        "urn:uuid:" + CANONICAL,
    ],
)
def test_notification_uuid_accepts_rust_formats(identifier):
    request = Ack.model_validate({**BODY, "notificationId": identifier, "leaseToken": identifier})
    assert request.notificationId == CANONICAL and request.leaseToken == CANONICAL


@pytest.mark.parametrize(
    "identifier",
    [
        "{" + CANONICAL.replace("-", "") + "}",
        "urn:uuid:" + CANONICAL.replace("-", ""),
        CANONICAL + "-",
        CANONICAL.replace("-", "--", 1),
        "{" + CANONICAL,
    ],
)
def test_notification_uuid_rejects_non_rust_formats(identifier):
    with pytest.raises(ValidationError):
        Ack.model_validate({**BODY, "notificationId": identifier})
