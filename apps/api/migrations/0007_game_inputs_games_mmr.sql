CREATE TABLE game_inputs (
    guild_id text NOT NULL,
    id uuid NOT NULL,
    series_id uuid,
    auction_match_id uuid,
    set_number smallint NOT NULL,
    side_preset text NOT NULL,
    winning_side text,
    patch_version text,
    status text NOT NULL,
    updated_by text,
    updated_at timestamptz NOT NULL,
    CONSTRAINT game_inputs_pkey PRIMARY KEY (guild_id, id),
    CONSTRAINT game_inputs_series_result_key
        UNIQUE (guild_id, id, series_id, set_number, side_preset, winning_side),
    CONSTRAINT game_inputs_auction_result_key
        UNIQUE (guild_id, id, auction_match_id, set_number, side_preset, winning_side),
    CONSTRAINT game_inputs_series_fk
        FOREIGN KEY (guild_id, series_id)
        REFERENCES series (guild_id, id) ON DELETE RESTRICT,
    CONSTRAINT game_inputs_auction_match_fk
        FOREIGN KEY (guild_id, auction_match_id)
        REFERENCES auction_matches (guild_id, id) ON DELETE RESTRICT,
    CONSTRAINT game_inputs_updated_by_fk
        FOREIGN KEY (guild_id, updated_by)
        REFERENCES guild_members (guild_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT game_inputs_parent_check
        CHECK (num_nonnulls(series_id, auction_match_id) = 1),
    CONSTRAINT game_inputs_set_number_check CHECK (set_number BETWEEN 1 AND 5),
    CONSTRAINT game_inputs_side_preset_check
        CHECK (side_preset IN ('DEFAULT', 'SWAPPED')),
    CONSTRAINT game_inputs_winning_side_check
        CHECK (winning_side IN ('BLUE', 'RED')),
    CONSTRAINT game_inputs_status_check
        CHECK (status IN ('DRAFT', 'SUBMITTED', 'SUPERSEDED')),
    CONSTRAINT game_inputs_submitted_winner_check
        CHECK (status <> 'SUBMITTED' OR winning_side IS NOT NULL)
);

CREATE UNIQUE INDEX game_inputs_one_live_series_set_idx
    ON game_inputs (guild_id, series_id, set_number)
    WHERE series_id IS NOT NULL AND status IN ('DRAFT', 'SUBMITTED');

CREATE UNIQUE INDEX game_inputs_one_live_auction_set_idx
    ON game_inputs (guild_id, auction_match_id, set_number)
    WHERE auction_match_id IS NOT NULL AND status IN ('DRAFT', 'SUBMITTED');

CREATE TABLE game_input_picks (
    guild_id text NOT NULL,
    game_input_id uuid NOT NULL,
    user_id text NOT NULL,
    champion_id text NOT NULL,
    CONSTRAINT game_input_picks_pkey PRIMARY KEY (guild_id, game_input_id, user_id),
    CONSTRAINT game_input_picks_input_fk
        FOREIGN KEY (guild_id, game_input_id)
        REFERENCES game_inputs (guild_id, id) ON DELETE CASCADE,
    CONSTRAINT game_input_picks_member_fk
        FOREIGN KEY (guild_id, user_id)
        REFERENCES guild_members (guild_id, user_id) ON DELETE RESTRICT
);

CREATE TABLE game_input_bans (
    guild_id text NOT NULL,
    game_input_id uuid NOT NULL,
    side text NOT NULL,
    slot smallint NOT NULL,
    champion_id text NOT NULL,
    CONSTRAINT game_input_bans_pkey PRIMARY KEY (guild_id, game_input_id, side, slot),
    CONSTRAINT game_input_bans_input_fk
        FOREIGN KEY (guild_id, game_input_id)
        REFERENCES game_inputs (guild_id, id) ON DELETE CASCADE,
    CONSTRAINT game_input_bans_side_check CHECK (side IN ('BLUE', 'RED')),
    CONSTRAINT game_input_bans_slot_check CHECK (slot BETWEEN 1 AND 5)
);

CREATE TABLE games (
    guild_id text NOT NULL,
    id uuid NOT NULL,
    season_id uuid NOT NULL,
    series_id uuid,
    auction_match_id uuid,
    source_input_id uuid NOT NULL,
    game_number smallint NOT NULL,
    side_preset text NOT NULL,
    winning_side text NOT NULL,
    duration_seconds integer,
    patch_version text,
    riot_match_id text,
    status text NOT NULL,
    mmr_status text NOT NULL,
    played_at timestamptz NOT NULL,
    confirmed_by text NOT NULL,
    confirmed_by_type text NOT NULL,
    confirmed_at timestamptz NOT NULL,
    voided_by text,
    voided_by_type text,
    voided_at timestamptz,
    void_reason text,
    CONSTRAINT games_pkey PRIMARY KEY (guild_id, id),
    CONSTRAINT games_source_input_key UNIQUE (guild_id, source_input_id),
    CONSTRAINT games_season_id_key UNIQUE (guild_id, season_id, id),
    CONSTRAINT games_season_fk
        FOREIGN KEY (guild_id, season_id)
        REFERENCES seasons (guild_id, id) ON DELETE RESTRICT,
    CONSTRAINT games_series_fk
        FOREIGN KEY (guild_id, season_id, series_id)
        REFERENCES series (guild_id, season_id, id) ON DELETE RESTRICT,
    CONSTRAINT games_auction_match_fk
        FOREIGN KEY (guild_id, season_id, auction_match_id)
        REFERENCES auction_matches (guild_id, season_id, id) ON DELETE RESTRICT,
    CONSTRAINT games_series_input_fk
        FOREIGN KEY (
            guild_id,
            source_input_id,
            series_id,
            game_number,
            side_preset,
            winning_side
        )
        REFERENCES game_inputs (
            guild_id,
            id,
            series_id,
            set_number,
            side_preset,
            winning_side
        ) ON DELETE RESTRICT,
    CONSTRAINT games_auction_input_fk
        FOREIGN KEY (
            guild_id,
            source_input_id,
            auction_match_id,
            game_number,
            side_preset,
            winning_side
        )
        REFERENCES game_inputs (
            guild_id,
            id,
            auction_match_id,
            set_number,
            side_preset,
            winning_side
        ) ON DELETE RESTRICT,
    CONSTRAINT games_parent_check
        CHECK (num_nonnulls(series_id, auction_match_id) = 1),
    CONSTRAINT games_game_number_check CHECK (game_number BETWEEN 1 AND 5),
    CONSTRAINT games_side_preset_check
        CHECK (side_preset IN ('DEFAULT', 'SWAPPED')),
    CONSTRAINT games_winning_side_check CHECK (winning_side IN ('BLUE', 'RED')),
    CONSTRAINT games_duration_seconds_check CHECK (duration_seconds > 0),
    CONSTRAINT games_status_check CHECK (status IN ('CONFIRMED', 'VOIDED')),
    CONSTRAINT games_mmr_status_check
        CHECK (mmr_status IN ('APPLIED', 'LEGACY_NOT_APPLIED', 'REVERSED')),
    CONSTRAINT games_confirmed_by_type_check
        CHECK (confirmed_by_type IN ('USER', 'BOT', 'MIGRATION')),
    CONSTRAINT games_voided_by_type_check
        CHECK (voided_by_type IN ('USER', 'BOT', 'SYSTEM')),
    CONSTRAINT games_void_lifecycle_check CHECK (
        (status = 'VOIDED'
            AND voided_by IS NOT NULL
            AND voided_by_type IS NOT NULL
            AND voided_at IS NOT NULL
            AND void_reason IS NOT NULL)
        OR (status = 'CONFIRMED'
            AND voided_by IS NULL
            AND voided_by_type IS NULL
            AND voided_at IS NULL
            AND void_reason IS NULL)
    ),
    CONSTRAINT games_status_mmr_check CHECK (
        (status = 'CONFIRMED' AND mmr_status IN ('APPLIED', 'LEGACY_NOT_APPLIED'))
        OR (status = 'VOIDED' AND mmr_status IN ('REVERSED', 'LEGACY_NOT_APPLIED'))
    )
);

CREATE UNIQUE INDEX games_riot_match_key
    ON games (guild_id, riot_match_id)
    WHERE riot_match_id IS NOT NULL;

CREATE UNIQUE INDEX games_one_confirmed_series_number_idx
    ON games (guild_id, series_id, game_number)
    WHERE series_id IS NOT NULL AND status = 'CONFIRMED';

CREATE UNIQUE INDEX games_one_confirmed_auction_number_idx
    ON games (guild_id, auction_match_id, game_number)
    WHERE auction_match_id IS NOT NULL AND status = 'CONFIRMED';

CREATE TABLE game_participants (
    guild_id text NOT NULL,
    game_id uuid NOT NULL,
    user_id text NOT NULL,
    team text NOT NULL,
    lane text NOT NULL,
    champion_id text,
    CONSTRAINT game_participants_pkey PRIMARY KEY (guild_id, game_id, user_id),
    CONSTRAINT game_participants_slot_key UNIQUE (guild_id, game_id, team, lane),
    CONSTRAINT game_participants_game_fk
        FOREIGN KEY (guild_id, game_id)
        REFERENCES games (guild_id, id) ON DELETE RESTRICT,
    CONSTRAINT game_participants_member_fk
        FOREIGN KEY (guild_id, user_id)
        REFERENCES guild_members (guild_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT game_participants_team_check CHECK (team IN ('TEAM_1', 'TEAM_2')),
    CONSTRAINT game_participants_lane_check
        CHECK (lane IN ('TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT'))
);

CREATE TABLE game_participant_stats (
    guild_id text NOT NULL,
    game_id uuid NOT NULL,
    user_id text NOT NULL,
    kills integer,
    deaths integer,
    assists integer,
    cs integer,
    CONSTRAINT game_participant_stats_pkey PRIMARY KEY (guild_id, game_id, user_id),
    CONSTRAINT game_participant_stats_participant_fk
        FOREIGN KEY (guild_id, game_id, user_id)
        REFERENCES game_participants (guild_id, game_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT game_participant_stats_kills_check CHECK (kills >= 0),
    CONSTRAINT game_participant_stats_deaths_check CHECK (deaths >= 0),
    CONSTRAINT game_participant_stats_assists_check CHECK (assists >= 0),
    CONSTRAINT game_participant_stats_cs_check CHECK (cs >= 0)
);

CREATE TABLE game_bans (
    guild_id text NOT NULL,
    game_id uuid NOT NULL,
    side text NOT NULL,
    slot smallint NOT NULL,
    champion_id text NOT NULL,
    CONSTRAINT game_bans_pkey PRIMARY KEY (guild_id, game_id, side, slot),
    CONSTRAINT game_bans_game_fk
        FOREIGN KEY (guild_id, game_id)
        REFERENCES games (guild_id, id) ON DELETE RESTRICT,
    CONSTRAINT game_bans_side_check CHECK (side IN ('BLUE', 'RED')),
    CONSTRAINT game_bans_slot_check CHECK (slot BETWEEN 1 AND 5)
);

CREATE TABLE user_lane_mmr (
    guild_id text NOT NULL,
    season_id uuid NOT NULL,
    user_id text NOT NULL,
    lane text NOT NULL,
    mmr double precision NOT NULL DEFAULT 1500,
    games_played integer NOT NULL DEFAULT 0,
    wins integer NOT NULL DEFAULT 0,
    revision bigint NOT NULL,
    updated_at timestamptz NOT NULL,
    CONSTRAINT user_lane_mmr_pkey PRIMARY KEY (guild_id, season_id, user_id, lane),
    CONSTRAINT user_lane_mmr_season_fk
        FOREIGN KEY (guild_id, season_id)
        REFERENCES seasons (guild_id, id) ON DELETE RESTRICT,
    CONSTRAINT user_lane_mmr_member_fk
        FOREIGN KEY (guild_id, user_id)
        REFERENCES guild_members (guild_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT user_lane_mmr_lane_check
        CHECK (lane IN ('TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT')),
    CONSTRAINT user_lane_mmr_mmr_finite_check CHECK (
        mmr NOT IN (
            'NaN'::double precision,
            'Infinity'::double precision,
            '-Infinity'::double precision
        )
    ),
    CONSTRAINT user_lane_mmr_games_played_check CHECK (games_played >= 0),
    CONSTRAINT user_lane_mmr_wins_check CHECK (wins >= 0 AND wins <= games_played),
    CONSTRAINT user_lane_mmr_revision_check CHECK (revision >= 0)
);

CREATE TABLE mmr_ledger_entries (
    guild_id text NOT NULL,
    id uuid NOT NULL,
    season_id uuid NOT NULL,
    user_id text NOT NULL,
    lane text NOT NULL,
    lane_revision bigint NOT NULL,
    source_type text NOT NULL,
    source_id uuid NOT NULL,
    game_id uuid,
    opponent_user_id text,
    mmr_before double precision NOT NULL,
    mmr_after double precision NOT NULL,
    delta double precision NOT NULL,
    games_played_delta smallint NOT NULL,
    wins_delta smallint NOT NULL,
    reverses_entry_id uuid,
    reversed_source_type text,
    actor_type text NOT NULL,
    actor_id text NOT NULL,
    note text,
    request_id text,
    created_at timestamptz NOT NULL,
    CONSTRAINT mmr_ledger_entries_pkey PRIMARY KEY (guild_id, id),
    CONSTRAINT mmr_ledger_entries_identity_key
        UNIQUE (guild_id, id, season_id, user_id, lane),
    CONSTRAINT mmr_ledger_entries_typed_identity_key
        UNIQUE (guild_id, id, season_id, user_id, lane, source_type),
    CONSTRAINT mmr_ledger_entries_lane_revision_key
        UNIQUE (guild_id, season_id, user_id, lane, lane_revision),
    CONSTRAINT mmr_ledger_entries_source_key
        UNIQUE (guild_id, source_type, source_id, user_id, lane),
    CONSTRAINT mmr_ledger_entries_season_fk
        FOREIGN KEY (guild_id, season_id)
        REFERENCES seasons (guild_id, id) ON DELETE RESTRICT,
    CONSTRAINT mmr_ledger_entries_member_fk
        FOREIGN KEY (guild_id, user_id)
        REFERENCES guild_members (guild_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT mmr_ledger_entries_game_fk
        FOREIGN KEY (guild_id, season_id, game_id)
        REFERENCES games (guild_id, season_id, id) ON DELETE RESTRICT,
    CONSTRAINT mmr_ledger_entries_opponent_member_fk
        FOREIGN KEY (guild_id, opponent_user_id)
        REFERENCES guild_members (guild_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT mmr_ledger_entries_game_user_fk
        FOREIGN KEY (guild_id, game_id, user_id)
        REFERENCES game_participants (guild_id, game_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT mmr_ledger_entries_game_opponent_fk
        FOREIGN KEY (guild_id, game_id, opponent_user_id)
        REFERENCES game_participants (guild_id, game_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT mmr_ledger_entries_reversal_fk
        FOREIGN KEY (
            guild_id,
            reverses_entry_id,
            season_id,
            user_id,
            lane,
            reversed_source_type
        )
        REFERENCES mmr_ledger_entries (
            guild_id,
            id,
            season_id,
            user_id,
            lane,
            source_type
        ) ON DELETE RESTRICT,
    CONSTRAINT mmr_ledger_entries_lane_check
        CHECK (lane IN ('TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT')),
    CONSTRAINT mmr_ledger_entries_lane_revision_check CHECK (lane_revision > 0),
    CONSTRAINT mmr_ledger_entries_source_type_check CHECK (
        source_type IN (
            'GAME_RESULT',
            'GAME_REVERSAL',
            'MANUAL_ADJUSTMENT',
            'MANUAL_REVERSAL',
            'MIGRATION_SEED'
        )
    ),
    CONSTRAINT mmr_ledger_entries_reversed_type_check
        CHECK (reversed_source_type IN ('GAME_RESULT', 'MANUAL_ADJUSTMENT')),
    CONSTRAINT mmr_ledger_entries_actor_type_check
        CHECK (actor_type IN ('USER', 'BOT', 'SYSTEM', 'MIGRATION')),
    CONSTRAINT mmr_ledger_entries_mmr_before_finite_check CHECK (
        mmr_before NOT IN (
            'NaN'::double precision,
            'Infinity'::double precision,
            '-Infinity'::double precision
        )
    ),
    CONSTRAINT mmr_ledger_entries_mmr_after_finite_check CHECK (
        mmr_after NOT IN (
            'NaN'::double precision,
            'Infinity'::double precision,
            '-Infinity'::double precision
        )
    ),
    CONSTRAINT mmr_ledger_entries_delta_finite_check CHECK (
        delta NOT IN (
            'NaN'::double precision,
            'Infinity'::double precision,
            '-Infinity'::double precision
        )
    ),
    CONSTRAINT mmr_ledger_entries_manual_delta_check
        CHECK (source_type <> 'MANUAL_ADJUSTMENT' OR delta <> 0),
    CONSTRAINT mmr_ledger_entries_reversal_presence_check CHECK (
        (source_type IN ('GAME_REVERSAL', 'MANUAL_REVERSAL'))
        = (reverses_entry_id IS NOT NULL)
    ),
    CONSTRAINT mmr_ledger_entries_reversal_type_check CHECK (
        (source_type = 'GAME_REVERSAL' AND reversed_source_type = 'GAME_RESULT')
        OR (source_type = 'MANUAL_REVERSAL'
            AND reversed_source_type = 'MANUAL_ADJUSTMENT')
        OR (source_type NOT IN ('GAME_REVERSAL', 'MANUAL_REVERSAL')
            AND reversed_source_type IS NULL)
    ),
    CONSTRAINT mmr_ledger_entries_game_presence_check CHECK (
        (source_type IN ('GAME_RESULT', 'GAME_REVERSAL')) = (game_id IS NOT NULL)
    ),
    CONSTRAINT mmr_ledger_entries_distinct_opponent_check CHECK (
        source_type NOT IN ('GAME_RESULT', 'GAME_REVERSAL')
        OR user_id <> opponent_user_id
    ),
    CONSTRAINT mmr_ledger_entries_manual_note_check
        CHECK (source_type <> 'MANUAL_ADJUSTMENT' OR note IS NOT NULL),
    CONSTRAINT mmr_ledger_entries_count_delta_check CHECK (
        (source_type = 'GAME_RESULT'
            AND opponent_user_id IS NOT NULL
            AND games_played_delta = 1
            AND wins_delta IN (0, 1))
        OR (source_type = 'GAME_REVERSAL'
            AND opponent_user_id IS NOT NULL
            AND games_played_delta = -1
            AND wins_delta IN (-1, 0))
        OR (source_type IN ('MANUAL_ADJUSTMENT', 'MANUAL_REVERSAL')
            AND opponent_user_id IS NULL
            AND games_played_delta = 0
            AND wins_delta = 0)
        OR (source_type = 'MIGRATION_SEED' AND opponent_user_id IS NULL)
    )
);

CREATE UNIQUE INDEX mmr_ledger_one_game_result_idx
    ON mmr_ledger_entries (guild_id, game_id, user_id, lane)
    WHERE source_type = 'GAME_RESULT' AND game_id IS NOT NULL;

CREATE UNIQUE INDEX mmr_ledger_one_reversal_idx
    ON mmr_ledger_entries (guild_id, reverses_entry_id)
    WHERE reverses_entry_id IS NOT NULL;
