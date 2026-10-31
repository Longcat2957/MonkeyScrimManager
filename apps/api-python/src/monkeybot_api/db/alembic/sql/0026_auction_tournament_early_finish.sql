-- A manual completion preserves played games/MMR while allowing a tied or unplayed
-- bracket to end without inventing a champion or missing lineup lock provenance.
ALTER TABLE auction_tournaments
    ADD COLUMN ended_early boolean NOT NULL DEFAULT false;
ALTER TABLE auction_matches
    ADD COLUMN ended_early boolean NOT NULL DEFAULT false;

ALTER TABLE auction_tournaments
    DROP CONSTRAINT auction_tournaments_terminal_state_check,
    ADD CONSTRAINT auction_tournaments_terminal_state_check CHECK (
        (status IN ('BIDDING', 'BRACKET_SETUP', 'IN_GAME')
            AND champion_team_id IS NULL AND ended_at IS NULL AND NOT ended_early)
        OR (status = 'COMPLETED' AND ended_at IS NOT NULL
            AND (champion_team_id IS NOT NULL OR ended_early))
        OR (status = 'CANCELLED' AND champion_team_id IS NULL
            AND ended_at IS NOT NULL AND NOT ended_early)
    );

ALTER TABLE auction_matches
    DROP CONSTRAINT auction_matches_terminal_state_check,
    ADD CONSTRAINT auction_matches_terminal_state_check CHECK (
        (status IN ('LINEUP_PENDING', 'READY', 'IN_PROGRESS')
            AND winner_team_id IS NULL AND ended_at IS NULL AND NOT ended_early)
        OR (status = 'COMPLETED' AND ended_at IS NOT NULL
            AND (winner_team_id IS NOT NULL OR ended_early))
        OR (status = 'CANCELLED' AND winner_team_id IS NULL
            AND ended_at IS NOT NULL AND NOT ended_early)
    ),
    DROP CONSTRAINT auction_matches_lineup_lock_state_check,
    ADD CONSTRAINT auction_matches_lineup_lock_state_check CHECK (
        lineup_lock_invariant_version IS NULL
        OR (
            (lineups_locked_at IS NULL) = (lineups_locked_by IS NULL)
            AND (
                (status IN ('LINEUP_PENDING', 'CANCELLED') AND lineups_locked_at IS NULL)
                OR (status IN ('READY', 'IN_PROGRESS', 'COMPLETED') AND lineups_locked_at IS NOT NULL)
                OR (status = 'COMPLETED' AND ended_early)
            )
        )
    ) NOT VALID;
