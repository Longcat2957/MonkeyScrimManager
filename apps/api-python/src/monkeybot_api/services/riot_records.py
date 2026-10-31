"""Public Riot records cache, refresh gate, and private service integration."""

import re
import uuid
from datetime import UTC, datetime
from typing import Any
from urllib.parse import urljoin

import httpx
from pydantic import ValidationError

from monkeybot_api.core.config import Settings
from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.riot_identity import normalize_riot_id
from monkeybot_api.db.database import Database
from monkeybot_api.integrations.riot.cache import valid_recent_matches
from monkeybot_api.integrations.riot.cache import validated as validated_records
from monkeybot_api.repositories import riot_records as repository
from monkeybot_api.schemas.riot_records import RecentPage
from monkeybot_api.services.internal_guilds import parse_platform, riot_component


def invalid_request() -> ApiError:
    return ApiError(400, "invalid_riot_records_request", "Riot 전적 조회 조건이 올바르지 않습니다.")


def lookup(game_name: str, tag_line: str, raw_platform: str) -> dict[str, str]:
    platform, region = parse_platform(raw_platform)
    try:
        name = riot_component(game_name, 64)
        tag = riot_component(tag_line, 16)
    except ApiError:
        raise invalid_request() from None
    normalized_name = normalize_riot_id(name)
    normalized_tag = normalize_riot_id(tag)
    if (
        not normalized_name
        or not normalized_tag
        or len(normalized_name) > 64
        or len(normalized_tag) > 16
    ):
        raise invalid_request()
    return {
        "platform": platform,
        "region": region,
        "game_name": name,
        "tag_line": tag,
        "normalized_name": normalized_name,
        "normalized_tag": normalized_tag,
    }


def _internal() -> ApiError:
    return ApiError.from_status(500)


def decode_cached(cached: dict[str, Any], platform: str) -> dict[str, Any]:
    if cached["schema_version"] != 1:
        raise _internal()
    payload = cached["snapshot"]
    record = validated_records(payload)
    if record is None or record.platform != platform or record.account.puuid != cached["puuid"]:
        raise _internal()
    result = record.model_dump()
    result["cache"]["state"] = "fresh"
    return result


def validate_records(value: Any, platform: str, region: str) -> dict[str, Any]:
    record = validated_records(value)
    if (
        record is None
        or record.platform != platform
        or record.routingRegion != region
        or not re.fullmatch(r"[A-Za-z0-9_-]{1,256}", record.account.puuid)
    ):
        raise ApiError(
            502, "riot_upstream_invalid_response", "Riot 검색 응답 형식이 올바르지 않습니다."
        )
    return record.model_dump()


def validate_page(
    value: Any, platform: str, region: str, puuid: str, offset: int, count: int
) -> dict[str, Any]:
    try:
        page = RecentPage.model_validate(value)
    except ValidationError:
        raise ApiError(
            502, "riot_upstream_invalid_response", "Riot 검색 응답 형식이 올바르지 않습니다."
        ) from None
    meta = page.page
    try:
        riot_component(page.account.gameName, 64)
        riot_component(page.account.tagLine, 16)
    except ApiError:
        raise ApiError(
            502, "riot_upstream_invalid_response", "Riot 검색 응답 형식이 올바르지 않습니다."
        ) from None
    if (
        not re.fullmatch(r"[A-Za-z0-9_-]{1,256}", page.account.puuid)
        or page.platform != platform
        or page.routingRegion != region
        or page.account.puuid != puuid
        or page.targetPuuid != puuid
        or len(page.matches) > count
        or meta.offset != offset
        or meta.requested != count
        or meta.returned != len(page.matches)
        or meta.returned > count
        or meta.nextOffset != (offset + count if meta.hasMore else None)
        or not valid_recent_matches(page.matches, puuid)
    ):
        raise ApiError(
            502, "riot_upstream_invalid_response", "Riot 검색 응답 형식이 올바르지 않습니다."
        )
    return page.model_dump()


def _retry_error(code: str, message: str, retry_at: datetime) -> ApiError:
    remaining = max(int((retry_at - datetime.now(UTC)).total_seconds()), 1)
    return ApiError(429, code, message, {"retryAfterSeconds": remaining})


async def private_request(
    settings: Settings,
    method: str,
    path: str,
    *,
    params: dict[str, Any] | None = None,
    body: dict[str, Any] | None = None,
) -> Any:
    if settings.riot_service_url is None or settings.riot_service_token is None:
        raise ApiError(503, "riot_unavailable", "Riot 검색 서비스를 사용할 수 없습니다.")
    url = urljoin(settings.riot_service_url, path)
    try:
        async with httpx.AsyncClient(
            timeout=settings.riot_service_request_timeout_ms / 1000
        ) as client:
            response = await client.request(
                method,
                url,
                params=params,
                json=body,
                headers={
                    "Authorization": f"Bearer {settings.riot_service_token.get_secret_value()}"
                },
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
        if upstream_code == "riot_account_not_found" or response.status_code == 404:
            raise ApiError(404, "riot_account_not_found", "요청한 Riot 계정을 찾을 수 없습니다.")
        if upstream_code == "riot_rate_limited" or response.status_code == 429:
            retry = response.headers.get("retry-after")
            details = (
                {"retryAfterSeconds": int(retry)}
                if retry is not None and retry.isascii() and retry.isdecimal()
                else None
            )
            raise ApiError(429, "riot_rate_limited", "Riot 조회 요청이 제한되었습니다.", details)
        if upstream_code == "unauthorized" or response.status_code in {401, 403}:
            raise ApiError(
                503, "riot_service_auth_failed", "Riot 검색 서비스 인증 설정이 일치하지 않습니다."
            )
        if upstream_code == "riot_timeout":
            raise ApiError(502, "riot_upstream_timeout", "Riot 조회 응답 시간이 초과되었습니다.")
        if upstream_code == "riot_credentials_rejected":
            raise ApiError(503, "riot_credentials_rejected", "Riot API 인증 정보가 거부되었습니다.")
        if upstream_code == "riot_unavailable":
            raise ApiError(503, "riot_unavailable", "Riot API를 일시적으로 사용할 수 없습니다.")
        raise ApiError(
            502,
            "riot_upstream_failure",
            "Riot 검색 서비스가 요청을 처리하지 못했습니다.",
            {"upstreamStatus": response.status_code},
        )
    try:
        return response.json()
    except ValueError:
        raise ApiError(
            502, "riot_upstream_invalid_response", "Riot 검색 응답 형식이 올바르지 않습니다."
        ) from None


async def _load(
    database: Database, settings: Settings, query: dict[str, str], refresh: bool
) -> dict[str, Any]:
    # The Rust handler requires a configured private service even for cache reads.
    if settings.riot_service_url is None or settings.riot_service_token is None:
        raise ApiError(503, "riot_unavailable", "Riot 검색 서비스를 사용할 수 없습니다.")
    async with database.connection() as c:
        cached = await repository.snapshot(c, query)
    if cached is not None and not refresh:
        return decode_cached(cached, query["platform"])
    lease = await repository.acquire_refresh(database, query)
    if lease["state"] == "cooldown":
        if refresh:
            raise _retry_error(
                "riot_records_refresh_cooldown",
                "전적은 30초마다 갱신할 수 있습니다.",
                lease["retry_at"],
            )
        return decode_cached(lease["snapshot"], query["platform"])
    if lease["state"] == "in_progress":
        if not refresh and lease["snapshot"] is not None:
            return decode_cached(lease["snapshot"], query["platform"])
        raise _retry_error(
            "riot_records_refresh_in_progress",
            "다른 전적 갱신 요청을 처리하고 있습니다.",
            lease["retry_at"],
        )
    token: uuid.UUID = lease["token"]
    try:
        value = await private_request(
            settings,
            "POST" if refresh else "GET",
            "records/refresh" if refresh else "records",
            params=None
            if refresh
            else {
                "gameName": query["game_name"],
                "tagLine": query["tag_line"],
                "platform": query["platform"],
            },
            body={
                "gameName": query["game_name"],
                "tagLine": query["tag_line"],
                "platform": query["platform"],
            }
            if refresh
            else None,
        )
        record = validate_records(value, query["platform"], query["region"])
    except ApiError:
        await repository.abort_refresh(database, query, token)
        if cached is not None:
            stale = decode_cached(cached, query["platform"])
            stale["cache"]["state"] = "stale"
            return stale
        raise
    account = record["account"]
    normalized_name = normalize_riot_id(account["gameName"])
    normalized_tag = normalize_riot_id(account["tagLine"])
    try:
        await repository.complete_refresh(
            database, query, token, record, normalized_name, normalized_tag, query["region"]
        )
    except Exception:
        await repository.abort_refresh(database, query, token)
        raise
    return record


async def records(
    database: Database,
    settings: Settings,
    game_name: str,
    tag_line: str,
    platform: str,
    *,
    refresh: bool = False,
) -> dict[str, Any]:
    return await _load(database, settings, lookup(game_name, tag_line, platform), refresh)


async def recent_matches(
    database: Database,
    settings: Settings,
    game_name: str,
    tag_line: str,
    platform: str,
    offset: int,
    count: int,
) -> dict[str, Any]:
    if not 1 <= count <= 20 or not 0 <= offset <= 10_000:
        raise invalid_request()
    query = lookup(game_name, tag_line, platform)
    async with database.connection() as c:
        cached = await repository.snapshot(c, query)
    if cached is None:
        await _load(database, settings, query, False)
        async with database.connection() as c:
            cached = await repository.snapshot(c, query)
        if cached is None:
            raise _internal()
    async with database.connection() as c:
        page = await repository.recent_page(c, query, offset, count)
    if page is not None:
        if page["schema_version"] != 1:
            raise _internal()
        try:
            return validate_page(
                page["page"],
                query["platform"],
                query["region"],
                cached["puuid"],
                offset,
                count,
            )
        except ApiError:
            raise _internal() from None
    value = await private_request(
        settings,
        "GET",
        "records/matches",
        params={
            "gameName": query["game_name"],
            "tagLine": query["tag_line"],
            "platform": query["platform"],
            "start": offset,
            "count": count,
        },
    )
    result = validate_page(
        value, query["platform"], query["region"], cached["puuid"], offset, count
    )
    await repository.store_page(database, query, offset, count, result)
    return result
