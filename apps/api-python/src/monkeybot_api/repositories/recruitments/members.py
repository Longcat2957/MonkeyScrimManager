from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection


async def lock_guild(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return await connection.scalar(
        text("SELECT status FROM guilds WHERE guild_id = :guild FOR UPDATE"), params
    )


async def lock_operator_role(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return await connection.scalar(
        text("SELECT operator_role_id FROM guild_config WHERE guild_id = :guild FOR UPDATE"), params
    )


async def lock_actor_membership(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return await connection.scalar(
        text(
            "SELECT status FROM guild_members "
            "WHERE guild_id = :guild AND user_id = :actor_id FOR UPDATE"
        ),
        params,
    )


async def operator_role_exists(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return await connection.scalar(
        text(
            "SELECT true FROM guild_member_discord_roles "
            "WHERE guild_id = :guild AND user_id = :actor_id AND role_id = :role FOR SHARE"
        ),
        params,
    )


async def lock_recruitment(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return (
        (
            await connection.execute(
                text(
                    (
                        "SELECT status, target_count FROM recruitments "
                        "WHERE guild_id = :guild AND id = :id FOR UPDATE"
                    )
                ),
                params,
            )
        )
        .mappings()
        .one_or_none()
    )


async def lock_entry_revision(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return await connection.scalar(
        text(
            "SELECT revision FROM recruitment_entry_drafts "
            "WHERE guild_id = :guild AND recruitment_id = :id FOR UPDATE"
        ),
        params,
    )


async def lock_target_membership(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return await connection.scalar(
        text(
            "SELECT status FROM guild_members "
            "WHERE guild_id = :guild AND user_id = :member FOR UPDATE"
        ),
        params,
    )


async def lock_participation(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return await connection.scalar(
        text(
            "SELECT status FROM recruitment_participants WHERE guild_id = :guild "
            "AND recruitment_id = :id AND user_id = :member FOR UPDATE"
        ),
        params,
    )


async def clear_preferred_lanes(connection: AsyncConnection, params: dict[str, Any]) -> None:
    await connection.execute(
        text(
            "DELETE FROM recruitment_preferred_lanes WHERE guild_id = :guild "
            "AND recruitment_id = :id AND user_id = :member"
        ),
        params,
    )


async def insert_preferred_lane(connection: AsyncConnection, params: dict[str, Any]) -> None:
    await connection.execute(
        text(
            "INSERT INTO recruitment_preferred_lanes "
            "(guild_id, recruitment_id, user_id, priority, lane) "
            "VALUES (:guild, :id, :member, :priority, :lane)"
        ),
        params,
    )


async def joined_count(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return await connection.scalar(
        text(
            "SELECT count(*) FROM recruitment_participants "
            "WHERE guild_id = :guild AND recruitment_id = :id AND status = 'JOINED'"
        ),
        params,
    )


async def join_participant(connection: AsyncConnection, params: dict[str, Any]) -> None:
    await connection.execute(
        text(
            """
            INSERT INTO recruitment_participants
                (guild_id, recruitment_id, user_id, status, joined_at, withdrawn_at)
            VALUES (:guild, :id, :member, 'JOINED', :now, NULL)
            ON CONFLICT (guild_id, recruitment_id, user_id) DO UPDATE
            SET status = 'JOINED', joined_at = EXCLUDED.joined_at, withdrawn_at = NULL
        """
        ),
        params,
    )


async def withdraw_participant(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return await connection.scalar(
        text(
            """
            UPDATE recruitment_participants SET status = 'WITHDRAWN', withdrawn_at = :now
            WHERE guild_id = :guild AND recruitment_id = :id AND user_id = :member
              AND status = 'JOINED' RETURNING user_id
        """
        ),
        params,
    )


async def remove_assignment(connection: AsyncConnection, params: dict[str, Any]) -> None:
    await connection.execute(
        text(
            "DELETE FROM recruitment_entry_assignments "
            "WHERE guild_id = :guild AND recruitment_id = :id AND user_id = :member"
        ),
        params,
    )


async def increment_recruitment_revision(
    connection: AsyncConnection, params: dict[str, Any]
) -> None:
    await connection.execute(
        text(
            "UPDATE recruitments SET revision = revision + 1, "
            "updated_by = :actor_id, updated_at = :now WHERE guild_id = :guild AND id = :id"
        ),
        params,
    )


async def increment_entry_revision(connection: AsyncConnection, params: dict[str, Any]) -> None:
    await connection.execute(
        text(
            "UPDATE recruitment_entry_drafts SET revision = revision + 1, "
            "updated_by = :actor_id, updated_at = :now "
            "WHERE guild_id = :guild AND recruitment_id = :id"
        ),
        params,
    )


async def insert_audit(connection: AsyncConnection, params: dict[str, Any]) -> None:
    await connection.execute(
        text(
            """
        INSERT INTO admin_audit_log
            (guild_id, id, principal_type, principal_id, action, target_type, target_id,
             request_id, before_summary, after_summary, result, created_at)
        VALUES (:guild, :audit_id, 'USER', :actor_id, :action, 'RECRUITMENT', :target, :request_id,
                CAST(:before AS jsonb), CAST(:snapshot AS jsonb), 'APPLIED', :now)
    """
        ),
        params,
    )


async def insert_event(connection: AsyncConnection, params: dict[str, Any]) -> None:
    await connection.execute(
        text(
            """
        INSERT INTO outbox_events (id, guild_id, topic, aggregate_type, aggregate_id,
            aggregate_revision, event_type, payload, occurred_at, available_at)
        VALUES (:event_id, :guild, :topic, 'RECRUITMENT', :target, :revision, :event_type,
                CAST(:payload AS jsonb), :now, :now)
    """
        ),
        params,
    )


async def enqueue_message(connection: AsyncConnection, params: dict[str, Any]) -> None:
    await connection.execute(
        text(
            """
            INSERT INTO bot_notification_outbox (id, guild_id, deduplication_key, discord_nonce,
                channel_id, notification_type, payload, created_at, available_at)
            VALUES (:notification_id, :guild, :dedup, :nonce, :channel,
                    'RECRUITMENT_MESSAGE_UPDATE',
                    CAST(:payload AS jsonb), :now, :now)
            ON CONFLICT (guild_id, deduplication_key) DO NOTHING
        """
        ),
        params,
    )


async def complete_membership(connection: AsyncConnection, params: dict[str, Any]) -> None:
    await connection.execute(
        text(
            """
        UPDATE idempotency_records SET state = 'COMPLETED', status_code = 200,
            response_body = CAST(:snapshot AS jsonb), completed_at = :now
        WHERE id = :idem_id AND guild_id = :guild AND scope_key = :guild AND state = 'IN_PROGRESS'
    """
        ),
        params,
    )
