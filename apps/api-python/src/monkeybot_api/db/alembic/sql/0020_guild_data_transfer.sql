ALTER TABLE mmr_ledger_entries
    DROP CONSTRAINT mmr_ledger_entries_source_type_check;

ALTER TABLE mmr_ledger_entries
    ADD CONSTRAINT mmr_ledger_entries_source_type_check CHECK (
        source_type IN (
            'GAME_RESULT',
            'GAME_REVERSAL',
            'MANUAL_ADJUSTMENT',
            'MANUAL_REVERSAL',
            'MIGRATION_SEED',
            'DATA_IMPORT_SEED'
        )
    );

ALTER TABLE mmr_ledger_entries
    DROP CONSTRAINT mmr_ledger_entries_count_delta_check;

ALTER TABLE mmr_ledger_entries
    ADD CONSTRAINT mmr_ledger_entries_count_delta_check CHECK (
        (source_type = 'GAME_RESULT'
            AND opponent_user_id IS NOT NULL
            AND games_played_delta = 1
            AND wins_delta IN (0, 1))
        OR (source_type = 'GAME_REVERSAL'
            AND opponent_user_id IS NOT NULL
            AND games_played_delta = -1
            AND wins_delta IN (-1, 0))
        OR (source_type IN ('MANUAL_ADJUSTMENT', 'MANUAL_REVERSAL')
            AND opponent_user_id IS NULL
            AND games_played_delta = 0
            AND wins_delta = 0)
        OR (source_type = 'MIGRATION_SEED' AND opponent_user_id IS NULL)
        OR (source_type = 'DATA_IMPORT_SEED'
            AND opponent_user_id IS NULL
            AND games_played_delta = 0
            AND wins_delta = 0)
    );

CREATE TABLE guild_data_imports (
    guild_id text NOT NULL,
    id uuid NOT NULL,
    actor_id text NOT NULL,
    status text NOT NULL,
    archive_digest bytea NOT NULL,
    archive_payload jsonb,
    source_guild_id text NOT NULL,
    source_season_id uuid NOT NULL,
    target_season_id uuid NOT NULL,
    target_members_revision bigint NOT NULL,
    target_mmr_digest bytea NOT NULL,
    summary jsonb NOT NULL,
    conflict_count integer NOT NULL,
    request_id text NOT NULL,
    created_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL,
    applied_at timestamptz,
    CONSTRAINT guild_data_imports_pkey PRIMARY KEY (guild_id, id),
    CONSTRAINT guild_data_imports_guild_fk
        FOREIGN KEY (guild_id) REFERENCES guilds (guild_id) ON DELETE RESTRICT,
    CONSTRAINT guild_data_imports_actor_fk
        FOREIGN KEY (guild_id, actor_id)
        REFERENCES guild_members (guild_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT guild_data_imports_target_season_fk
        FOREIGN KEY (guild_id, target_season_id)
        REFERENCES seasons (guild_id, id) ON DELETE RESTRICT,
    CONSTRAINT guild_data_imports_source_guild_id_check
        CHECK (public.is_discord_snowflake(source_guild_id)),
    CONSTRAINT guild_data_imports_digest_size_check
        CHECK (octet_length(archive_digest) = 32),
    CONSTRAINT guild_data_imports_target_revision_check
        CHECK (target_members_revision >= 0),
    CONSTRAINT guild_data_imports_target_mmr_digest_size_check
        CHECK (octet_length(target_mmr_digest) = 32),
    CONSTRAINT guild_data_imports_conflict_count_check
        CHECK (conflict_count >= 0),
    CONSTRAINT guild_data_imports_request_id_check
        CHECK (char_length(request_id) BETWEEN 1 AND 128),
    CONSTRAINT guild_data_imports_status_check
        CHECK (status IN ('PREVIEWED', 'APPLIED')),
    CONSTRAINT guild_data_imports_lifecycle_check CHECK (
        (status = 'PREVIEWED' AND archive_payload IS NOT NULL AND applied_at IS NULL)
        OR (status = 'APPLIED' AND archive_payload IS NULL AND applied_at IS NOT NULL)
    ),
    CONSTRAINT guild_data_imports_expiry_check CHECK (expires_at > created_at)
);

CREATE INDEX guild_data_imports_actor_status_expiry_idx
    ON guild_data_imports (guild_id, actor_id, status, expires_at DESC);

CREATE INDEX guild_data_imports_target_season_idx
    ON guild_data_imports (guild_id, target_season_id);

CREATE INDEX guild_data_imports_expired_preview_idx
    ON guild_data_imports (expires_at)
    WHERE status = 'PREVIEWED';
