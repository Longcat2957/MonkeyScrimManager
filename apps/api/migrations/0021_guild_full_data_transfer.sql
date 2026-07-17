CREATE TABLE guild_full_data_imports (
    guild_id text NOT NULL,
    id uuid NOT NULL,
    actor_id text NOT NULL,
    status text NOT NULL,
    archive_digest bytea NOT NULL,
    archive_payload jsonb,
    source_guild_id text NOT NULL,
    target_state_digest bytea NOT NULL,
    summary jsonb NOT NULL,
    conflicts jsonb NOT NULL,
    conflict_count integer NOT NULL,
    request_id text NOT NULL,
    created_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL,
    applied_at timestamptz,
    CONSTRAINT guild_full_data_imports_pkey PRIMARY KEY (guild_id, id),
    CONSTRAINT guild_full_data_imports_guild_fk
        FOREIGN KEY (guild_id) REFERENCES guilds (guild_id) ON DELETE RESTRICT,
    CONSTRAINT guild_full_data_imports_actor_fk
        FOREIGN KEY (guild_id, actor_id)
        REFERENCES guild_members (guild_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT guild_full_data_imports_source_guild_id_check
        CHECK (public.is_discord_snowflake(source_guild_id)),
    CONSTRAINT guild_full_data_imports_archive_digest_size_check
        CHECK (octet_length(archive_digest) = 32),
    CONSTRAINT guild_full_data_imports_target_digest_size_check
        CHECK (octet_length(target_state_digest) = 32),
    CONSTRAINT guild_full_data_imports_conflicts_array_check
        CHECK (jsonb_typeof(conflicts) = 'array'),
    CONSTRAINT guild_full_data_imports_conflict_count_check
        CHECK (conflict_count >= 0 AND conflict_count = jsonb_array_length(conflicts)),
    CONSTRAINT guild_full_data_imports_request_id_check
        CHECK (char_length(request_id) BETWEEN 1 AND 128),
    CONSTRAINT guild_full_data_imports_status_check
        CHECK (status IN ('PREVIEWED', 'APPLIED')),
    CONSTRAINT guild_full_data_imports_lifecycle_check CHECK (
        (status = 'PREVIEWED' AND archive_payload IS NOT NULL AND applied_at IS NULL)
        OR (status = 'APPLIED' AND archive_payload IS NULL AND applied_at IS NOT NULL)
    ),
    CONSTRAINT guild_full_data_imports_expiry_check CHECK (expires_at > created_at)
);

CREATE INDEX guild_full_data_imports_actor_status_expiry_idx
    ON guild_full_data_imports (guild_id, actor_id, status, expires_at DESC);

CREATE INDEX guild_full_data_imports_expired_preview_idx
    ON guild_full_data_imports (expires_at)
    WHERE status = 'PREVIEWED';
