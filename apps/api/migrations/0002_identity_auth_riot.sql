CREATE TABLE users (
    discord_id text PRIMARY KEY,
    display_name text NOT NULL,
    avatar_hash text,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL,
    deleted_at timestamptz,
    CONSTRAINT users_discord_id_check
        CHECK (public.is_discord_snowflake(discord_id))
);

CREATE TABLE oauth_login_attempts (
    id uuid PRIMARY KEY,
    state_hash bytea NOT NULL,
    pkce_verifier_ciphertext bytea NOT NULL,
    redirect_path text NOT NULL,
    created_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL,
    consumed_at timestamptz,
    CONSTRAINT oauth_login_attempts_state_hash_key UNIQUE (state_hash)
);

CREATE TABLE user_sessions (
    id uuid PRIMARY KEY,
    user_id text NOT NULL,
    token_hash bytea NOT NULL,
    csrf_secret_hash bytea NOT NULL,
    rotated_from_id uuid,
    created_at timestamptz NOT NULL,
    last_seen_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL,
    revoked_at timestamptz,
    CONSTRAINT user_sessions_token_hash_key UNIQUE (token_hash),
    CONSTRAINT user_sessions_user_fk
        FOREIGN KEY (user_id) REFERENCES users (discord_id) ON DELETE RESTRICT,
    CONSTRAINT user_sessions_rotated_from_fk
        FOREIGN KEY (rotated_from_id) REFERENCES user_sessions (id) ON DELETE SET NULL
);

CREATE TABLE riot_accounts (
    puuid text PRIMARY KEY,
    user_id text NOT NULL,
    platform text NOT NULL,
    routing_region text NOT NULL,
    game_name text NOT NULL,
    tag_line text NOT NULL,
    normalized_game_name text NOT NULL,
    normalized_tag_line text NOT NULL,
    profile_icon_id integer,
    is_main boolean NOT NULL,
    main_position text,
    main_position_updated_at timestamptz,
    last_synced_at timestamptz,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL,
    CONSTRAINT riot_accounts_user_fk
        FOREIGN KEY (user_id) REFERENCES users (discord_id) ON DELETE RESTRICT,
    CONSTRAINT riot_accounts_normalized_id_key
        UNIQUE (routing_region, normalized_game_name, normalized_tag_line),
    CONSTRAINT riot_accounts_profile_icon_id_check
        CHECK (profile_icon_id >= 0),
    CONSTRAINT riot_accounts_main_position_check
        CHECK (main_position IN ('TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT', 'FLEX'))
);

CREATE UNIQUE INDEX riot_accounts_one_main_per_user_idx
    ON riot_accounts (user_id)
    WHERE is_main;

CREATE TABLE user_privacy_settings (
    user_id text PRIMARY KEY,
    public_profile_enabled boolean NOT NULL,
    show_riot_ids boolean NOT NULL,
    allow_external_record_search boolean NOT NULL,
    updated_at timestamptz NOT NULL,
    CONSTRAINT user_privacy_settings_user_fk
        FOREIGN KEY (user_id) REFERENCES users (discord_id) ON DELETE CASCADE
);

CREATE TABLE user_champion_preferences (
    user_id text NOT NULL,
    lane text NOT NULL,
    position smallint NOT NULL,
    champion_id text NOT NULL,
    CONSTRAINT user_champion_preferences_pkey PRIMARY KEY (user_id, lane, position),
    CONSTRAINT user_champion_preferences_champion_key
        UNIQUE (user_id, lane, champion_id),
    CONSTRAINT user_champion_preferences_user_fk
        FOREIGN KEY (user_id) REFERENCES users (discord_id) ON DELETE CASCADE,
    CONSTRAINT user_champion_preferences_lane_check
        CHECK (lane IN ('TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT')),
    CONSTRAINT user_champion_preferences_position_check
        CHECK (position BETWEEN 1 AND 10)
);
