-- 0018 is already applied in deployed databases. Preserve only rows that cannot
-- prove the new lock invariant without inventing a historical lock actor.
ALTER TABLE auction_matches
    ADD COLUMN lineup_lock_invariant_version smallint;

ALTER TABLE auction_matches
    ALTER COLUMN lineup_lock_invariant_version SET DEFAULT 1;

-- Existing rows that already satisfy the invariant become fully enforced. Rows
-- that remain NULL are explicit legacy exemptions and can still receive a later
-- state/revision update without being blocked by the 0018 check.
UPDATE auction_matches
SET lineup_lock_invariant_version = 1
WHERE (lineups_locked_at IS NULL) = (lineups_locked_by IS NULL)
  AND (
      (status IN ('LINEUP_PENDING', 'CANCELLED') AND lineups_locked_at IS NULL)
      OR (status IN ('READY', 'IN_PROGRESS', 'COMPLETED') AND lineups_locked_at IS NOT NULL)
  );

ALTER TABLE auction_matches
    ADD CONSTRAINT auction_matches_lineup_lock_invariant_version_check CHECK (
        lineup_lock_invariant_version IS NULL OR lineup_lock_invariant_version = 1
    ) NOT VALID;

ALTER TABLE auction_matches
    DROP CONSTRAINT auction_matches_lineup_lock_state_check;

ALTER TABLE auction_matches
    ADD CONSTRAINT auction_matches_lineup_lock_state_check CHECK (
        lineup_lock_invariant_version IS NULL
        OR (
            (lineups_locked_at IS NULL) = (lineups_locked_by IS NULL)
            AND (
                (status IN ('LINEUP_PENDING', 'CANCELLED') AND lineups_locked_at IS NULL)
                OR (status IN ('READY', 'IN_PROGRESS', 'COMPLETED') AND lineups_locked_at IS NOT NULL)
            )
        )
    ) NOT VALID;
