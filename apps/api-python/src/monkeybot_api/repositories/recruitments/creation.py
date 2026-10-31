from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection


async def lock_guild(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return await connection.scalar(
        text("SELECT status FROM guilds WHERE guild_id = :guild FOR UPDATE"), params
    )


async def lock_config(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return (
        (
            await connection.execute(
                text(
                    "SELECT operator_role_id, recruitment_channel_id "
                    "FROM guild_config WHERE guild_id = :guild FOR UPDATE"
                ),
                params,
            )
        )
        .mappings()
        .one_or_none()
    )


async def lock_actor_membership(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return await connection.scalar(
        text(
            "SELECT status FROM guild_members "
            "WHERE guild_id = :guild AND user_id = :actor_id FOR UPDATE"
        ),
        params,
    )


async def active_season(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return await connection.scalar(
        text("SELECT id FROM seasons WHERE guild_id = :guild AND status = 'ACTIVE'"), params
    )


async def next_display_number(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return await connection.scalar(
        text(
            "UPDATE guild_counters SET next_value = next_value + 1 "
            "WHERE guild_id = :guild AND counter_key = 'RECRUITMENT' "
            "RETURNING next_value - 1"
        ),
        params,
    )


async def insert_recruitment(connection: AsyncConnection, params: dict[str, Any]) -> None:
    await connection.execute(
        text(
            """
        INSERT INTO recruitments
            (guild_id, id, season_id, display_number, title, target_count, best_of, hard_fearless,
             status, revision, channel_id, message_id, created_by, created_at,
             updated_by, updated_at)
        VALUES (:guild, :id, :season, :number, :title, 10, :best_of, :fearless,
                'OPEN', 0, NULL, NULL, :actor_id, :now, :actor_id, :now)
    """
        ),
        params,
    )


async def insert_entry_draft(connection: AsyncConnection, params: dict[str, Any]) -> None:
    await connection.execute(
        text(
            """
        INSERT INTO recruitment_entry_drafts
            (guild_id, recruitment_id, revision, updated_by, updated_at)
        VALUES (:guild, :id, 0, :actor_id, :now)
    """
        ),
        params,
    )


async def enqueue_initial_message(connection: AsyncConnection, params: dict[str, Any]) -> None:
    await connection.execute(
        text(
            """
            INSERT INTO bot_notification_outbox
              (id, guild_id, deduplication_key, discord_nonce, channel_id, notification_type,
               payload, created_at, available_at)
            VALUES (:notification_id, :guild, :dedup, :nonce, :channel,
                    'RECRUITMENT_MESSAGE_CREATE',
                    CAST(:payload AS jsonb), :now, :now)
            ON CONFLICT (guild_id, deduplication_key) DO NOTHING
        """
        ),
        params,
    )


async def insert_audit(connection: AsyncConnection, params: dict[str, Any]) -> None:
    await connection.execute(
        text(
            """
        INSERT INTO admin_audit_log
          (guild_id, id, principal_type, principal_id, action, target_type, target_id,
           checkpoint_id, request_id, before_summary, after_summary, result, note, created_at)
        VALUES (:guild, :audit_id, 'USER', :actor_id, 'RECRUITMENT_CREATE', 'RECRUITMENT', :target,
                NULL, :request_id, NULL, CAST(:snapshot AS jsonb), 'APPLIED', NULL, :now)
    """
        ),
        params,
    )


async def insert_event(connection: AsyncConnection, params: dict[str, Any]) -> None:
    await connection.execute(
        text(
            """
        INSERT INTO outbox_events (id, guild_id, topic, aggregate_type, aggregate_id,
                                   aggregate_revision, event_type, payload,
                                   occurred_at, available_at)
        VALUES (:event_id, :guild, :topic, 'RECRUITMENT', :target, 0, 'RECRUITMENT_CREATED',
                CAST(:event AS jsonb), :now, :now)
    """
        ),
        params,
    )


async def complete_creation(connection: AsyncConnection, params: dict[str, Any]) -> Any:
    return await connection.scalar(
        text(
            """
        UPDATE idempotency_records
        SET state = 'COMPLETED', status_code = 201, response_body = CAST(:snapshot AS jsonb),
            completed_at = :now
        WHERE id = :idem_id AND scope_key = :guild AND guild_id = :guild AND state = 'IN_PROGRESS'
        RETURNING id
    """
        ),
        params,
    )
