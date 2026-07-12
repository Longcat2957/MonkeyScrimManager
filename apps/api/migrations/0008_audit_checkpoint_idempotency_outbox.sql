CREATE TABLE revert_checkpoints (
    guild_id text NOT NULL,
    id uuid NOT NULL,
    target_type text NOT NULL,
    target_id text NOT NULL,
    target_revision bigint NOT NULL,
    action text NOT NULL,
    schema_version integer NOT NULL,
    restore_payload jsonb NOT NULL,
    status text NOT NULL,
    created_by_type text NOT NULL,
    created_by text NOT NULL,
    created_at timestamptz NOT NULL,
    reverted_by_type text,
    reverted_by text,
    reverted_at timestamptz,
    CONSTRAINT revert_checkpoints_pkey PRIMARY KEY (guild_id, id),
    CONSTRAINT revert_checkpoints_guild_fk
        FOREIGN KEY (guild_id) REFERENCES guilds (guild_id) ON DELETE RESTRICT,
    CONSTRAINT revert_checkpoints_target_revision_check CHECK (target_revision >= 0),
    CONSTRAINT revert_checkpoints_schema_version_check CHECK (schema_version > 0),
    CONSTRAINT revert_checkpoints_status_check
        CHECK (status IN ('ACTIVE', 'REVERTED', 'EXPIRED')),
    CONSTRAINT revert_checkpoints_created_by_type_check
        CHECK (created_by_type IN ('USER', 'BOT', 'SYSTEM')),
    CONSTRAINT revert_checkpoints_reverted_by_type_check
        CHECK (reverted_by_type IN ('USER', 'BOT', 'SYSTEM')),
    CONSTRAINT revert_checkpoints_lifecycle_check CHECK (
        (status = 'REVERTED'
            AND reverted_by_type IS NOT NULL
            AND reverted_by IS NOT NULL
            AND reverted_at IS NOT NULL)
        OR (status IN ('ACTIVE', 'EXPIRED')
            AND reverted_by_type IS NULL
            AND reverted_by IS NULL
            AND reverted_at IS NULL)
    )
);

CREATE TABLE admin_audit_log (
    guild_id text NOT NULL,
    id uuid NOT NULL,
    principal_type text NOT NULL,
    principal_id text NOT NULL,
    action text NOT NULL,
    target_type text,
    target_id text,
    checkpoint_id uuid,
    request_id text,
    before_summary jsonb,
    after_summary jsonb,
    result text NOT NULL,
    note text,
    created_at timestamptz NOT NULL,
    CONSTRAINT admin_audit_log_pkey PRIMARY KEY (guild_id, id),
    CONSTRAINT admin_audit_log_checkpoint_fk
        FOREIGN KEY (guild_id, checkpoint_id)
        REFERENCES revert_checkpoints (guild_id, id) ON DELETE RESTRICT,
    CONSTRAINT admin_audit_log_guild_fk
        FOREIGN KEY (guild_id) REFERENCES guilds (guild_id) ON DELETE RESTRICT,
    CONSTRAINT admin_audit_log_principal_type_check
        CHECK (principal_type IN ('USER', 'BOT', 'SYSTEM', 'MIGRATION')),
    CONSTRAINT admin_audit_log_result_check CHECK (result IN ('APPLIED', 'REVERTED'))
);

CREATE TABLE idempotency_records (
    id uuid PRIMARY KEY,
    guild_id text,
    scope_key text NOT NULL,
    principal_type text NOT NULL,
    principal_id text NOT NULL,
    method text NOT NULL,
    route_template text NOT NULL,
    idempotency_key text NOT NULL,
    request_hash bytea NOT NULL,
    state text NOT NULL,
    status_code smallint,
    response_body jsonb,
    created_at timestamptz NOT NULL,
    completed_at timestamptz,
    expires_at timestamptz NOT NULL,
    CONSTRAINT idempotency_records_guild_fk
        FOREIGN KEY (guild_id) REFERENCES guilds (guild_id) ON DELETE RESTRICT,
    CONSTRAINT idempotency_records_command_key UNIQUE (
        scope_key,
        principal_type,
        principal_id,
        method,
        route_template,
        idempotency_key
    ),
    CONSTRAINT idempotency_records_principal_type_check
        CHECK (principal_type IN ('USER', 'BOT', 'SERVICE')),
    CONSTRAINT idempotency_records_state_check
        CHECK (state IN ('IN_PROGRESS', 'COMPLETED')),
    CONSTRAINT idempotency_records_status_code_check
        CHECK (status_code BETWEEN 100 AND 599),
    CONSTRAINT idempotency_records_scope_check
        CHECK (scope_key = COALESCE(guild_id, 'GLOBAL')),
    CONSTRAINT idempotency_records_lifecycle_check CHECK (
        (state = 'IN_PROGRESS'
            AND status_code IS NULL
            AND response_body IS NULL
            AND completed_at IS NULL)
        OR (state = 'COMPLETED'
            AND status_code IS NOT NULL
            AND completed_at IS NOT NULL)
    )
);

CREATE TABLE outbox_events (
    id uuid PRIMARY KEY,
    guild_id text NOT NULL,
    topic text NOT NULL,
    aggregate_type text NOT NULL,
    aggregate_id text NOT NULL,
    aggregate_revision bigint NOT NULL,
    event_type text NOT NULL,
    payload jsonb NOT NULL,
    occurred_at timestamptz NOT NULL,
    available_at timestamptz NOT NULL,
    attempts integer NOT NULL DEFAULT 0,
    lease_owner text,
    lease_token uuid,
    lease_until timestamptz,
    published_at timestamptz,
    last_error text,
    CONSTRAINT outbox_events_guild_fk
        FOREIGN KEY (guild_id) REFERENCES guilds (guild_id) ON DELETE RESTRICT,
    CONSTRAINT outbox_events_aggregate_revision_check CHECK (aggregate_revision >= 0),
    CONSTRAINT outbox_events_attempts_check CHECK (attempts >= 0),
    CONSTRAINT outbox_events_lease_check
        CHECK (num_nonnulls(lease_owner, lease_token, lease_until) IN (0, 3)),
    CONSTRAINT outbox_events_published_lease_check
        CHECK (published_at IS NULL OR lease_owner IS NULL)
);

CREATE TABLE bot_notification_outbox (
    id uuid PRIMARY KEY,
    guild_id text NOT NULL,
    deduplication_key text NOT NULL,
    discord_nonce text NOT NULL,
    channel_id text NOT NULL,
    notification_type text NOT NULL,
    payload jsonb NOT NULL,
    created_at timestamptz NOT NULL,
    available_at timestamptz NOT NULL,
    attempts integer NOT NULL DEFAULT 0,
    first_attempt_at timestamptz,
    lease_owner text,
    lease_token uuid,
    lease_until timestamptz,
    discord_message_id text,
    delivered_at timestamptz,
    failed_at timestamptz,
    failure_code text,
    last_error text,
    CONSTRAINT bot_notification_outbox_dedup_key
        UNIQUE (guild_id, deduplication_key),
    CONSTRAINT bot_notification_outbox_nonce_key UNIQUE (discord_nonce),
    CONSTRAINT bot_notification_outbox_guild_fk
        FOREIGN KEY (guild_id) REFERENCES guilds (guild_id) ON DELETE RESTRICT,
    CONSTRAINT bot_notification_outbox_channel_id_check
        CHECK (public.is_discord_snowflake(channel_id)),
    CONSTRAINT bot_notification_outbox_discord_message_id_check
        CHECK (public.is_discord_snowflake(discord_message_id)),
    CONSTRAINT bot_notification_outbox_nonce_check
        CHECK (char_length(discord_nonce) BETWEEN 1 AND 25),
    CONSTRAINT bot_notification_outbox_attempts_check CHECK (attempts >= 0),
    CONSTRAINT bot_notification_outbox_lease_check
        CHECK (num_nonnulls(lease_owner, lease_token, lease_until) IN (0, 3)),
    CONSTRAINT bot_notification_outbox_first_attempt_check CHECK (
        (attempts = 0 AND first_attempt_at IS NULL)
        OR (attempts > 0 AND first_attempt_at IS NOT NULL)
    ),
    CONSTRAINT bot_notification_outbox_terminal_check
        CHECK (NOT (delivered_at IS NOT NULL AND failed_at IS NOT NULL)),
    CONSTRAINT bot_notification_outbox_failure_code_check
        CHECK ((failed_at IS NULL) = (failure_code IS NULL)),
    CONSTRAINT bot_notification_outbox_delivery_check
        CHECK (delivered_at IS NULL OR discord_message_id IS NOT NULL),
    CONSTRAINT bot_notification_outbox_failure_check CHECK (
        failed_at IS NULL
        OR (discord_message_id IS NULL AND last_error IS NOT NULL)
    ),
    CONSTRAINT bot_notification_outbox_terminal_lease_check
        CHECK ((delivered_at IS NULL AND failed_at IS NULL) OR lease_owner IS NULL)
);

CREATE TABLE bot_instances (
    instance_id text PRIMARY KEY,
    version text NOT NULL,
    commit_sha text,
    started_at timestamptz NOT NULL,
    last_heartbeat_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL
);
