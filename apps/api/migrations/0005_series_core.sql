CREATE TABLE series (
    guild_id text NOT NULL,
    id uuid NOT NULL,
    season_id uuid NOT NULL,
    source_recruitment_id uuid NOT NULL,
    display_number bigint NOT NULL,
    title text NOT NULL,
    best_of smallint NOT NULL,
    status text NOT NULL,
    winning_team text,
    terminal_reason text,
    revision bigint NOT NULL,
    started_at timestamptz NOT NULL,
    ended_at timestamptz,
    created_by text,
    end_card_channel_id text,
    end_card_message_id text,
    updated_by text,
    updated_at timestamptz NOT NULL,
    deleted_at timestamptz,
    CONSTRAINT series_pkey PRIMARY KEY (guild_id, id),
    CONSTRAINT series_display_number_key UNIQUE (guild_id, display_number),
    CONSTRAINT series_season_id_key UNIQUE (guild_id, season_id, id),
    CONSTRAINT series_season_fk
        FOREIGN KEY (guild_id, season_id)
        REFERENCES seasons (guild_id, id) ON DELETE RESTRICT,
    CONSTRAINT series_source_recruitment_fk
        FOREIGN KEY (guild_id, season_id, source_recruitment_id)
        REFERENCES recruitments (guild_id, season_id, id) ON DELETE RESTRICT,
    CONSTRAINT series_created_by_fk
        FOREIGN KEY (guild_id, created_by)
        REFERENCES guild_members (guild_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT series_updated_by_fk
        FOREIGN KEY (guild_id, updated_by)
        REFERENCES guild_members (guild_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT series_end_card_channel_id_check
        CHECK (public.is_discord_snowflake(end_card_channel_id)),
    CONSTRAINT series_end_card_message_id_check
        CHECK (public.is_discord_snowflake(end_card_message_id)),
    CONSTRAINT series_display_number_check CHECK (display_number > 0),
    CONSTRAINT series_best_of_check CHECK (best_of IN (1, 3, 5)),
    CONSTRAINT series_status_check
        CHECK (status IN ('IN_PROGRESS', 'COMPLETED', 'CANCELLED')),
    CONSTRAINT series_winning_team_check
        CHECK (winning_team IN ('TEAM_1', 'TEAM_2')),
    CONSTRAINT series_terminal_reason_check CHECK (
        terminal_reason IN ('SCORE_REACHED', 'EARLY_WIN', 'EARLY_DRAW', 'CANCELLED')
    ),
    CONSTRAINT series_revision_check CHECK (revision >= 0),
    CONSTRAINT series_terminal_state_check CHECK (
        (status = 'IN_PROGRESS'
            AND winning_team IS NULL
            AND terminal_reason IS NULL
            AND ended_at IS NULL)
        OR (status = 'COMPLETED'
            AND terminal_reason IN ('SCORE_REACHED', 'EARLY_WIN')
            AND winning_team IS NOT NULL
            AND ended_at IS NOT NULL)
        OR (status = 'COMPLETED'
            AND terminal_reason = 'EARLY_DRAW'
            AND winning_team IS NULL
            AND ended_at IS NOT NULL)
        OR (status = 'CANCELLED'
            AND terminal_reason = 'CANCELLED'
            AND winning_team IS NULL
            AND ended_at IS NOT NULL)
    )
);

CREATE UNIQUE INDEX series_one_active_per_recruitment_idx
    ON series (guild_id, source_recruitment_id)
    WHERE status <> 'CANCELLED' AND deleted_at IS NULL;

CREATE TABLE series_participants (
    guild_id text NOT NULL,
    series_id uuid NOT NULL,
    user_id text NOT NULL,
    team text NOT NULL,
    lane text NOT NULL,
    mmr_at_lock double precision,
    mmr_snapshot_origin text NOT NULL,
    CONSTRAINT series_participants_pkey PRIMARY KEY (guild_id, series_id, user_id),
    CONSTRAINT series_participants_slot_key UNIQUE (guild_id, series_id, team, lane),
    CONSTRAINT series_participants_series_fk
        FOREIGN KEY (guild_id, series_id)
        REFERENCES series (guild_id, id) ON DELETE RESTRICT,
    CONSTRAINT series_participants_member_fk
        FOREIGN KEY (guild_id, user_id)
        REFERENCES guild_members (guild_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT series_participants_team_check CHECK (team IN ('TEAM_1', 'TEAM_2')),
    CONSTRAINT series_participants_lane_check
        CHECK (lane IN ('TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT')),
    CONSTRAINT series_participants_snapshot_origin_check
        CHECK (mmr_snapshot_origin IN ('CAPTURED', 'INFERRED', 'UNAVAILABLE')),
    CONSTRAINT series_participants_mmr_at_lock_finite_check CHECK (
        mmr_at_lock IS NULL
        OR mmr_at_lock NOT IN (
            'NaN'::double precision,
            'Infinity'::double precision,
            '-Infinity'::double precision
        )
    ),
    CONSTRAINT series_participants_snapshot_check CHECK (
        (mmr_snapshot_origin IN ('CAPTURED', 'INFERRED') AND mmr_at_lock IS NOT NULL)
        OR (mmr_snapshot_origin = 'UNAVAILABLE' AND mmr_at_lock IS NULL)
    )
);
