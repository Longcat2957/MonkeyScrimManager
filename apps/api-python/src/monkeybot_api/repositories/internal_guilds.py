"""Transactional guild setup and Discord registration persistence."""

import json
from typing import Any

from sqlalchemy import text
from sqlalchemy.exc import IntegrityError
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.riot_identity import normalize_riot_id
from monkeybot_api.repositories.effects import audit
from monkeybot_api.repositories.members import sync_actor
from monkeybot_api.schemas.internal_guilds import (
    CanonicalRiotAccount,
    RegisterGuildMember,
    SetupGuild,
)


def _setup_required() -> ApiError:
    return ApiError(409, "guild_setup_required", "길드 초기 설정을 먼저 완료해야 합니다.")


def _inactive(status: str) -> ApiError:
    return ApiError(
        409,
        "guild_inactive",
        "현재 길드에서는 command를 실행할 수 없습니다.",
        {"currentStatus": status},
    )


async def preflight_registration(c: AsyncConnection, p: dict[str, Any]) -> dict[str, Any] | None:
    row = (
        (
            await c.execute(
                text("""
                SELECT request_hash, state, status_code, response_body
                FROM idempotency_records
                WHERE scope_key = :scope AND guild_id IS NOT DISTINCT FROM CAST(:guild AS text)
                  AND principal_type = 'BOT' AND principal_id = :principal
                  AND method = 'POST' AND route_template = :route AND idempotency_key = :key
                  AND expires_at > :now
            """),
                p,
            )
        )
        .mappings()
        .one_or_none()
    )
    if row is not None:
        return _replay(row, p["hash"])
    eligibility = (
        (
            await c.execute(
                text("""
                SELECT guild.status, config.guild_id IS NOT NULL AS configured,
                       member.status AS member_status
                FROM guilds AS guild
                LEFT JOIN guild_config AS config ON config.guild_id = guild.guild_id
                LEFT JOIN guild_members AS member
                  ON member.guild_id = guild.guild_id AND member.user_id = :actor_id
                WHERE guild.guild_id = :guild
            """),
                p,
            )
        )
        .mappings()
        .one_or_none()
    )
    if eligibility is None:
        raise _setup_required()
    if eligibility["status"] != "ACTIVE":
        raise _inactive(eligibility["status"])
    if not eligibility["configured"]:
        raise _setup_required()
    if eligibility["member_status"] == "INACTIVE":
        raise ApiError(
            403, "member_inactive", "비활성화된 길드 멤버는 이 작업을 수행할 수 없습니다."
        )
    return None


def _replay(row: Any, request_hash: bytes) -> dict[str, Any]:
    if bytes(row["request_hash"]) != request_hash:
        raise ApiError(
            409, "idempotency_conflict", "같은 interaction ID가 다른 요청에 사용되었습니다."
        )
    if row["state"] == "IN_PROGRESS":
        raise ApiError(409, "idempotency_in_progress", "같은 interaction 요청을 처리하고 있습니다.")
    if row["state"] != "COMPLETED" or row["status_code"] != 200:
        raise ApiError.from_status(500)
    result = row["response_body"]
    if not isinstance(result, dict):
        raise ApiError.from_status(500)
    return dict(result)


async def acquire(c: AsyncConnection, p: dict[str, Any]) -> dict[str, Any] | None:
    locked = await c.scalar(
        text("""
        SELECT pg_try_advisory_xact_lock(hashtextextended(jsonb_build_array(
            'monkeybot:idempotency:v1', CAST(:scope AS text), 'BOT', CAST(:principal AS text),
            'POST', CAST(:route AS text), CAST(:key AS text)
        )::text, 0))
    """),
        p,
    )
    if not locked:
        raise ApiError(409, "idempotency_in_progress", "같은 interaction 요청을 처리하고 있습니다.")
    await c.execute(
        text("""
        DELETE FROM idempotency_records
        WHERE scope_key = :scope AND guild_id IS NOT DISTINCT FROM CAST(:guild AS text)
          AND principal_type = 'BOT' AND principal_id = :principal AND method = 'POST'
          AND route_template = :route AND idempotency_key = :key AND expires_at <= :now
    """),
        p,
    )
    inserted = await c.scalar(
        text("""
        INSERT INTO idempotency_records
          (id, guild_id, scope_key, principal_type, principal_id, method, route_template,
           idempotency_key, request_hash, state, created_at, expires_at)
        VALUES (:idem_id, :guild, :scope, 'BOT', :principal, 'POST', :route,
                :key, :hash, 'IN_PROGRESS', :now, :expires)
        ON CONFLICT ON CONSTRAINT idempotency_records_command_key DO NOTHING RETURNING id
    """),
        p,
    )
    if inserted is not None:
        return None
    row = (
        (
            await c.execute(
                text("""
                SELECT request_hash, state, status_code, response_body
                FROM idempotency_records
                WHERE scope_key = :scope AND guild_id IS NOT DISTINCT FROM CAST(:guild AS text)
                  AND principal_type = 'BOT' AND principal_id = :principal AND method = 'POST'
                  AND route_template = :route AND idempotency_key = :key FOR UPDATE
            """),
                p,
            )
        )
        .mappings()
        .one_or_none()
    )
    if row is None:
        raise ApiError.from_status(500)
    return _replay(row, p["hash"])


async def complete(c: AsyncConnection, p: dict[str, Any], result: dict[str, Any]) -> None:
    identifier = await c.scalar(
        text("""
        UPDATE idempotency_records
        SET state = 'COMPLETED', status_code = 200, response_body = CAST(:response AS jsonb),
            completed_at = :now
        WHERE id = :idem_id AND scope_key = :scope
          AND guild_id IS NOT DISTINCT FROM CAST(:guild AS text) AND state = 'IN_PROGRESS'
        RETURNING id
    """),
        {**p, "response": json.dumps(result, ensure_ascii=False)},
    )
    if identifier is None:
        raise ApiError.from_status(500)


async def lock_active_guild(c: AsyncConnection, guild: str) -> None:
    status = await c.scalar(
        text("SELECT status FROM guilds WHERE guild_id = :guild FOR UPDATE"), {"guild": guild}
    )
    if status is None:
        raise _setup_required()
    if status != "ACTIVE":
        raise _inactive(status)
    config = await c.scalar(
        text("SELECT operator_role_id FROM guild_config WHERE guild_id = :guild FOR UPDATE"),
        {"guild": guild},
    )
    if config is None:
        raise _setup_required()


async def setup(c: AsyncConnection, p: dict[str, Any], body: SetupGuild) -> dict[str, Any]:
    replay = await acquire(c, p)
    if replay is not None:
        return replay
    row = await c.scalar(text("SELECT status FROM guilds WHERE guild_id = :guild_id FOR UPDATE"), p)
    if row is not None:
        await _check_setup_available(c, p["guild_id"], row)
        await c.execute(
            text("""
            UPDATE guilds SET discord_name = :name, icon_hash = :icon, status = 'ACTIVE',
                activated_at = COALESCE(activated_at, :now), left_at = NULL, updated_at = :now
            WHERE guild_id = :guild_id
        """),
            p,
        )
    else:
        inserted = await c.scalar(
            text("""
            INSERT INTO guilds (guild_id, discord_name, icon_hash, status, installed_by,
                installed_at, activated_at, left_at, updated_at)
            VALUES (:guild_id, :name, :icon, 'ACTIVE', NULL, :now, :now, NULL, :now)
            ON CONFLICT (guild_id) DO NOTHING RETURNING guild_id
        """),
            p,
        )
        if inserted is None:
            status = await c.scalar(
                text("SELECT status FROM guilds WHERE guild_id = :guild_id FOR UPDATE"), p
            )
            if status is None:
                raise ApiError.from_status(500)
            await _check_setup_available(c, p["guild_id"], status)
            await c.execute(
                text("""
                UPDATE guilds SET discord_name = :name, icon_hash = :icon, status = 'ACTIVE',
                    activated_at = COALESCE(activated_at, :now), left_at = NULL, updated_at = :now
                WHERE guild_id = :guild_id
            """),
                p,
            )
    actor_params = {**p, "guild": p["guild_id"]}
    await sync_actor(c, body.actor, actor_params)
    await c.execute(
        text("UPDATE guilds SET installed_by = :actor_id WHERE guild_id = :guild_id"), p
    )
    await c.execute(
        text("""
        INSERT INTO guild_config
          (guild_id, display_name, operator_role_id, recruitment_channel_id,
           entry_channel_id, result_channel_id, log_channel_id, is_externally_public,
           revision, updated_by, updated_at)
        VALUES (:guild_id, :name, :operatorRoleId, :recruitmentChannelId,
                :entryChannelId, :resultChannelId, :logChannelId, false, 0, :actor_id, :now)
    """),
        {**p, **body.model_dump()},
    )
    season = (
        (
            await c.execute(
                text("""
                SELECT id, name FROM seasons WHERE guild_id = :guild_id AND status = 'ACTIVE'
                FOR UPDATE
            """),
                p,
            )
        )
        .mappings()
        .one_or_none()
    )
    if season is None:
        await c.execute(
            text("""
            INSERT INTO seasons
              (guild_id, id, name, status, started_at, ended_at, created_by, created_at)
            VALUES (:guild_id, :season_id, :season_name, 'ACTIVE', :now, NULL, :actor_id, :now)
        """),
            p,
        )
        current_season = {"id": str(p["season_id"]), "name": p["season_name"]}
    else:
        current_season = {"id": str(season["id"]), "name": season["name"]}
    for counter in ("RECRUITMENT", "SERIES", "AUCTION_RECRUITMENT", "AUCTION_TOURNAMENT"):
        await c.execute(
            text("""
            INSERT INTO guild_counters (guild_id, counter_key, next_value)
            VALUES (:guild_id, :counter, 1) ON CONFLICT (guild_id, counter_key) DO NOTHING
        """),
            {**p, "counter": counter},
        )
    result = {
        "guildId": p["guild_id"],
        "displayName": p["name"],
        "operatorRoleId": body.operatorRoleId,
        "currentSeason": current_season,
        "configRevision": 0,
        "dashboardHref": f"/g/{p['guild_id']}",
    }
    await audit(c, actor_params, "GUILD_SETUP", "GUILD", p["guild_id"], None, result)
    await complete(c, p, result)
    return result


async def _check_setup_available(c: AsyncConnection, guild: str, status: str) -> None:
    configured = await c.scalar(
        text("SELECT EXISTS(SELECT 1 FROM guild_config WHERE guild_id = :guild)"),
        {"guild": guild},
    )
    if status == "ACTIVE" or configured:
        raise ApiError(
            409,
            "guild_already_configured",
            "이미 초기 설정이 완료된 길드입니다. 길드 관리 화면에서 설정을 변경해 주세요.",
        )
    if status in {"DISABLED", "LEFT"}:
        raise ApiError(
            409,
            "guild_setup_unavailable",
            "비활성화되었거나 연결이 해제된 길드는 초기 설정을 실행할 수 없습니다.",
            {"currentStatus": status},
        )
    if status not in {"INSTALLED", "SETUP_REQUIRED"}:
        raise ApiError.from_status(500)


async def register(
    c: AsyncConnection, p: dict[str, Any], body: RegisterGuildMember, account: CanonicalRiotAccount
) -> dict[str, Any]:
    await lock_active_guild(c, p["guild"])
    replay = await acquire(c, p)
    if replay is not None:
        return replay
    existing_status = await c.scalar(
        text("""
        SELECT status FROM guild_members WHERE guild_id = :guild AND user_id = :actor_id
        FOR UPDATE
    """),
        p,
    )
    if existing_status == "INACTIVE":
        raise ApiError(
            403, "member_inactive", "비활성화된 길드 멤버는 이 작업을 수행할 수 없습니다."
        )
    await sync_actor(c, body.actor, p)
    link_state, linked = await link_account(c, p["actor_id"], account)
    result = {
        "member": {"id": body.actor.id, "displayName": body.actor.displayName, "status": "ACTIVE"},
        "riotAccount": {
            "puuid": linked["puuid"],
            "gameName": linked["game_name"],
            "tagLine": linked["tag_line"],
            "platform": linked["platform"],
            "isMain": linked["is_main"],
        },
        "registration": "EXISTING" if existing_status is not None else "CREATED",
        "accountLink": link_state,
    }
    await complete(c, p, result)
    return result


async def link_account(
    c: AsyncConnection, user_id: str, account: CanonicalRiotAccount
) -> tuple[str, dict[str, Any]]:
    user = await c.scalar(
        text("""
        SELECT user_id FROM users
        WHERE user_id = :user AND identity_kind = 'DISCORD' AND deleted_at IS NULL FOR UPDATE
    """),
        {"user": user_id},
    )
    if user is None:
        raise ApiError.from_status(500)
    owner = await c.scalar(
        text(
            "SELECT user_id FROM riot_accounts WHERE puuid = :puuid AND origin = 'RIOT' FOR UPDATE"
        ),
        {"puuid": account.puuid},
    )
    if owner is not None and owner != user_id:
        raise ApiError(
            409,
            "riot_account_already_linked",
            "이 Riot 계정은 이미 다른 사용자에게 연결되어 있습니다.",
        )
    values = {
        "puuid": account.puuid,
        "user": user_id,
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
    if owner is None:
        has_account = await c.scalar(
            text(
                "SELECT EXISTS(SELECT 1 FROM riot_accounts "
                "WHERE user_id = :user AND origin = 'RIOT')"
            ),
            values,
        )
        try:
            await c.execute(
                text("""
                INSERT INTO riot_accounts
                  (puuid, user_id, platform, routing_region, game_name, tag_line,
                   normalized_game_name, normalized_tag_line, profile_icon_id, is_main,
                   main_position, main_position_updated_at, last_synced_at, created_at,
                   updated_at, origin)
                VALUES (:puuid, :user, :platform, :region, :game_name, :tag_line,
                        :normalized_game_name, :normalized_tag_line, :icon, :main,
                        NULL, NULL, :synced, clock_timestamp(), clock_timestamp(), 'RIOT')
            """),
                {**values, "main": not has_account},
            )
        except IntegrityError as exc:
            raise account_constraint_error(exc) from None
        state = "CREATED"
    else:
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
            raise account_constraint_error(exc) from None
        if updated.rowcount != 1:
            raise ApiError.from_status(500)
        state = "EXISTING"
    counts = (
        (
            await c.execute(
                text("""
                SELECT count(*) AS account_count, count(*) FILTER (WHERE is_main) AS main_count
                FROM riot_accounts WHERE user_id = :user AND origin = 'RIOT'
            """),
                values,
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
            values,
        )
    row = (
        (
            await c.execute(
                text("""
                SELECT puuid, game_name, tag_line, platform, is_main FROM riot_accounts
                WHERE puuid = :puuid AND user_id = :user AND origin = 'RIOT'
            """),
                values,
            )
        )
        .mappings()
        .one_or_none()
    )
    if row is None:
        raise ApiError.from_status(500)
    return state, dict(row)


def account_constraint_error(exc: IntegrityError) -> Exception:
    original = exc.orig
    constraint = getattr(original, "constraint_name", None) or getattr(
        getattr(original, "__cause__", None), "constraint_name", None
    )
    if constraint == "riot_accounts_pkey":
        return ApiError(
            409,
            "riot_account_already_linked",
            "이 Riot 계정은 이미 다른 사용자에게 연결되어 있습니다.",
        )
    if constraint == "riot_accounts_riot_normalized_id_key":
        return ApiError(409, "riot_id_conflict", "동일한 Riot ID가 다른 계정에 연결되어 있습니다.")
    return exc
