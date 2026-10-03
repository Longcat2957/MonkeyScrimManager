ALTER TABLE user_champion_preferences
    ADD CONSTRAINT user_champion_preferences_canonical_champion_id_check
        CHECK (
            octet_length(champion_id) BETWEEN 1 AND 64
            AND champion_id ~ '^[A-Za-z0-9]+$'
        ) NOT VALID;

ALTER TABLE game_input_picks
    ADD CONSTRAINT game_input_picks_canonical_champion_id_check
        CHECK (
            octet_length(champion_id) BETWEEN 1 AND 64
            AND champion_id ~ '^[A-Za-z0-9]+$'
        ) NOT VALID;

ALTER TABLE game_input_bans
    ADD CONSTRAINT game_input_bans_canonical_champion_id_check
        CHECK (
            octet_length(champion_id) BETWEEN 1 AND 64
            AND champion_id ~ '^[A-Za-z0-9]+$'
        ) NOT VALID;

ALTER TABLE game_participants
    ADD CONSTRAINT game_participants_canonical_champion_id_check
        CHECK (
            champion_id IS NULL
            OR (
                octet_length(champion_id) BETWEEN 1 AND 64
                AND champion_id ~ '^[A-Za-z0-9]+$'
            )
        ) NOT VALID;

ALTER TABLE game_bans
    ADD CONSTRAINT game_bans_canonical_champion_id_check
        CHECK (
            octet_length(champion_id) BETWEEN 1 AND 64
            AND champion_id ~ '^[A-Za-z0-9]+$'
        ) NOT VALID;

ALTER TABLE user_champion_preferences
    VALIDATE CONSTRAINT user_champion_preferences_canonical_champion_id_check;

ALTER TABLE game_input_picks
    VALIDATE CONSTRAINT game_input_picks_canonical_champion_id_check;

ALTER TABLE game_input_bans
    VALIDATE CONSTRAINT game_input_bans_canonical_champion_id_check;

ALTER TABLE game_participants
    VALIDATE CONSTRAINT game_participants_canonical_champion_id_check;

ALTER TABLE game_bans
    VALIDATE CONSTRAINT game_bans_canonical_champion_id_check;
