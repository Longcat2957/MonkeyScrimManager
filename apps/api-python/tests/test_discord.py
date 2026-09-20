import httpx
import pytest
from pydantic import SecretStr

from monkeybot_api.config import Settings
from monkeybot_api.discord import DiscordClient
from monkeybot_api.errors import ApiError


@pytest.mark.parametrize(
    "scenario",
    [
        "wrong_scope",
        "empty_token",
        "wrong_token_type",
        "malformed_json",
        "timeout",
        "numeric_id",
        "blank_name",
        "long_utf8_name",
        "invalid_avatar",
        "redirect",
    ],
)
async def test_provider_contract_failures_are_sanitized(scenario: str) -> None:
    settings = Settings(
        env="test",
        public_origin="http://localhost",
        discord_application_id="123",
        discord_client_secret=SecretStr("test-only"),
        oauth_encryption_key=SecretStr("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
    )

    def upstream(request: httpx.Request) -> httpx.Response:
        if scenario == "timeout":
            raise httpx.ReadTimeout("sensitive upstream response", request=request)
        if scenario == "malformed_json":
            return httpx.Response(200, content="sensitive upstream response")
        if scenario == "redirect":
            return httpx.Response(302, headers={"location": "https://untrusted.example"})
        if request.url.path.endswith("/token"):
            token = {"access_token": "test-only", "token_type": "Bearer", "scope": "identify"}
            if scenario == "wrong_scope":
                token["scope"] = "guilds"
            if scenario == "empty_token":
                token["access_token"] = ""
            if scenario == "wrong_token_type":
                token["token_type"] = "Basic"
            return httpx.Response(200, json=token)
        user = {"id": "123", "username": "tester", "global_name": None, "avatar": None}
        if scenario == "numeric_id":
            user["id"] = 123
        if scenario == "blank_name":
            user["username"] = " "
        if scenario == "long_utf8_name":
            user["global_name"] = "가" * 43
        if scenario == "invalid_avatar":
            user["avatar"] = "../../secret"
        return httpx.Response(200, json=user)

    client = DiscordClient(settings, httpx.MockTransport(upstream))
    try:
        with pytest.raises(ApiError) as error:
            await client.authenticate("code", "v" * 43, "test-request-id")
        assert error.value.status == 503
        assert error.value.code == "oauth_provider_unavailable"
        assert error.value.message == "Discord 인증 서비스를 일시적으로 사용할 수 없습니다."
        assert "sensitive" not in str(error.value)
    finally:
        await client.http.aclose()
