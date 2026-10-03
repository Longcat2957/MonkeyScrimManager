"""Discord guild setup and membership registration commands."""

import hashlib
import json
import re
import uuid
from datetime import UTC, datetime, timedelta
from typing import Any
from urllib.parse import urljoin

import httpx

from monkeybot_api.core.config import Settings
from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.text import trim
from monkeybot_api.db.database import Database
from monkeybot_api.db.transactions import mutation
from monkeybot_api.repositories import internal_guilds as repository
from monkeybot_api.schemas.bot import invalid_bot, validate_actor
from monkeybot_api.schemas.internal_guilds import (
    CanonicalRiotAccount,
    RegisterGuildMember,
    RegistrationResult,
    SetupGuild,
)

BOT_PRINCIPAL = "monkeybot-discord-bot"
SETUP_ROUTE = "/internal/guilds/{guildId}/setup"
REGISTRATION_ROUTE = "/internal/guilds/{guildId}/registrations"
PLATFORMS = {
    "br": ("BR1", "AMERICAS"),
    "br1": ("BR1", "AMERICAS"),
    "eune": ("EUN1", "EUROPE"),
    "eun1": ("EUN1", "EUROPE"),
    "euw": ("EUW1", "EUROPE"),
    "euw1": ("EUW1", "EUROPE"),
    "jp": ("JP1", "ASIA"),
    "jp1": ("JP1", "ASIA"),
    "kr": ("KR", "ASIA"),
    "lan": ("LA1", "AMERICAS"),
    "la1": ("LA1", "AMERICAS"),
    "las": ("LA2", "AMERICAS"),
    "la2": ("LA2", "AMERICAS"),
    "na": ("NA1", "AMERICAS"),
    "na1": ("NA1", "AMERICAS"),
    "oce": ("OC1", "AMERICAS"),
    "oc1": ("OC1", "AMERICAS"),
    "ph": ("PH2", "SEA"),
    "ph2": ("PH2", "SEA"),
    "ru": ("RU", "EUROPE"),
    "sg": ("SG2", "SEA"),
    "sg2": ("SG2", "SEA"),
    "th": ("TH2", "SEA"),
    "th2": ("TH2", "SEA"),
    "tr": ("TR1", "EUROPE"),
    "tr1": ("TR1", "EUROPE"),
    "tw": ("TW2", "SEA"),
    "tw2": ("TW2", "SEA"),
    "vn": ("VN2", "SEA"),
    "vn2": ("VN2", "SEA"),
}


def request_hash(guild: str, command: dict[str, Any]) -> bytes:
    return hashlib.sha256(
        json.dumps([guild, command], ensure_ascii=False, separators=(",", ":")).encode()
    ).digest()


def params(guild: str, actor_id: str, key: str, route: str, hash_value: bytes) -> dict[str, Any]:
    now = datetime.now(UTC)
    return {
        "guild": None if route == SETUP_ROUTE else guild,
        "guild_id": guild,
        "scope": "GLOBAL" if route == SETUP_ROUTE else guild,
        "actor_id": actor_id,
        "principal": BOT_PRINCIPAL,
        "route": route,
        "key": key,
        "hash": hash_value,
        "now": now,
        "expires": now + timedelta(hours=24),
        "idem_id": uuid.uuid7(),
    }


async def setup_guild(
    database: Database,
    guild: str,
    key: str,
    request_id: str,
    body: SetupGuild,
) -> dict[str, Any]:
    validate_actor(body.actor)
    for name in ("guildName", "seasonName"):
        value = getattr(body, name)
        if not trim(value) or len(value) > 100:
            raise invalid_bot(name)
        setattr(body, name, trim(value))
    if body.guildIconHash is not None and (
        not body.guildIconHash or len(body.guildIconHash.encode()) > 128
    ):
        raise invalid_bot("guildIconHash")
    if body.operatorRoleId == guild:
        raise invalid_bot("operatorRoleId")
    p = params(guild, body.actor.id, key, SETUP_ROUTE, request_hash(guild, body.model_dump()))
    p.update(
        {
            "name": body.guildName,
            "icon": body.guildIconHash,
            "season_name": body.seasonName,
            "season_id": uuid.uuid7(),
            "audit_id": uuid.uuid7(),
            "request_id": request_id,
        }
    )
    result = await mutation(database, lambda c: repository.setup(c, p, body))
    try:
        return {
            "guild": {
                "id": result["guildId"],
                "displayName": result["displayName"],
                "operatorRoleId": result["operatorRoleId"],
                "currentSeason": result["currentSeason"],
                "dashboardHref": result["dashboardHref"],
            }
        }
    except KeyError, TypeError:
        raise ApiError.from_status(500) from None


def riot_component(value: str, maximum: int) -> str:
    value = trim(value)
    if (
        not 1 <= len(value) <= maximum
        or "#" in value
        or any(ord(character) < 32 or 127 <= ord(character) <= 159 for character in value)
    ):
        raise ApiError(400, "invalid_riot_id", "GameName과 TagLine이 올바르지 않습니다.")
    return value


def parse_platform(value: str) -> tuple[str, str]:
    parsed = PLATFORMS.get(trim(value).lower())
    if parsed is None:
        raise ApiError(
            400,
            "invalid_region",
            "지원하지 않는 Riot 플랫폼입니다.",
            {"platform": value},
        )
    return parsed


async def register_guild_member(
    database: Database,
    settings: Settings,
    guild: str,
    key: str,
    body: RegisterGuildMember,
) -> dict[str, Any]:
    validate_actor(body.actor)
    platform, region = parse_platform(body.platform)
    game_name = riot_component(body.gameName, 64)
    tag_line = riot_component(body.tagLine, 16)
    command = {
        "actor": body.actor.model_dump(),
        "requestedGameName": game_name,
        "requestedTagLine": tag_line,
        "requestedPlatform": platform,
    }
    p = params(guild, body.actor.id, key, REGISTRATION_ROUTE, request_hash(guild, command))
    async with database.connection() as c:
        replay = await repository.preflight_registration(c, p)
    if replay is not None:
        return _registration_result(replay)
    account = await resolve_account(settings, game_name, tag_line, platform, region)
    return _registration_result(
        await mutation(database, lambda c: repository.register(c, p, body, account))
    )


def _registration_result(result: dict[str, Any]) -> dict[str, Any]:
    try:
        return RegistrationResult.model_validate(result).model_dump()
    except ValueError:
        raise ApiError.from_status(500) from None


def _upstream_failure(status: int, code: str | None, retry_after: str | None) -> ApiError:
    errors = {
        "unauthorized": (
            503,
            "riot_service_auth_failed",
            "Riot 검색 서비스 인증 설정이 일치하지 않습니다.",
        ),
        "invalid_request": (400, "invalid_riot_id", "GameName과 TagLine이 올바르지 않습니다."),
        "riot_account_not_found": (
            404,
            "riot_account_not_found",
            "요청한 Riot 계정을 찾을 수 없습니다.",
        ),
        "riot_timeout": (502, "riot_upstream_timeout", "Riot 조회 응답 시간이 초과되었습니다."),
        "riot_credentials_rejected": (
            503,
            "riot_credentials_rejected",
            "Riot API 인증 정보가 거부되었습니다.",
        ),
        "riot_unavailable": (503, "riot_unavailable", "Riot API를 일시적으로 사용할 수 없습니다."),
        "riot_bad_gateway": (
            502,
            "riot_upstream_invalid_response",
            "Riot API 응답을 처리할 수 없습니다.",
        ),
    }
    if code in errors:
        http_status, error_code, message = errors[code]
        return ApiError(http_status, error_code, message)
    if code == "riot_rate_limited" or status == 429:
        retry = (
            int(retry_after)
            if retry_after
            and len(retry_after) <= 20
            and retry_after.isascii()
            and retry_after.isdecimal()
            else None
        )
        return ApiError(
            429,
            "riot_rate_limited",
            "Riot 조회 요청이 제한되었습니다.",
            {"retryAfterSeconds": retry} if retry is not None and retry < 2**64 else None,
        )
    if status == 404:
        return ApiError(404, "riot_account_not_found", "요청한 Riot 계정을 찾을 수 없습니다.")
    return ApiError(
        502,
        "riot_upstream_failure",
        "Riot 검색 서비스가 요청을 처리하지 못했습니다.",
        {"upstreamStatus": status},
    )


def _malformed_upstream() -> ApiError:
    return ApiError(
        502, "riot_upstream_invalid_response", "Riot 검색 응답 형식이 올바르지 않습니다."
    )


async def resolve_account(
    settings: Settings, game_name: str, tag_line: str, platform: str, region: str
) -> CanonicalRiotAccount:
    return await _resolve_account_payload(
        settings,
        platform,
        region,
        {"kind": "riotId", "gameName": game_name, "tagLine": tag_line},
        None,
    )


async def resolve_account_by_puuid(
    settings: Settings, puuid: str, platform: str, region: str
) -> CanonicalRiotAccount:
    return await _resolve_account_payload(
        settings, platform, region, {"kind": "puuid", "puuid": puuid}, puuid
    )


async def _resolve_account_payload(
    settings: Settings,
    platform: str,
    region: str,
    lookup: dict[str, str],
    requested_puuid: str | None,
) -> CanonicalRiotAccount:
    if settings.riot_service_url is None or settings.riot_service_token is None:
        raise ApiError(503, "riot_unavailable", "Riot 검색 서비스를 사용할 수 없습니다.")
    url = urljoin(str(settings.riot_service_url), "accounts/resolve")
    token = settings.riot_service_token.get_secret_value()
    timeout = settings.riot_service_request_timeout_ms / 1000
    try:
        async with httpx.AsyncClient(timeout=timeout) as client:
            response = await client.post(
                url,
                headers={"Authorization": f"Bearer {token}"},
                json={"platform": platform, "lookup": lookup},
            )
    except httpx.TimeoutException:
        raise ApiError(
            502, "riot_upstream_timeout", "Riot 조회 응답 시간이 초과되었습니다."
        ) from None
    except httpx.RequestError:
        raise ApiError(503, "riot_unavailable", "Riot 검색 서비스에 연결할 수 없습니다.") from None
    if response.status_code < 200 or response.status_code >= 300:
        try:
            upstream_code = response.json()["error"]["code"]
        except ValueError, KeyError, TypeError:
            upstream_code = None
        raise _upstream_failure(
            response.status_code,
            upstream_code if isinstance(upstream_code, str) else None,
            response.headers.get("retry-after"),
        )
    try:
        data = response.json()
        account = data["account"]
        puuid = account["puuid"]
        canonical_game_name = account["gameName"]
        canonical_tag_line = account["tagLine"]
        canonical_platform = data["platform"]
        canonical_region = data["routingRegion"]
        icon = data.get("profileIconId")
        level = data.get("summonerLevel")
        synced = data["syncedAt"]
        if not all(
            isinstance(item, str)
            for item in (
                puuid,
                canonical_game_name,
                canonical_tag_line,
                canonical_platform,
                canonical_region,
                synced,
            )
        ):
            raise ValueError
        if not re.fullmatch(r"[A-Za-z0-9_-]{1,256}", puuid, flags=re.ASCII):
            raise ValueError
        riot_component(canonical_game_name, 64)
        riot_component(canonical_tag_line, 16)
        if (
            canonical_platform != platform
            or canonical_region != region
            or (requested_puuid is not None and puuid != requested_puuid)
        ):
            raise ValueError
        if icon is not None and (type(icon) is not int or icon < 0 or icon >= 2**31):
            raise ValueError
        if level is not None and (type(level) is not int or level < 0 or level >= 2**63):
            raise ValueError
        if not synced or not re.search(r"(?:Z|[+-]\d{2}:\d{2})$", synced):
            raise ValueError
        parsed = datetime.fromisoformat(synced.replace("Z", "+00:00"))
        if parsed.tzinfo is None:
            raise ValueError
        return CanonicalRiotAccount(
            puuid,
            canonical_platform,
            canonical_region,
            canonical_game_name,
            canonical_tag_line,
            icon,
            parsed,
        )
    except ValueError, KeyError, TypeError, AttributeError, ApiError:
        raise _malformed_upstream() from None
