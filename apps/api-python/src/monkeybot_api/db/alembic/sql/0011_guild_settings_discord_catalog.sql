ALTER TABLE guild_config
    ADD COLUMN entry_channel_id text,
    ADD CONSTRAINT guild_config_entry_channel_id_check
        CHECK (public.is_discord_snowflake(entry_channel_id));

CREATE TABLE guild_discord_roles (
    guild_id text NOT NULL,
    role_id text NOT NULL,
    name text NOT NULL,
    position integer NOT NULL,
    managed boolean NOT NULL,
    updated_at timestamptz NOT NULL,
    CONSTRAINT guild_discord_roles_pkey PRIMARY KEY (guild_id, role_id),
    CONSTRAINT guild_discord_roles_guild_fk
        FOREIGN KEY (guild_id) REFERENCES guilds (guild_id) ON DELETE CASCADE,
    CONSTRAINT guild_discord_roles_role_id_check
        CHECK (public.is_discord_snowflake(role_id)),
    CONSTRAINT guild_discord_roles_not_everyone_check CHECK (role_id <> guild_id),
    CONSTRAINT guild_discord_roles_name_check
        CHECK (name = btrim(name) AND char_length(name) BETWEEN 1 AND 100),
    CONSTRAINT guild_discord_roles_position_check CHECK (position >= 0)
);

CREATE TABLE guild_discord_channels (
    guild_id text NOT NULL,
    channel_id text NOT NULL,
    name text NOT NULL,
    parent_name text,
    position integer NOT NULL,
    kind text NOT NULL,
    updated_at timestamptz NOT NULL,
    CONSTRAINT guild_discord_channels_pkey PRIMARY KEY (guild_id, channel_id),
    CONSTRAINT guild_discord_channels_guild_fk
        FOREIGN KEY (guild_id) REFERENCES guilds (guild_id) ON DELETE CASCADE,
    CONSTRAINT guild_discord_channels_channel_id_check
        CHECK (public.is_discord_snowflake(channel_id)),
    CONSTRAINT guild_discord_channels_name_check
        CHECK (name = btrim(name) AND char_length(name) BETWEEN 1 AND 100),
    CONSTRAINT guild_discord_channels_parent_name_check
        CHECK (
            parent_name IS NULL
            OR (
                parent_name = btrim(parent_name)
                AND char_length(parent_name) BETWEEN 1 AND 100
            )
        ),
    CONSTRAINT guild_discord_channels_position_check CHECK (position >= 0),
    CONSTRAINT guild_discord_channels_kind_check CHECK (kind IN ('TEXT', 'ANNOUNCEMENT'))
);

CREATE TABLE guild_discord_catalog_state (
    guild_id text PRIMARY KEY,
    synced_at timestamptz NOT NULL,
    CONSTRAINT guild_discord_catalog_state_guild_fk
        FOREIGN KEY (guild_id) REFERENCES guilds (guild_id) ON DELETE CASCADE
);
