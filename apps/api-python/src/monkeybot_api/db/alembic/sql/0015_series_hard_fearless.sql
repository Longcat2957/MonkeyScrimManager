ALTER TABLE recruitments
    ADD COLUMN best_of smallint NOT NULL DEFAULT 3,
    ADD COLUMN hard_fearless boolean NOT NULL DEFAULT false,
    ADD CONSTRAINT recruitments_best_of_check CHECK (best_of IN (1, 3, 5));

ALTER TABLE series
    ADD COLUMN hard_fearless boolean NOT NULL DEFAULT false;
