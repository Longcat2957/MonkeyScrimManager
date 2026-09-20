import re
from datetime import UTC, datetime
from typing import Any

from fastapi import APIRouter, Request
from pydantic import BaseModel, ConfigDict, field_validator
from sqlalchemy import text
from starlette.responses import Response

from monkeybot_api.auth import require_csrf
from monkeybot_api.config import Settings, is_snowflake
from monkeybot_api.database import Database
from monkeybot_api.errors import ApiError, success
from monkeybot_api.json_body import read_json
from monkeybot_api.session import resolve_session

GUILDS_SQL = """
SELECT guild.guild_id AS id,
       COALESCE(NULLIF(config.display_name, ''), guild.discord_name) AS display_name,
       guild.icon_hash,
       (SELECT count(*) FROM guild_members AS counted_member
        WHERE counted_member.guild_id = guild.guild_id
          AND counted_member.status = 'ACTIVE') AS member_count,
       EXISTS (SELECT 1 FROM guild_member_discord_roles AS member_role
               WHERE member_role.guild_id = guild.guild_id AND member_role.user_id = :user_id
                 AND member_role.role_id = config.operator_role_id) AS is_operator,
       COALESCE(config.is_externally_public, false) AS is_externally_public
FROM guild_members AS current_membership
JOIN guilds AS guild
  ON guild.guild_id = current_membership.guild_id AND guild.status = 'ACTIVE'
LEFT JOIN guild_config AS config ON config.guild_id = guild.guild_id
WHERE current_membership.user_id = :user_id AND current_membership.status = 'ACTIVE'
"""


class DefaultGuildInput(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)
    guildId: str

    @field_validator("guildId")
    @classmethod
    def validate_guild_id(cls, value: str) -> str:
        if not is_snowflake(value):
            raise ValueError("invalid guild snowflake")
        return value


def guild_summary(row: dict[str, Any]) -> dict[str, Any]:
    icon = row["icon_hash"]
    icon_url = None
    if icon and re.fullmatch(r"[0-9a-fA-F_]{1,128}", icon):
        icon_url = f"https://cdn.discordapp.com/icons/{row['id']}/{icon}.webp?size=128"
    return {
        "id": row["id"],
        "displayName": row["display_name"],
        "iconUrl": icon_url,
        "memberCount": row["member_count"],
        "currentUserRole": "OPERATOR" if row["is_operator"] else "MEMBER",
    }


def timestamp(value: datetime) -> str:
    result = value.astimezone(UTC).isoformat().removesuffix("+00:00")
    if "." in result:
        result = result.rstrip("0").rstrip(".")
    return result + "Z"


def hidden_guild() -> ApiError:
    return ApiError(404, "not_found", "요청한 길드를 찾을 수 없습니다.")


def router(settings: Settings, database: Database) -> APIRouter:
    routes = APIRouter()

    @routes.api_route("/api/me/guilds", methods=["GET", "HEAD"])
    async def list_guilds(request: Request) -> Response:
        principal = await resolve_session(request, database, settings)
        async with database.connection() as connection:
            rows = (
                (
                    await connection.execute(
                        text(
                            GUILDS_SQL + " ORDER BY COALESCE(NULLIF(config.display_name, ''),"
                            " guild.discord_name), guild.guild_id"
                        ),
                        {"user_id": principal["user_id"]},
                    )
                )
                .mappings()
                .all()
            )
        return success(request, {"guilds": [guild_summary(dict(row)) for row in rows]})

    @routes.api_route("/api/guilds/{guild_id}", methods=["GET", "HEAD"])
    async def guild(request: Request, guild_id: str) -> Response:
        # Axum extracts the path before the authenticated session for this route.
        if not is_snowflake(guild_id):
            raise ApiError.from_status(400)
        principal = await resolve_session(request, database, settings)
        async with database.connection() as connection:
            # Read season and membership in the same READ COMMITTED statement snapshot.
            row = (
                (
                    await connection.execute(
                        text(
                            """
                SELECT context.*, season.id AS season_id, season.name AS season_name,
                       season.started_at AS season_started_at
                FROM (
            """
                            + GUILDS_SQL
                            + """
                    AND guild.guild_id = :guild_id
                ) AS context
                LEFT JOIN seasons AS season
                  ON season.guild_id = context.id AND season.status = 'ACTIVE'
            """
                        ),
                        {"user_id": principal["user_id"], "guild_id": guild_id},
                    )
                )
                .mappings()
                .one_or_none()
            )
        if row is None:
            raise hidden_guild()
        season = None
        if row["season_id"] is not None:
            season = {
                "id": str(row["season_id"]),
                "name": row["season_name"],
                "startedAt": timestamp(row["season_started_at"]),
            }
        return success(
            request,
            {
                **guild_summary(dict(row)),
                "isExternallyPublic": row["is_externally_public"],
                "currentSeason": season,
            },
        )

    @routes.put("/api/me/default-guild")
    async def default_guild(request: Request) -> Response:
        principal = await resolve_session(request, database, settings)
        command = await read_json(request, DefaultGuildInput)
        require_csrf(request, settings, principal["csrf_secret_hash"])
        async with database.connection(transaction=True) as connection:
            changed = await connection.scalar(
                text("""
                INSERT INTO user_guild_preferences (user_id, default_guild_id, updated_at)
                SELECT app_user.user_id, guild.guild_id, clock_timestamp()
                FROM users AS app_user
                JOIN guild_members AS membership
                  ON membership.user_id = app_user.user_id AND membership.guild_id = :guild_id
                 AND membership.status = 'ACTIVE'
                JOIN guilds AS guild
                  ON guild.guild_id = membership.guild_id AND guild.status = 'ACTIVE'
                WHERE app_user.user_id = :user_id AND app_user.deleted_at IS NULL
                ON CONFLICT (user_id) DO UPDATE
                SET default_guild_id = EXCLUDED.default_guild_id, updated_at = EXCLUDED.updated_at
                RETURNING default_guild_id
            """),
                {"user_id": principal["user_id"], "guild_id": command.guildId},
            )
        if changed is None:
            raise hidden_guild()
        return success(request, {"defaultGuildId": changed})

    return routes
