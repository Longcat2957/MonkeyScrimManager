ALTER TABLE auction_recruitments
    ADD COLUMN semifinal_best_of smallint,
    ADD COLUMN final_best_of smallint NOT NULL DEFAULT 3,
    ADD COLUMN hard_fearless boolean NOT NULL DEFAULT false;

UPDATE auction_recruitments
SET semifinal_best_of = 3
WHERE format = 'TWENTY_PERSON';

ALTER TABLE auction_recruitments
    ADD CONSTRAINT auction_recruitments_final_best_of_check
        CHECK (final_best_of IN (1, 3, 5)),
    ADD CONSTRAINT auction_recruitments_semifinal_best_of_check
        CHECK (semifinal_best_of IS NULL OR semifinal_best_of IN (1, 3, 5)),
    ADD CONSTRAINT auction_recruitments_format_series_rules_check CHECK (
        (format = 'TEN_PERSON' AND semifinal_best_of IS NULL)
        OR (format = 'TWENTY_PERSON' AND semifinal_best_of IS NOT NULL)
    );

ALTER TABLE auction_tournaments
    ADD COLUMN semifinal_best_of smallint,
    ADD COLUMN final_best_of smallint NOT NULL DEFAULT 3,
    ADD COLUMN hard_fearless boolean NOT NULL DEFAULT false;

UPDATE auction_tournaments
SET semifinal_best_of = 3
WHERE format = 'TWENTY_PERSON';

ALTER TABLE auction_tournaments
    ADD CONSTRAINT auction_tournaments_final_best_of_check
        CHECK (final_best_of IN (1, 3, 5)),
    ADD CONSTRAINT auction_tournaments_semifinal_best_of_check
        CHECK (semifinal_best_of IS NULL OR semifinal_best_of IN (1, 3, 5)),
    ADD CONSTRAINT auction_tournaments_format_series_rules_check CHECK (
        (format = 'TEN_PERSON' AND semifinal_best_of IS NULL)
        OR (format = 'TWENTY_PERSON' AND semifinal_best_of IS NOT NULL)
    );

ALTER TABLE auction_matches
    ADD COLUMN hard_fearless boolean NOT NULL DEFAULT false;
