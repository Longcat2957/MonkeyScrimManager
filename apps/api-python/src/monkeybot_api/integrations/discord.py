import asyncio
import logging
import re
from dataclasses import dataclass
from typing import Any
from urllib.parse import urlencode

import httpx

from monkeybot_api.core.config import Settings, is_snowflake
from monkeybot_api.core.errors import ApiError

logger = logging.getLogger(__name__)


@dataclass(frozen=True)
class DiscordUser:
    id: str
    display_name: str
    avatar_hash: str | None


def provider_failure(exchange: bool = False) -> ApiError:
    if exchange:
        return ApiError(
            400, "oauth_exchange_failed", "Discord 로그인을 완료할 수 없습니다. 다시 시도해 주세요."
        )
    return ApiError(
        503, "oauth_provider_unavailable", "Discord 인증 서비스를 일시적으로 사용할 수 없습니다."
    )


class DiscordClient:
    def __init__(
        self, settings: Settings, transport: httpx.AsyncBaseTransport | None = None
    ) -> None:
        self.settings = settings
        self.http = httpx.AsyncClient(
            transport=transport,
            timeout=httpx.Timeout(3, connect=2),
            follow_redirects=False,
            trust_env=False,
            headers={"user-agent": "MonkeyBot/0.1 (+https://monkeybot.lol)"},
        )

    @property
    def callback_url(self) -> str:
        return self.settings.public_origin + "/oauth/callback"

    def authorization_url(self, state: str, challenge: str) -> str:
        return "https://discord.com/oauth2/authorize?" + urlencode(
            {
                "response_type": "code",
                "client_id": self.settings.discord_application_id,
                "redirect_uri": self.callback_url,
                "scope": "identify",
                "state": state,
                "code_challenge": challenge,
                "code_challenge_method": "S256",
            }
        )

    async def _request(
        self, stage: str, request_id: str, method: str, url: str, **kwargs: Any
    ) -> Any:
        try:
            async with asyncio.timeout(3):
                response = await self.http.request(method, url, **kwargs)
        except (httpx.HTTPError, TimeoutError) as error:
            logger.warning(
                "oauth_transport_failed",
                extra={
                    "request_id": request_id,
                    "failure_type": type(error).__name__,
                    "stage": stage,
                },
            )
            raise provider_failure() from None
        if not response.is_success:
            logger.warning(
                "oauth_upstream_rejected",
                extra={
                    "request_id": request_id,
                    "status": response.status_code,
                    "stage": stage,
                },
            )
            raise provider_failure(stage == "token" and response.status_code == 400)
        try:
            return response.json()
        except ValueError:
            logger.warning("oauth_invalid_json", extra={"request_id": request_id, "stage": stage})
            raise provider_failure() from None

    async def authenticate(self, code: str, verifier: str, request_id: str) -> DiscordUser:
        if not code or len(code.encode()) > 2048:
            raise provider_failure(exchange=True)
        assert self.settings.discord_application_id is not None
        assert self.settings.discord_client_secret is not None
        token = await self._request(
            "token",
            request_id,
            "POST",
            "https://discord.com/api/v10/oauth2/token",
            auth=httpx.BasicAuth(
                self.settings.discord_application_id,
                self.settings.discord_client_secret.get_secret_value(),
            ),
            data={
                "grant_type": "authorization_code",
                "code": code,
                "redirect_uri": self.callback_url,
                "code_verifier": verifier,
            },
        )
        if not isinstance(token, dict) or not all(
            isinstance(token.get(key), str) for key in ("access_token", "token_type", "scope")
        ):
            raise provider_failure()
        if (
            not token["access_token"]
            or len(token["access_token"].encode()) > 4096
            or token["token_type"].lower() != "bearer"
            or "identify" not in re.split(r"[\t\n\r\f\v ]+", token["scope"])
        ):
            raise provider_failure()
        user = await self._request(
            "user",
            request_id,
            "GET",
            "https://discord.com/api/v10/users/@me",
            headers={"authorization": "Bearer " + token["access_token"]},
        )
        if (
            not isinstance(user, dict)
            or not isinstance(user.get("id"), str)
            or not is_snowflake(user["id"])
            or not isinstance(user.get("username"), str)
            or any(
                user.get(key) is not None and not isinstance(user[key], str)
                for key in ("global_name", "avatar")
            )
        ):
            raise provider_failure()
        name = user.get("global_name")
        if not name or not name.strip():
            name = user["username"]
        avatar = user.get("avatar") or None
        if (
            not name.strip()
            or len(name.encode()) > 128
            or (avatar is not None and not re.fullmatch(r"[0-9a-fA-F_]{1,128}", avatar))
        ):
            raise provider_failure()
        return DiscordUser(user["id"], name, avatar)
