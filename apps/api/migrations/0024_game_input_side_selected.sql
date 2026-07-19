ALTER TABLE game_inputs
    ADD COLUMN side_selected boolean NOT NULL DEFAULT true;

ALTER TABLE game_inputs
    ADD CONSTRAINT game_inputs_submitted_side_selected_check
        CHECK (status <> 'SUBMITTED' OR side_selected);
