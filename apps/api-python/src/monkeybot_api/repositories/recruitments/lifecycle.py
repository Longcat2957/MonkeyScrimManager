from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection


async def lock_actor_membership(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return await connection.scalar(
        text(
            "SELECT status FROM guild_members "
            "WHERE guild_id = :guild AND user_id = :actor_id FOR UPDATE"
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
        VALUES (:guild, :audit_id, 'USER', :actor_id, :audit_action, :target_type, :target,
                :request_id, CAST(:before AS jsonb), CAST(:snapshot AS jsonb), 'APPLIED', :now)
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
            VALUES (:event, :guild, :topic, :aggregate, :target, :revision, :event_type,
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
            VALUES (:notification_id, :guild, :dedup, :nonce, :channel, :notification_type,
                    CAST(:payload AS jsonb), :now, :now)
            ON CONFLICT (guild_id, deduplication_key) DO NOTHING
        """
        ),
        params,
    )


async def complete_lifecycle(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return await connection.scalar(
        text(
            """
        UPDATE idempotency_records SET state = 'COMPLETED', status_code = 200,
            response_body = CAST(:snapshot AS jsonb), completed_at = :now
        WHERE id = :idem_id AND guild_id = :guild AND scope_key = :guild AND state = 'IN_PROGRESS'
        RETURNING id
    """
        ),
        params,
    )


async def lock_guild(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return await connection.scalar(
        text("SELECT status FROM guilds WHERE guild_id = :guild FOR UPDATE"), params
    )


async def lock_operator_role(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return await connection.scalar(
        text("SELECT operator_role_id FROM guild_config WHERE guild_id = :guild FOR UPDATE"), params
    )


async def lock_recruitment(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return (
        (
            await connection.execute(
                text(
                    (
                        """
        SELECT status, revision, target_count, created_by, closed_at
        FROM recruitments WHERE guild_id = :guild AND id = :id FOR UPDATE
    """
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


async def joined_count(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return await connection.scalar(
        text(
            "SELECT count(*) FROM recruitment_participants "
            "WHERE guild_id = :guild AND recruitment_id = :id AND status = 'JOINED'"
        ),
        params,
    )


async def clear_assignments(connection: AsyncConnection, params: dict[str, Any]) -> None:
    await connection.execute(
        text(
            "DELETE FROM recruitment_entry_assignments "
            "WHERE guild_id = :guild AND recruitment_id = :id"
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


async def touch_recruitment(connection: AsyncConnection, params: dict[str, Any]) -> None:
    await connection.execute(
        text(
            "UPDATE recruitments SET updated_by = :actor_id, updated_at = :now "
            "WHERE guild_id = :guild AND id = :id"
        ),
        params,
    )


async def update_lifecycle(connection: AsyncConnection, params: dict[str, Any], action: str) -> Any:
    changes = {
        "Close": "status = 'CLOSED', closed_at = :now",
        "Cancel": "status = 'CANCELLED', cancelled_at = :now",
        "Restore": (
            "status = :restored, converted_at = NULL, cancelled_at = NULL, message_id = NULL"
        ),
        "Reopen": "status = 'OPEN', closed_at = NULL, cancelled_at = NULL",
    }[action]
    guard = " AND revision = :expected" if action in {"Cancel", "Restore"} else ""
    return await connection.scalar(
        text(
            f"""
        UPDATE recruitments SET {changes}, revision = revision + 1,
            updated_by = :actor_id, updated_at = :now
        WHERE guild_id = :guild AND id = :id {guard} RETURNING id
    """
        ),
        params,
    )


async def reopen_entry_draft(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return await connection.scalar(
        text(
            """
            UPDATE recruitment_entry_drafts SET revision = revision + 1,
                updated_by = :actor_id, updated_at = :now
            WHERE guild_id = :guild AND recruitment_id = :id RETURNING recruitment_id
        """
        ),
        params,
    )
