"""SQL for development-only dummy recruitment participant changes."""

import uuid
from datetime import datetime
from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError


async def authorize_operator(c: AsyncConnection, guild: str, actor: str) -> None:
    allowed = await c.scalar(
        text("""
        SELECT EXISTS (
            SELECT 1 FROM guild_members AS member
            JOIN guilds AS guild ON guild.guild_id=member.guild_id AND guild.status='ACTIVE'
            JOIN guild_config AS config ON config.guild_id=member.guild_id
            JOIN guild_member_discord_roles AS member_role
              ON member_role.guild_id=member.guild_id
             AND member_role.user_id=member.user_id
             AND member_role.role_id=config.operator_role_id
            WHERE member.guild_id=:guild AND member.user_id=:actor
              AND member.status='ACTIVE'
        )
    """),
        {"guild": guild, "actor": actor},
    )
    if not allowed:
        raise ApiError(403, "operator_required", "해당 길드의 운영자 권한이 필요합니다.")


async def overview(c: AsyncConnection, guild: str, recruitment: uuid.UUID) -> dict[str, Any]:
    summary = (
        (
            await c.execute(
                text("""
        SELECT recruitment.title, recruitment.status, recruitment.target_count,
               recruitment.revision,
               count(participant.user_id) FILTER (WHERE participant.status='JOINED')
                   AS joined_count
        FROM recruitments AS recruitment
        LEFT JOIN recruitment_participants AS participant
          ON participant.guild_id=recruitment.guild_id
         AND participant.recruitment_id=recruitment.id
        WHERE recruitment.guild_id=:guild AND recruitment.id=:recruitment
        GROUP BY recruitment.guild_id, recruitment.id
    """),
                {"guild": guild, "recruitment": recruitment},
            )
        )
        .mappings()
        .one_or_none()
    )
    if summary is None:
        raise ApiError(404, "recruitment_not_found", "모집을 찾을 수 없습니다.")
    rows = (
        (
            await c.execute(
                text("""
        SELECT profile.user_id, member.display_name, member.main_lane,
               profile.state AS profile_state, member.status AS membership_status,
               CASE participant.status
                 WHEN 'JOINED' THEN 'JOINED'
                 WHEN 'WITHDRAWN' THEN 'WITHDRAWN'
                 ELSE 'AVAILABLE'
               END AS participation_state
        FROM dummy_user_profiles AS profile
        JOIN guild_members AS member
          ON member.user_id=profile.user_id AND member.guild_id=:guild
        LEFT JOIN recruitment_participants AS participant
          ON participant.guild_id=member.guild_id
         AND participant.recruitment_id=:recruitment
         AND participant.user_id=member.user_id
        ORDER BY lower(member.display_name), member.user_id
    """),
                {"guild": guild, "recruitment": recruitment},
            )
        )
        .mappings()
        .all()
    )
    joined = int(summary["joined_count"])
    target = int(summary["target_count"])
    return {
        "recruitment": {
            "id": str(recruitment),
            "title": summary["title"],
            "status": summary["status"],
            "targetCount": target,
            "joinedCount": joined,
            "remainingSlots": max(target - joined, 0),
            "revision": summary["revision"],
        },
        "candidates": [
            {
                "userId": row["user_id"],
                "displayName": row["display_name"],
                "mainLane": row["main_lane"],
                "profileState": row["profile_state"],
                "membershipStatus": row["membership_status"],
                "participationState": row["participation_state"],
            }
            for row in rows
        ],
    }


async def lock_recruitment(
    c: AsyncConnection, guild: str, recruitment: uuid.UUID
) -> dict[str, Any]:
    row = (
        (
            await c.execute(
                text("""
        SELECT recruitment.channel_id, recruitment.message_id, recruitment.status,
               recruitment.target_count, recruitment.revision,
               (SELECT count(*) FROM recruitment_participants AS participant
                WHERE participant.guild_id=recruitment.guild_id
                  AND participant.recruitment_id=recruitment.id
                  AND participant.status='JOINED') AS joined_count
        FROM recruitments AS recruitment
        WHERE recruitment.guild_id=:guild AND recruitment.id=:recruitment FOR UPDATE
    """),
                {"guild": guild, "recruitment": recruitment},
            )
        )
        .mappings()
        .one_or_none()
    )
    if row is None:
        raise ApiError(404, "recruitment_not_found", "모집을 찾을 수 없습니다.")
    return dict(row)


async def selected_candidates(
    c: AsyncConnection, guild: str, recruitment: uuid.UUID, users: list[str]
) -> dict[str, dict[str, Any]]:
    rows = (
        (
            await c.execute(
                text("""
        SELECT profile.user_id, member.display_name, member.main_lane,
               profile.state AS profile_state, member.status AS membership_status,
               CASE participant.status
                 WHEN 'JOINED' THEN 'JOINED'
                 WHEN 'WITHDRAWN' THEN 'WITHDRAWN'
                 ELSE 'AVAILABLE'
               END AS participation_state
        FROM dummy_user_profiles AS profile
        JOIN guild_members AS member
          ON member.user_id=profile.user_id AND member.guild_id=:guild
        LEFT JOIN recruitment_participants AS participant
          ON participant.guild_id=member.guild_id
         AND participant.recruitment_id=:recruitment
         AND participant.user_id=member.user_id
        WHERE profile.user_id = ANY(CAST(:users AS text[]))
        ORDER BY profile.user_id FOR UPDATE OF profile, member
    """),
                {"guild": guild, "recruitment": recruitment, "users": users},
            )
        )
        .mappings()
        .all()
    )
    return {
        row["user_id"]: {
            "userId": row["user_id"],
            "displayName": row["display_name"],
            "mainLane": row["main_lane"],
            "profileState": row["profile_state"],
            "membershipStatus": row["membership_status"],
            "participationState": row["participation_state"],
        }
        for row in rows
    }


async def add_participant(
    c: AsyncConnection,
    guild: str,
    recruitment: uuid.UUID,
    candidate: dict[str, Any],
    now: datetime,
) -> None:
    params = {
        "guild": guild,
        "recruitment": recruitment,
        "user": candidate["userId"],
        "lane": candidate["mainLane"],
        "now": now,
    }
    await c.execute(
        text("""
        INSERT INTO recruitment_participants (
            guild_id, recruitment_id, user_id, status, joined_at, withdrawn_at
        ) VALUES (:guild, :recruitment, :user, 'JOINED', :now, NULL)
        ON CONFLICT (guild_id, recruitment_id, user_id) DO UPDATE
        SET status='JOINED', joined_at=EXCLUDED.joined_at, withdrawn_at=NULL
    """),
        params,
    )
    await c.execute(
        text("""
        DELETE FROM recruitment_preferred_lanes
        WHERE guild_id=:guild AND recruitment_id=:recruitment AND user_id=:user
    """),
        params,
    )
    if candidate["mainLane"] is not None:
        await c.execute(
            text("""
            INSERT INTO recruitment_preferred_lanes (
                guild_id, recruitment_id, user_id, priority, lane
            ) VALUES (:guild, :recruitment, :user, 1, :lane)
        """),
            params,
        )


async def remove_participant(
    c: AsyncConnection,
    guild: str,
    recruitment: uuid.UUID,
    user: str,
    now: datetime,
) -> None:
    params = {"guild": guild, "recruitment": recruitment, "user": user, "now": now}
    await c.execute(
        text("""
        DELETE FROM recruitment_entry_assignments
        WHERE guild_id=:guild AND recruitment_id=:recruitment AND user_id=:user
    """),
        params,
    )
    await c.execute(
        text("""
        DELETE FROM recruitment_preferred_lanes
        WHERE guild_id=:guild AND recruitment_id=:recruitment AND user_id=:user
    """),
        params,
    )
    updated = await c.execute(
        text("""
        UPDATE recruitment_participants
        SET status='WITHDRAWN', withdrawn_at=:now
        WHERE guild_id=:guild AND recruitment_id=:recruitment
          AND user_id=:user AND status='JOINED'
    """),
        params,
    )
    if updated.rowcount != 1:
        raise ApiError(
            409,
            "not_participant",
            "현재 모집 참가자가 아닌 더미 사용자입니다.",
            {"userId": user},
        )


async def bump_revisions(
    c: AsyncConnection,
    guild: str,
    recruitment: uuid.UUID,
    actor: str,
    now: datetime,
) -> None:
    params = {"guild": guild, "recruitment": recruitment, "actor": actor, "now": now}
    changed = await c.execute(
        text("""
        UPDATE recruitments SET revision=revision+1, updated_by=:actor, updated_at=:now
        WHERE guild_id=:guild AND id=:recruitment
    """),
        params,
    )
    draft = await c.execute(
        text("""
        UPDATE recruitment_entry_drafts
        SET revision=revision+1, updated_by=:actor, updated_at=:now
        WHERE guild_id=:guild AND recruitment_id=:recruitment
    """),
        params,
    )
    if changed.rowcount != 1 or draft.rowcount != 1:
        raise ApiError(
            500,
            "dummy_recruitment_invariant_failed",
            "더미 참가자 처리 결과의 일관성을 확인할 수 없습니다.",
        )
