ALTER TABLE users
    RENAME COLUMN discord_id TO user_id;

ALTER TABLE users
    DROP CONSTRAINT users_discord_id_check;

ALTER TABLE users
    ADD COLUMN identity_kind text NOT NULL DEFAULT 'DISCORD',
    ADD CONSTRAINT users_user_id_check CHECK (
        (identity_kind = 'DISCORD' AND public.is_discord_snowflake(user_id))
        OR (
            identity_kind = 'DUMMY'
            AND user_id ~ '^dmy_[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
        )
    ),
    ADD CONSTRAINT users_identity_kind_check
        CHECK (identity_kind IN ('DISCORD', 'DUMMY')),
    ADD CONSTRAINT users_user_id_identity_kind_key UNIQUE (user_id, identity_kind);

CREATE TABLE user_discord_identities (
    user_id text PRIMARY KEY,
    discord_id text NOT NULL,
    identity_kind text NOT NULL DEFAULT 'DISCORD',
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL,
    CONSTRAINT user_discord_identities_user_fk
        FOREIGN KEY (user_id, identity_kind)
        REFERENCES users (user_id, identity_kind) ON DELETE CASCADE,
    CONSTRAINT user_discord_identities_discord_id_key UNIQUE (discord_id),
    CONSTRAINT user_discord_identities_discord_id_check
        CHECK (public.is_discord_snowflake(discord_id)),
    CONSTRAINT user_discord_identities_kind_check
        CHECK (identity_kind = 'DISCORD')
);

CREATE INDEX user_discord_identities_user_kind_idx
    ON user_discord_identities (user_id, identity_kind);

INSERT INTO user_discord_identities (user_id, discord_id, created_at, updated_at)
SELECT user_id, user_id, created_at, updated_at
FROM users
WHERE identity_kind = 'DISCORD';
