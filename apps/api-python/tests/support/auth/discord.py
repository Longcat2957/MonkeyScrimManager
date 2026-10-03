"""Deterministic Discord OAuth transport for authentication tests."""

import httpx


class DiscordStub:
    def __init__(self) -> None:
        self.requests: list[httpx.Request] = []
        self.token_status = 200
        self.user_status = 200
        self.token = {
            "access_token": "test-provider-token",
            "token_type": "Bearer",
            "scope": "identify",
        }
        self.user = {
            "id": "328516947210438656",
            "username": "test-user",
            "global_name": "OAuth 사용자",
            "avatar": "a_0123abcdef",
        }

    def __call__(self, request: httpx.Request) -> httpx.Response:
        self.requests.append(request)
        if request.url.path == "/api/v10/oauth2/token":
            return httpx.Response(self.token_status, json=self.token)
        assert request.url.path == "/api/v10/users/@me"
        assert request.headers["authorization"] == "Bearer test-provider-token"
        return httpx.Response(self.user_status, json=self.user)
