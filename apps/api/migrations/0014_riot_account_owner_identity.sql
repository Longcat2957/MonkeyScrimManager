ALTER TABLE riot_accounts
    ADD COLUMN owner_identity_kind text GENERATED ALWAYS AS (
        CASE origin
            WHEN 'RIOT' THEN 'DISCORD'
            WHEN 'DUMMY' THEN 'DUMMY'
        END
    ) STORED,
    ADD CONSTRAINT riot_accounts_owner_identity_fk
        FOREIGN KEY (user_id, owner_identity_kind)
        REFERENCES users (user_id, identity_kind) ON DELETE RESTRICT;

CREATE INDEX riot_accounts_user_identity_kind_idx
    ON riot_accounts (user_id, owner_identity_kind);
