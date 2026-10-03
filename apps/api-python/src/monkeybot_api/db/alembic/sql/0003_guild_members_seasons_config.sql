CREATE TABLE guilds (
    guild_id text PRIMARY KEY,
    discord_name text NOT NULL,
    icon_hash text,
    status text NOT NULL,
    installed_by text,
    installed_at timestamptz NOT NULL,
    activated_at timestamptz,
    left_at timestamptz,
    updated_at timestamptz NOT NULL,
    CONSTRAINT guilds_guild_id_check
        CHECK (public.is_discord_snowflake(guild_id)),
    CONSTRAINT guilds_status_check
        CHECK (status IN ('INSTALLED', 'SETUP_REQUIRED', 'ACTIVE', 'DISABLED', 'LEFT')),
    CONSTRAINT guilds_installed_by_fk
        FOREIGN KEY (installed_by) REFERENCES users (discord_id) ON DELETE RESTRICT
);

CREATE TABLE guild_members (
    guild_id text NOT NULL,
    user_id text NOT NULL,
    display_name text NOT NULL,
    status text NOT NULL,
    main_lane text,
    first_seen_at timestamptz NOT NULL,
    registered_at timestamptz,
    discord_joined_at timestamptz,
    last_active_at timestamptz,
    last_synced_at timestamptz,
    inactivated_at timestamptz,
    CONSTRAINT guild_members_pkey PRIMARY KEY (guild_id, user_id),
    CONSTRAINT guild_members_guild_fk
        FOREIGN KEY (guild_id) REFERENCES guilds (guild_id) ON DELETE RESTRICT,
    CONSTRAINT guild_members_user_fk
        FOREIGN KEY (user_id) REFERENCES users (discord_id) ON DELETE RESTRICT,
    CONSTRAINT guild_members_status_check
        CHECK (status IN ('ACTIVE', 'INACTIVE')),
    CONSTRAINT guild_members_main_lane_check
        CHECK (main_lane IN ('TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT'))
);

CREATE TABLE guild_member_discord_roles (
    guild_id text NOT NULL,
    user_id text NOT NULL,
    role_id text NOT NULL,
    last_synced_at timestamptz NOT NULL,
    CONSTRAINT guild_member_discord_roles_pkey PRIMARY KEY (guild_id, user_id, role_id),
    CONSTRAINT guild_member_discord_roles_member_fk
        FOREIGN KEY (guild_id, user_id)
        REFERENCES guild_members (guild_id, user_id) ON DELETE CASCADE,
    CONSTRAINT guild_member_discord_roles_role_id_check
        CHECK (public.is_discord_snowflake(role_id))
);

CREATE TABLE guild_config (
    guild_id text PRIMARY KEY,
    display_name text NOT NULL,
    operator_role_id text NOT NULL,
    recruitment_channel_id text,
    log_channel_id text,
    is_externally_public boolean NOT NULL,
    revision bigint NOT NULL,
    updated_by text,
    updated_at timestamptz NOT NULL,
    CONSTRAINT guild_config_guild_fk
        FOREIGN KEY (guild_id) REFERENCES guilds (guild_id) ON DELETE RESTRICT,
    CONSTRAINT guild_config_updated_by_fk
        FOREIGN KEY (guild_id, updated_by)
        REFERENCES guild_members (guild_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT guild_config_operator_role_id_check
        CHECK (public.is_discord_snowflake(operator_role_id)),
    CONSTRAINT guild_config_recruitment_channel_id_check
        CHECK (public.is_discord_snowflake(recruitment_channel_id)),
    CONSTRAINT guild_config_log_channel_id_check
        CHECK (public.is_discord_snowflake(log_channel_id)),
    CONSTRAINT guild_config_revision_check CHECK (revision >= 0)
);

CREATE TABLE guild_admin_state (
    guild_id text NOT NULL,
    scope text NOT NULL,
    revision bigint NOT NULL,
    updated_by text,
    updated_at timestamptz NOT NULL,
    CONSTRAINT guild_admin_state_pkey PRIMARY KEY (guild_id, scope),
    CONSTRAINT guild_admin_state_guild_fk
        FOREIGN KEY (guild_id) REFERENCES guilds (guild_id) ON DELETE RESTRICT,
    CONSTRAINT guild_admin_state_updated_by_fk
        FOREIGN KEY (guild_id, updated_by)
        REFERENCES guild_members (guild_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT guild_admin_state_scope_check
        CHECK (scope IN ('MEMBERS', 'OPERATIONS')),
    CONSTRAINT guild_admin_state_revision_check CHECK (revision >= 0)
);

CREATE TABLE seasons (
    guild_id text NOT NULL,
    id uuid NOT NULL,
    name text NOT NULL,
    status text NOT NULL,
    started_at timestamptz NOT NULL,
    ended_at timestamptz,
    created_by text,
    created_at timestamptz NOT NULL,
    CONSTRAINT seasons_pkey PRIMARY KEY (guild_id, id),
    CONSTRAINT seasons_guild_fk
        FOREIGN KEY (guild_id) REFERENCES guilds (guild_id) ON DELETE RESTRICT,
    CONSTRAINT seasons_created_by_fk
        FOREIGN KEY (guild_id, created_by)
        REFERENCES guild_members (guild_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT seasons_status_check CHECK (status IN ('ACTIVE', 'CLOSED')),
    CONSTRAINT seasons_status_ended_at_check CHECK (
        (status = 'ACTIVE' AND ended_at IS NULL)
        OR (status = 'CLOSED' AND ended_at IS NOT NULL)
    )
);

CREATE UNIQUE INDEX seasons_one_active_per_guild_idx
    ON seasons (guild_id)
    WHERE status = 'ACTIVE';

CREATE TABLE guild_counters (
    guild_id text NOT NULL,
    counter_key text NOT NULL,
    next_value bigint NOT NULL,
    CONSTRAINT guild_counters_pkey PRIMARY KEY (guild_id, counter_key),
    CONSTRAINT guild_counters_guild_fk
        FOREIGN KEY (guild_id) REFERENCES guilds (guild_id) ON DELETE RESTRICT,
    CONSTRAINT guild_counters_key_check CHECK (
        counter_key IN ('RECRUITMENT', 'SERIES', 'AUCTION_RECRUITMENT', 'AUCTION_TOURNAMENT')
    ),
    CONSTRAINT guild_counters_next_value_check CHECK (next_value > 0)
);

CREATE TABLE user_guild_preferences (
    user_id text PRIMARY KEY,
    default_guild_id text,
    updated_at timestamptz NOT NULL,
    CONSTRAINT user_guild_preferences_user_fk
        FOREIGN KEY (user_id) REFERENCES users (discord_id) ON DELETE CASCADE,
    CONSTRAINT user_guild_preferences_membership_fk
        FOREIGN KEY (default_guild_id, user_id)
        REFERENCES guild_members (guild_id, user_id) ON DELETE RESTRICT
);
