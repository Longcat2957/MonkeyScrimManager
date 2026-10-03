CREATE TABLE dummy_user_profiles (
    user_id text PRIMARY KEY,
    stable_key text NOT NULL,
    state text NOT NULL,
    avatar_seed text,
    notes text,
    revision bigint NOT NULL,
    created_by text NOT NULL,
    updated_by text NOT NULL,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL,
    CONSTRAINT dummy_user_profiles_user_fk
        FOREIGN KEY (user_id) REFERENCES users (user_id) ON DELETE RESTRICT,
    CONSTRAINT dummy_user_profiles_created_by_fk
        FOREIGN KEY (created_by) REFERENCES users (user_id) ON DELETE RESTRICT,
    CONSTRAINT dummy_user_profiles_updated_by_fk
        FOREIGN KEY (updated_by) REFERENCES users (user_id) ON DELETE RESTRICT,
    CONSTRAINT dummy_user_profiles_stable_key_key UNIQUE (stable_key),
    CONSTRAINT dummy_user_profiles_user_id_check
        CHECK (user_id ~ '^dmy_[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'),
    CONSTRAINT dummy_user_profiles_stable_key_check
        CHECK (stable_key ~ '^[a-z0-9][a-z0-9_-]{0,63}$'),
    CONSTRAINT dummy_user_profiles_state_check
        CHECK (state IN ('ACTIVE', 'ARCHIVED')),
    CONSTRAINT dummy_user_profiles_avatar_seed_check
        CHECK (avatar_seed IS NULL OR (
            char_length(avatar_seed) BETWEEN 1 AND 128
            AND avatar_seed = btrim(avatar_seed)
        )),
    CONSTRAINT dummy_user_profiles_notes_check
        CHECK (notes IS NULL OR char_length(notes) <= 1000),
    CONSTRAINT dummy_user_profiles_revision_check CHECK (revision >= 0)
);

ALTER TABLE riot_accounts
    ADD COLUMN origin text NOT NULL DEFAULT 'RIOT',
    ADD CONSTRAINT riot_accounts_origin_check CHECK (origin IN ('RIOT', 'DUMMY'));

ALTER TABLE riot_accounts
    DROP CONSTRAINT riot_accounts_normalized_id_key;

CREATE UNIQUE INDEX riot_accounts_riot_normalized_id_key
    ON riot_accounts (routing_region, normalized_game_name, normalized_tag_line)
    WHERE origin = 'RIOT';

CREATE UNIQUE INDEX riot_accounts_one_dummy_per_user_idx
    ON riot_accounts (user_id)
    WHERE origin = 'DUMMY';

ALTER TABLE riot_accounts
    ADD CONSTRAINT riot_accounts_dummy_owner_check CHECK (
        origin = 'RIOT' OR (puuid LIKE 'dummy:%' AND last_synced_at IS NULL)
    );

CREATE TABLE dummy_user_groups (
    id uuid PRIMARY KEY,
    name text NOT NULL,
    description text,
    revision bigint NOT NULL,
    created_by text NOT NULL,
    updated_by text NOT NULL,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL,
    CONSTRAINT dummy_user_groups_created_by_fk
        FOREIGN KEY (created_by) REFERENCES users (user_id) ON DELETE RESTRICT,
    CONSTRAINT dummy_user_groups_updated_by_fk
        FOREIGN KEY (updated_by) REFERENCES users (user_id) ON DELETE RESTRICT,
    CONSTRAINT dummy_user_groups_name_check
        CHECK (char_length(name) BETWEEN 1 AND 100 AND name = btrim(name)),
    CONSTRAINT dummy_user_groups_description_check
        CHECK (description IS NULL OR char_length(description) <= 500),
    CONSTRAINT dummy_user_groups_revision_check CHECK (revision >= 0)
);

CREATE UNIQUE INDEX dummy_user_groups_name_ci_key
    ON dummy_user_groups (lower(name));

CREATE TABLE dummy_user_group_members (
    group_id uuid NOT NULL,
    user_id text NOT NULL,
    position smallint NOT NULL,
    added_at timestamptz NOT NULL,
    CONSTRAINT dummy_user_group_members_pkey PRIMARY KEY (group_id, user_id),
    CONSTRAINT dummy_user_group_members_position_key UNIQUE (group_id, position),
    CONSTRAINT dummy_user_group_members_group_fk
        FOREIGN KEY (group_id) REFERENCES dummy_user_groups (id) ON DELETE CASCADE,
    CONSTRAINT dummy_user_group_members_user_fk
        FOREIGN KEY (user_id) REFERENCES dummy_user_profiles (user_id) ON DELETE RESTRICT,
    CONSTRAINT dummy_user_group_members_position_check CHECK (position BETWEEN 1 AND 100)
);

CREATE TABLE dummy_user_audit_log (
    id uuid PRIMARY KEY,
    actor_user_id text NOT NULL,
    action text NOT NULL,
    target_type text NOT NULL,
    target_id text NOT NULL,
    request_id text NOT NULL,
    before_summary jsonb,
    after_summary jsonb,
    created_at timestamptz NOT NULL,
    CONSTRAINT dummy_user_audit_log_actor_fk
        FOREIGN KEY (actor_user_id) REFERENCES users (user_id) ON DELETE RESTRICT,
    CONSTRAINT dummy_user_audit_log_action_check CHECK (char_length(action) BETWEEN 1 AND 100),
    CONSTRAINT dummy_user_audit_log_target_type_check
        CHECK (target_type IN ('DUMMY_USER', 'DUMMY_USER_GROUP', 'GUILD_MEMBER')),
    CONSTRAINT dummy_user_audit_log_target_id_check CHECK (char_length(target_id) BETWEEN 1 AND 256),
    CONSTRAINT dummy_user_audit_log_request_id_check CHECK (char_length(request_id) BETWEEN 1 AND 128)
);

CREATE INDEX dummy_user_profiles_state_updated_idx
    ON dummy_user_profiles (state, updated_at DESC, user_id);

CREATE INDEX dummy_user_group_members_user_idx
    ON dummy_user_group_members (user_id, group_id);

CREATE INDEX dummy_user_audit_log_target_created_idx
    ON dummy_user_audit_log (target_type, target_id, created_at DESC);
