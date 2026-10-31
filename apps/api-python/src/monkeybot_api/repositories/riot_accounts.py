"""Current user's PUUID-owned Riot account persistence."""

from typing import Any

from sqlalchemy import text
from sqlalchemy.exc import IntegrityError
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.riot_identity import normalize_riot_id
from monkeybot_api.core.time import timestamp
from monkeybot_api.repositories import internal_guilds
from monkeybot_api.schemas.internal_guilds import CanonicalRiotAccount


def not_found() -> ApiError:
    return ApiError(404, "riot_account_not_found", "요청한 Riot 계정을 찾을 수 없습니다.")


async def list_accounts(c: AsyncConnection, user: str) -> dict[str, Any]:
    rows = (
        (
            await c.execute(
                text("""
                SELECT puuid, platform, routing_region, game_name, tag_line, profile_icon_id,
                       is_main, main_position, last_synced_at, created_at, updated_at
                FROM riot_accounts WHERE user_id = :user AND origin = 'RIOT'
                ORDER BY is_main DESC, created_at, puuid
            """),
                {"user": user},
            )
        )
        .mappings()
        .all()
    )
    if sum(bool(row["is_main"]) for row in rows) != (1 if rows else 0):
        raise ApiError.from_status(500)
    return {
        "riotAccounts": [
            {
                "puuid": row["puuid"],
                "platform": row["platform"],
                "routingRegion": row["routing_region"],
                "gameName": row["game_name"],
                "tagLine": row["tag_line"],
                "profileIconId": row["profile_icon_id"],
                "isMain": row["is_main"],
                "mainPosition": row["main_position"],
                "lastSyncedAt": timestamp(row["last_synced_at"])
                if row["last_synced_at"] is not None
                else None,
                "createdAt": timestamp(row["created_at"]),
                "updatedAt": timestamp(row["updated_at"]),
            }
            for row in rows
        ]
    }


async def stored_platform(c: AsyncConnection, user: str, puuid: str) -> str:
    platform = await c.scalar(
        text("""
        SELECT platform FROM riot_accounts
        WHERE user_id = :user AND puuid = :puuid AND origin = 'RIOT'
    """),
        {"user": user, "puuid": puuid},
    )
    if platform is None:
        raise not_found()
    return str(platform)


async def _lock_user(c: AsyncConnection, user: str) -> None:
    found = await c.scalar(
        text("""
        SELECT user_id FROM users
        WHERE user_id = :user AND identity_kind = 'DISCORD' AND deleted_at IS NULL FOR UPDATE
    """),
        {"user": user},
    )
    if found is None:
        raise ApiError.from_status(500)


async def _require_owned(c: AsyncConnection, user: str, puuid: str) -> None:
    found = await c.scalar(
        text("""
        SELECT puuid FROM riot_accounts
        WHERE user_id = :user AND puuid = :puuid AND origin = 'RIOT' FOR UPDATE
    """),
        {"user": user, "puuid": puuid},
    )
    if found is None:
        raise not_found()


async def _ensure_one_main(c: AsyncConnection, user: str) -> None:
    counts = (
        (
            await c.execute(
                text("""
                SELECT count(*) AS account_count, count(*) FILTER (WHERE is_main) AS main_count
                FROM riot_accounts WHERE user_id = :user AND origin = 'RIOT'
            """),
                {"user": user},
            )
        )
        .mappings()
        .one()
    )
    if counts["main_count"] > 1:
        raise ApiError.from_status(500)
    if counts["account_count"] > 0 and counts["main_count"] == 0:
        await c.execute(
            text("""
            UPDATE riot_accounts SET is_main = true, updated_at = clock_timestamp()
            WHERE puuid = (
                SELECT puuid FROM riot_accounts WHERE user_id = :user AND origin = 'RIOT'
                ORDER BY created_at, puuid LIMIT 1
            )
        """),
            {"user": user},
        )


async def link(c: AsyncConnection, user: str, account: CanonicalRiotAccount) -> dict[str, Any]:
    await internal_guilds.link_account(c, user, account)
    return await list_accounts(c, user)


async def set_main(c: AsyncConnection, user: str, puuid: str) -> dict[str, Any]:
    await _lock_user(c, user)
    await _require_owned(c, user, puuid)
    await c.execute(
        text("""
        UPDATE riot_accounts SET is_main = false, updated_at = clock_timestamp()
        WHERE user_id = :user AND is_main AND origin = 'RIOT'
    """),
        {"user": user},
    )
    await c.execute(
        text("""
        UPDATE riot_accounts SET is_main = true, updated_at = clock_timestamp()
        WHERE user_id = :user AND puuid = :puuid AND origin = 'RIOT'
    """),
        {"user": user, "puuid": puuid},
    )
    await _ensure_one_main(c, user)
    return await list_accounts(c, user)


async def delete(c: AsyncConnection, user: str, puuid: str) -> dict[str, Any]:
    await _lock_user(c, user)
    deleted = await c.scalar(
        text("""
        DELETE FROM riot_accounts
        WHERE user_id = :user AND puuid = :puuid AND origin = 'RIOT' RETURNING puuid
    """),
        {"user": user, "puuid": puuid},
    )
    if deleted is None:
        raise not_found()
    await _ensure_one_main(c, user)
    return await list_accounts(c, user)


async def refresh(c: AsyncConnection, user: str, account: CanonicalRiotAccount) -> dict[str, Any]:
    await _lock_user(c, user)
    await _require_owned(c, user, account.puuid)
    values = {
        "user": user,
        "puuid": account.puuid,
        "platform": account.platform,
        "region": account.routing_region,
        "game_name": account.game_name,
        "tag_line": account.tag_line,
        "normalized_game_name": normalize_riot_id(account.game_name),
        "normalized_tag_line": normalize_riot_id(account.tag_line),
        "icon": account.profile_icon_id,
        "synced": account.last_synced_at,
    }
    if not values["normalized_game_name"] or not values["normalized_tag_line"]:
        raise ApiError.from_status(500)
    try:
        updated = await c.execute(
            text("""
            UPDATE riot_accounts
            SET platform = :platform, routing_region = :region, game_name = :game_name,
                tag_line = :tag_line, normalized_game_name = :normalized_game_name,
                normalized_tag_line = :normalized_tag_line, profile_icon_id = :icon,
                last_synced_at = :synced, updated_at = clock_timestamp()
            WHERE puuid = :puuid AND user_id = :user AND origin = 'RIOT'
        """),
            values,
        )
    except IntegrityError as exc:
        raise internal_guilds.account_constraint_error(exc) from None
    if updated.rowcount != 1:
        raise not_found()
    await _ensure_one_main(c, user)
    return await list_accounts(c, user)
