import re

from fastapi import APIRouter, Request
from pydantic import BaseModel, ConfigDict
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection
from starlette.responses import Response

from monkeybot_api.auth import require_csrf
from monkeybot_api.config import Settings, is_snowflake
from monkeybot_api.database import Database
from monkeybot_api.errors import ApiError, success
from monkeybot_api.json_body import read_json
from monkeybot_api.session import resolve_session

LANES = ("TOP", "JUNGLE", "MID", "BOTTOM", "SUPPORT")
PRIVACY_COLUMNS = '''public_profile_enabled AS "publicProfile", show_riot_ids AS "showRiotIds",
                     allow_external_record_search AS "allowExternalRecordSearch"'''


class PrivacyInput(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)
    publicProfile: bool
    showRiotIds: bool
    allowExternalRecordSearch: bool


class ChampionsInput(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)
    championIds: list[str]


async def preferences(connection: AsyncConnection, user_id: str) -> dict[str, list[str]]:
    rows = await connection.execute(
        text(
            "SELECT lane, position, champion_id FROM user_champion_preferences "
            "WHERE user_id = :user_id ORDER BY lane, position"
        ),
        {"user_id": user_id},
    )
    snapshot: dict[str, list[str]] = {lane: [] for lane in LANES}
    for lane, position, champion in rows:
        if lane not in snapshot or position != len(snapshot[lane]) + 1:
            raise ApiError.from_status(500)
        snapshot[lane].append(champion)
    return snapshot


def validate_champions(ids: list[str]) -> None:
    if len(ids) > 10:
        raise ApiError(
            422,
            "too_many_champion_preferences",
            "한 포지션에는 선호 챔피언을 최대 10개까지 저장할 수 있습니다.",
            {"max": 10},
        )
    seen: set[str] = set()
    for champion in ids:
        if not re.fullmatch(r"[A-Za-z0-9]{1,64}", champion):
            raise ApiError(422, "invalid_champion_id", "챔피언 ID 형식이 올바르지 않습니다.")
        if champion in seen:
            raise ApiError(
                422,
                "duplicate_champion_id",
                "같은 챔피언을 한 포지션에 중복해서 저장할 수 없습니다.",
            )
        seen.add(champion)


def router(settings: Settings, database: Database) -> APIRouter:
    routes = APIRouter()

    @routes.api_route("/api/users/{discord_id}/profile", methods=["GET", "HEAD"])
    async def public_profile(request: Request, discord_id: str) -> Response:
        if not is_snowflake(discord_id):
            raise ApiError(400, "invalid_discord_id", "Discord 사용자 ID 형식이 올바르지 않습니다.")
        async with database.connection() as connection:
            row = (
                (
                    await connection.execute(
                        text("""
                SELECT identity.discord_id AS "discordId", app_user.display_name AS "displayName",
                       count(DISTINCT guild.guild_id) AS "activeGuildCount",
                       count(DISTINCT season.id) AS "seasonCount"
                FROM user_discord_identities identity
                JOIN users app_user ON app_user.user_id = identity.user_id
                  AND app_user.identity_kind = 'DISCORD'
                LEFT JOIN user_privacy_settings privacy ON privacy.user_id = app_user.user_id
                LEFT JOIN guild_members member ON member.user_id = app_user.user_id
                  AND member.status = 'ACTIVE'
                LEFT JOIN guilds guild ON guild.guild_id = member.guild_id
                  AND guild.status = 'ACTIVE'
                LEFT JOIN seasons season ON season.guild_id = guild.guild_id
                WHERE identity.discord_id = :discord_id AND identity.identity_kind = 'DISCORD'
                  AND app_user.deleted_at IS NULL AND COALESCE(privacy.public_profile_enabled, true)
                GROUP BY identity.discord_id, app_user.display_name
            """),
                        {"discord_id": discord_id},
                    )
                )
                .mappings()
                .one_or_none()
            )
        if row is None:
            raise ApiError(404, "not_found", "요청한 사용자 프로필을 찾을 수 없습니다.")
        if not row["displayName"].strip():
            raise ApiError.from_status(500)
        return success(request, dict(row))

    @routes.api_route("/api/me/privacy", methods=["GET", "HEAD"])
    async def privacy(request: Request) -> Response:
        principal = await resolve_session(request, database, settings)
        async with database.connection() as connection:
            row = (
                (
                    await connection.execute(
                        text(
                            f"SELECT {PRIVACY_COLUMNS} FROM user_privacy_settings "
                            "WHERE user_id = :user_id"
                        ),
                        {"user_id": principal["user_id"]},
                    )
                )
                .mappings()
                .one_or_none()
            )
        return success(
            request,
            {
                "settings": dict(row)
                if row is not None
                else {
                    "publicProfile": True,
                    "showRiotIds": True,
                    "allowExternalRecordSearch": False,
                }
            },
        )

    @routes.put("/api/me/privacy")
    async def update_privacy(request: Request) -> Response:
        principal = await resolve_session(request, database, settings)
        command = await read_json(request, PrivacyInput)
        require_csrf(request, settings, principal["csrf_secret_hash"])
        async with database.connection(transaction=True) as connection:
            row = (
                (
                    await connection.execute(
                        text(f"""
                INSERT INTO user_privacy_settings
                  (user_id, public_profile_enabled, show_riot_ids, allow_external_record_search,
                   updated_at)
                VALUES (:user_id, :publicProfile, :showRiotIds, :allowExternalRecordSearch,
                        clock_timestamp())
                ON CONFLICT (user_id) DO UPDATE
                SET public_profile_enabled = EXCLUDED.public_profile_enabled,
                    show_riot_ids = EXCLUDED.show_riot_ids,
                    allow_external_record_search = EXCLUDED.allow_external_record_search,
                    updated_at = EXCLUDED.updated_at
                RETURNING {PRIVACY_COLUMNS}
            """),
                        {"user_id": principal["user_id"], **command.model_dump()},
                    )
                )
                .mappings()
                .one()
            )
        return success(request, {"settings": dict(row)})

    @routes.api_route("/api/me/champion-preferences", methods=["GET", "HEAD"])
    async def champions(request: Request) -> Response:
        principal = await resolve_session(request, database, settings)
        async with database.connection() as connection:
            snapshot = await preferences(connection, principal["user_id"])
        return success(request, {"preferences": snapshot})

    @routes.put("/api/me/champion-preferences/{lane}")
    async def update_champions(request: Request, lane: str) -> Response:
        principal = await resolve_session(request, database, settings)
        command = await read_json(request, ChampionsInput)
        require_csrf(request, settings, principal["csrf_secret_hash"])
        if lane not in LANES:
            raise ApiError(400, "invalid_lane", "지원하지 않는 포지션입니다.", {"lane": lane})
        validate_champions(command.championIds)
        async with database.connection(transaction=True) as connection:
            # Serialize every lane mutation for this user, including the returned snapshot.
            locked = await connection.scalar(
                text(
                    "SELECT user_id FROM users WHERE user_id = :user_id "
                    "AND deleted_at IS NULL FOR UPDATE"
                ),
                {"user_id": principal["user_id"]},
            )
            if locked is None:
                raise ApiError.from_status(500)
            params = {"user_id": principal["user_id"], "lane": lane}
            await connection.execute(
                text(
                    "DELETE FROM user_champion_preferences "
                    "WHERE user_id = :user_id AND lane = :lane"
                ),
                params,
            )
            await connection.execute(
                text("""
                INSERT INTO user_champion_preferences (user_id, lane, position, champion_id)
                SELECT :user_id, :lane, requested.position::smallint, requested.champion_id
                FROM unnest(CAST(:ids AS text[]))
                WITH ORDINALITY AS requested(champion_id, position)
            """),
                {**params, "ids": command.championIds},
            )
            snapshot = await preferences(connection, principal["user_id"])
        return success(request, {"preferences": snapshot})

    return routes
