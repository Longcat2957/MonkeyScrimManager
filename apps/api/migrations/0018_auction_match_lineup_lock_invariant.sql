ALTER TABLE auction_matches
    ADD CONSTRAINT auction_matches_lineup_lock_state_check CHECK (
        (lineups_locked_at IS NULL) = (lineups_locked_by IS NULL)
        AND (
            (status IN ('LINEUP_PENDING', 'CANCELLED') AND lineups_locked_at IS NULL)
            OR (status IN ('READY', 'IN_PROGRESS', 'COMPLETED') AND lineups_locked_at IS NOT NULL)
        )
    ) NOT VALID;
