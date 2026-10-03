"""Reusable fixtures and request builders; no test module imports."""

from http.cookies import SimpleCookie
from urllib.parse import parse_qs, urlsplit

import httpx

USER_ID = "328516947210438656"


ORIGIN = "http://localhost:5173"


def cookies(response: httpx.Response) -> dict[str, str]:
    result = {}
    for header in response.headers.get_list("set-cookie"):
        parsed = SimpleCookie()
        parsed.load(header)
        result.update({name: value.value for name, value in parsed.items()})
    return result


async def start_login(client: httpx.AsyncClient, next_path: str = "/me") -> tuple[str, dict]:
    response = await client.get("/oauth/discord/start", params={"next": next_path})
    assert response.status_code == 302
    query = parse_qs(urlsplit(response.headers["location"]).query)
    assert query["scope"] == ["identify"]
    assert query["redirect_uri"] == [ORIGIN + "/oauth/callback"]
    assert query["code_challenge_method"] == ["S256"]
    state = query["state"][0]
    assert cookies(response)["monkeybot_oauth_state"] == state
    cookie = response.headers["set-cookie"]
    assert "Path=/oauth/callback" in cookie and "HttpOnly" in cookie and "Max-Age=300" in cookie
    return state, query


async def finish_login(client: httpx.AsyncClient, state: str, previous: str | None = None):
    header = f"monkeybot_oauth_state={state}"
    if previous is not None:
        header += f"; monkeybot_session={previous}"
    return await client.get(
        "/oauth/callback", params={"state": state, "code": "test-code"}, headers={"cookie": header}
    )
