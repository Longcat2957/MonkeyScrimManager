CREATE TABLE auction_recruitments (
    guild_id text NOT NULL,
    id uuid NOT NULL,
    season_id uuid NOT NULL,
    display_number bigint NOT NULL,
    title text NOT NULL,
    format text NOT NULL,
    target_count smallint NOT NULL,
    status text NOT NULL,
    revision bigint NOT NULL,
    channel_id text,
    message_id text,
    created_by text NOT NULL,
    created_at timestamptz NOT NULL,
    updated_by text,
    updated_at timestamptz NOT NULL,
    closed_at timestamptz,
    converted_at timestamptz,
    cancelled_at timestamptz,
    CONSTRAINT auction_recruitments_pkey PRIMARY KEY (guild_id, id),
    CONSTRAINT auction_recruitments_display_number_key UNIQUE (guild_id, display_number),
    CONSTRAINT auction_recruitments_season_id_key UNIQUE (guild_id, season_id, id),
    CONSTRAINT auction_recruitments_season_fk
        FOREIGN KEY (guild_id, season_id)
        REFERENCES seasons (guild_id, id) ON DELETE RESTRICT,
    CONSTRAINT auction_recruitments_created_by_fk
        FOREIGN KEY (guild_id, created_by)
        REFERENCES guild_members (guild_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT auction_recruitments_updated_by_fk
        FOREIGN KEY (guild_id, updated_by)
        REFERENCES guild_members (guild_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT auction_recruitments_channel_id_check
        CHECK (public.is_discord_snowflake(channel_id)),
    CONSTRAINT auction_recruitments_message_id_check
        CHECK (public.is_discord_snowflake(message_id)),
    CONSTRAINT auction_recruitments_display_number_check CHECK (display_number > 0),
    CONSTRAINT auction_recruitments_format_check
        CHECK (format IN ('TEN_PERSON', 'TWENTY_PERSON')),
    CONSTRAINT auction_recruitments_target_count_check CHECK (target_count IN (10, 20)),
    CONSTRAINT auction_recruitments_status_check
        CHECK (status IN ('OPEN', 'CLOSED', 'CONVERTED', 'CANCELLED')),
    CONSTRAINT auction_recruitments_revision_check CHECK (revision >= 0),
    CONSTRAINT auction_recruitments_format_target_check CHECK (
        (format = 'TEN_PERSON' AND target_count = 10)
        OR (format = 'TWENTY_PERSON' AND target_count = 20)
    )
);

CREATE TABLE auction_recruitment_participants (
    guild_id text NOT NULL,
    recruitment_id uuid NOT NULL,
    user_id text NOT NULL,
    status text NOT NULL,
    declared_lane text,
    mmr_at_join double precision,
    snapshot_origin text NOT NULL,
    joined_at timestamptz NOT NULL,
    withdrawn_at timestamptz,
    CONSTRAINT auction_recruitment_participants_pkey
        PRIMARY KEY (guild_id, recruitment_id, user_id),
    CONSTRAINT auction_recruitment_participants_recruitment_fk
        FOREIGN KEY (guild_id, recruitment_id)
        REFERENCES auction_recruitments (guild_id, id) ON DELETE RESTRICT,
    CONSTRAINT auction_recruitment_participants_member_fk
        FOREIGN KEY (guild_id, user_id)
        REFERENCES guild_members (guild_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT auction_recruitment_participants_status_check
        CHECK (status IN ('JOINED', 'WITHDRAWN')),
    CONSTRAINT auction_recruitment_participants_lane_check
        CHECK (declared_lane IN ('TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT')),
    CONSTRAINT auction_recruitment_participants_origin_check
        CHECK (snapshot_origin IN ('CAPTURED', 'INFERRED', 'UNAVAILABLE')),
    CONSTRAINT auction_recruitment_participants_mmr_finite_check CHECK (
        mmr_at_join IS NULL
        OR mmr_at_join NOT IN (
            'NaN'::double precision,
            'Infinity'::double precision,
            '-Infinity'::double precision
        )
    ),
    CONSTRAINT auction_recruitment_participants_snapshot_check CHECK (
        (snapshot_origin IN ('CAPTURED', 'INFERRED')
            AND declared_lane IS NOT NULL
            AND mmr_at_join IS NOT NULL)
        OR (snapshot_origin = 'UNAVAILABLE'
            AND (declared_lane IS NULL OR mmr_at_join IS NULL))
    )
);

CREATE TABLE auction_setup_drafts (
    guild_id text NOT NULL,
    recruitment_id uuid NOT NULL,
    revision bigint NOT NULL,
    updated_by text,
    updated_at timestamptz NOT NULL,
    CONSTRAINT auction_setup_drafts_pkey PRIMARY KEY (guild_id, recruitment_id),
    CONSTRAINT auction_setup_drafts_recruitment_fk
        FOREIGN KEY (guild_id, recruitment_id)
        REFERENCES auction_recruitments (guild_id, id) ON DELETE RESTRICT,
    CONSTRAINT auction_setup_drafts_updated_by_fk
        FOREIGN KEY (guild_id, updated_by)
        REFERENCES guild_members (guild_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT auction_setup_drafts_revision_check CHECK (revision >= 0)
);

CREATE TABLE auction_setup_teams (
    guild_id text NOT NULL,
    recruitment_id uuid NOT NULL,
    team_index smallint NOT NULL,
    team_name text NOT NULL,
    captain_user_id text,
    initial_points integer NOT NULL,
    CONSTRAINT auction_setup_teams_pkey PRIMARY KEY (guild_id, recruitment_id, team_index),
    CONSTRAINT auction_setup_teams_captain_key
        UNIQUE (guild_id, recruitment_id, captain_user_id),
    CONSTRAINT auction_setup_teams_draft_fk
        FOREIGN KEY (guild_id, recruitment_id)
        REFERENCES auction_setup_drafts (guild_id, recruitment_id) ON DELETE CASCADE,
    CONSTRAINT auction_setup_teams_captain_fk
        FOREIGN KEY (guild_id, recruitment_id, captain_user_id)
        REFERENCES auction_recruitment_participants (guild_id, recruitment_id, user_id)
        ON DELETE RESTRICT,
    CONSTRAINT auction_setup_teams_team_index_check CHECK (team_index BETWEEN 1 AND 4),
    CONSTRAINT auction_setup_teams_initial_points_check CHECK (initial_points > 0)
);

CREATE TABLE auction_tournaments (
    guild_id text NOT NULL,
    id uuid NOT NULL,
    season_id uuid NOT NULL,
    source_recruitment_id uuid NOT NULL,
    display_number bigint NOT NULL,
    title text NOT NULL,
    format text NOT NULL,
    status text NOT NULL,
    revision bigint NOT NULL,
    bracket_locked_at timestamptz,
    bracket_locked_by text,
    champion_team_id uuid,
    started_at timestamptz NOT NULL,
    ended_at timestamptz,
    created_by text NOT NULL,
    updated_by text,
    updated_at timestamptz NOT NULL,
    end_card_channel_id text,
    end_card_message_id text,
    deleted_at timestamptz,
    CONSTRAINT auction_tournaments_pkey PRIMARY KEY (guild_id, id),
    CONSTRAINT auction_tournaments_display_number_key UNIQUE (guild_id, display_number),
    CONSTRAINT auction_tournaments_season_id_key UNIQUE (guild_id, season_id, id),
    CONSTRAINT auction_tournaments_season_fk
        FOREIGN KEY (guild_id, season_id)
        REFERENCES seasons (guild_id, id) ON DELETE RESTRICT,
    CONSTRAINT auction_tournaments_source_recruitment_fk
        FOREIGN KEY (guild_id, season_id, source_recruitment_id)
        REFERENCES auction_recruitments (guild_id, season_id, id) ON DELETE RESTRICT,
    CONSTRAINT auction_tournaments_created_by_fk
        FOREIGN KEY (guild_id, created_by)
        REFERENCES guild_members (guild_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT auction_tournaments_bracket_locked_by_fk
        FOREIGN KEY (guild_id, bracket_locked_by)
        REFERENCES guild_members (guild_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT auction_tournaments_updated_by_fk
        FOREIGN KEY (guild_id, updated_by)
        REFERENCES guild_members (guild_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT auction_tournaments_end_card_channel_id_check
        CHECK (public.is_discord_snowflake(end_card_channel_id)),
    CONSTRAINT auction_tournaments_end_card_message_id_check
        CHECK (public.is_discord_snowflake(end_card_message_id)),
    CONSTRAINT auction_tournaments_display_number_check CHECK (display_number > 0),
    CONSTRAINT auction_tournaments_format_check
        CHECK (format IN ('TEN_PERSON', 'TWENTY_PERSON')),
    CONSTRAINT auction_tournaments_status_check CHECK (
        status IN ('BIDDING', 'BRACKET_SETUP', 'IN_GAME', 'COMPLETED', 'CANCELLED')
    ),
    CONSTRAINT auction_tournaments_revision_check CHECK (revision >= 0),
    CONSTRAINT auction_tournaments_terminal_state_check CHECK (
        (status IN ('BIDDING', 'BRACKET_SETUP', 'IN_GAME')
            AND champion_team_id IS NULL
            AND ended_at IS NULL)
        OR (status = 'COMPLETED'
            AND champion_team_id IS NOT NULL
            AND ended_at IS NOT NULL)
        OR (status = 'CANCELLED'
            AND champion_team_id IS NULL
            AND ended_at IS NOT NULL)
    )
);

CREATE UNIQUE INDEX auction_tournaments_one_active_recruitment_idx
    ON auction_tournaments (guild_id, source_recruitment_id)
    WHERE status <> 'CANCELLED' AND deleted_at IS NULL;

CREATE TABLE auction_teams (
    guild_id text NOT NULL,
    tournament_id uuid NOT NULL,
    id uuid NOT NULL,
    team_index smallint NOT NULL,
    team_name text NOT NULL,
    initial_points integer NOT NULL,
    current_points integer NOT NULL,
    point_revision bigint NOT NULL,
    created_at timestamptz NOT NULL,
    CONSTRAINT auction_teams_pkey PRIMARY KEY (guild_id, tournament_id, id),
    CONSTRAINT auction_teams_team_index_key UNIQUE (guild_id, tournament_id, team_index),
    CONSTRAINT auction_teams_tournament_fk
        FOREIGN KEY (guild_id, tournament_id)
        REFERENCES auction_tournaments (guild_id, id) ON DELETE RESTRICT,
    CONSTRAINT auction_teams_team_index_check CHECK (team_index BETWEEN 1 AND 4),
    CONSTRAINT auction_teams_initial_points_check CHECK (initial_points > 0),
    CONSTRAINT auction_teams_current_points_check CHECK (current_points >= 0),
    CONSTRAINT auction_teams_point_revision_check CHECK (point_revision >= 0)
);

ALTER TABLE auction_tournaments
    ADD CONSTRAINT auction_tournaments_champion_team_fk
    FOREIGN KEY (guild_id, id, champion_team_id)
    REFERENCES auction_teams (guild_id, tournament_id, id) ON DELETE RESTRICT;

CREATE TABLE auction_bid_candidates (
    guild_id text NOT NULL,
    tournament_id uuid NOT NULL,
    user_id text NOT NULL,
    declared_lane text,
    mmr_at_start double precision,
    snapshot_origin text NOT NULL,
    queue_position bigint NOT NULL,
    state text NOT NULL,
    draw_count integer NOT NULL,
    updated_at timestamptz NOT NULL,
    CONSTRAINT auction_bid_candidates_pkey PRIMARY KEY (guild_id, tournament_id, user_id),
    CONSTRAINT auction_bid_candidates_queue_position_key
        UNIQUE (guild_id, tournament_id, queue_position),
    CONSTRAINT auction_bid_candidates_tournament_fk
        FOREIGN KEY (guild_id, tournament_id)
        REFERENCES auction_tournaments (guild_id, id) ON DELETE RESTRICT,
    CONSTRAINT auction_bid_candidates_member_fk
        FOREIGN KEY (guild_id, user_id)
        REFERENCES guild_members (guild_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT auction_bid_candidates_lane_check
        CHECK (declared_lane IN ('TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT')),
    CONSTRAINT auction_bid_candidates_origin_check
        CHECK (snapshot_origin IN ('CAPTURED', 'INFERRED', 'UNAVAILABLE')),
    CONSTRAINT auction_bid_candidates_queue_position_check CHECK (queue_position >= 0),
    CONSTRAINT auction_bid_candidates_state_check
        CHECK (state IN ('QUEUED', 'ACTIVE', 'UNSOLD', 'ASSIGNED')),
    CONSTRAINT auction_bid_candidates_draw_count_check CHECK (draw_count >= 0),
    CONSTRAINT auction_bid_candidates_mmr_finite_check CHECK (
        mmr_at_start IS NULL
        OR mmr_at_start NOT IN (
            'NaN'::double precision,
            'Infinity'::double precision,
            '-Infinity'::double precision
        )
    ),
    CONSTRAINT auction_bid_candidates_snapshot_check CHECK (
        (snapshot_origin IN ('CAPTURED', 'INFERRED')
            AND declared_lane IS NOT NULL
            AND mmr_at_start IS NOT NULL)
        OR (snapshot_origin = 'UNAVAILABLE'
            AND (declared_lane IS NULL OR mmr_at_start IS NULL))
    )
);

CREATE UNIQUE INDEX auction_bid_candidates_one_active_idx
    ON auction_bid_candidates (guild_id, tournament_id)
    WHERE state = 'ACTIVE';

CREATE TABLE auction_lots (
    guild_id text NOT NULL,
    tournament_id uuid NOT NULL,
    id uuid NOT NULL,
    draw_sequence bigint NOT NULL,
    target_user_id text NOT NULL,
    status text NOT NULL,
    opened_by_type text NOT NULL,
    opened_by_id text NOT NULL,
    opened_at timestamptz NOT NULL,
    closed_by_type text,
    closed_by_id text,
    closed_at timestamptz,
    CONSTRAINT auction_lots_pkey PRIMARY KEY (guild_id, tournament_id, id),
    CONSTRAINT auction_lots_draw_sequence_key
        UNIQUE (guild_id, tournament_id, draw_sequence),
    CONSTRAINT auction_lots_target_key
        UNIQUE (guild_id, tournament_id, id, target_user_id),
    CONSTRAINT auction_lots_tournament_fk
        FOREIGN KEY (guild_id, tournament_id)
        REFERENCES auction_tournaments (guild_id, id) ON DELETE RESTRICT,
    CONSTRAINT auction_lots_target_fk
        FOREIGN KEY (guild_id, tournament_id, target_user_id)
        REFERENCES auction_bid_candidates (guild_id, tournament_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT auction_lots_draw_sequence_check CHECK (draw_sequence > 0),
    CONSTRAINT auction_lots_status_check
        CHECK (status IN ('ACTIVE', 'AWARDED', 'UNSOLD', 'ASSIGNED', 'REVERTED')),
    CONSTRAINT auction_lots_opened_by_type_check
        CHECK (opened_by_type IN ('USER', 'BOT', 'SYSTEM', 'MIGRATION')),
    CONSTRAINT auction_lots_closed_by_type_check
        CHECK (closed_by_type IN ('USER', 'BOT', 'SYSTEM', 'MIGRATION')),
    CONSTRAINT auction_lots_closed_state_check CHECK (
        (status = 'ACTIVE'
            AND closed_by_type IS NULL
            AND closed_by_id IS NULL
            AND closed_at IS NULL)
        OR (status <> 'ACTIVE'
            AND closed_by_type IS NOT NULL
            AND closed_by_id IS NOT NULL
            AND closed_at IS NOT NULL)
    )
);

CREATE UNIQUE INDEX auction_lots_one_active_idx
    ON auction_lots (guild_id, tournament_id)
    WHERE status = 'ACTIVE';

CREATE TABLE auction_actions (
    guild_id text NOT NULL,
    tournament_id uuid NOT NULL,
    id uuid NOT NULL,
    sequence bigint NOT NULL,
    kind text NOT NULL,
    target_user_id text,
    team_id uuid,
    lot_id uuid,
    points integer,
    reverses_action_id uuid,
    reversed_action_kind text,
    actor_type text NOT NULL,
    actor_id text NOT NULL,
    created_at timestamptz NOT NULL,
    CONSTRAINT auction_actions_pkey PRIMARY KEY (guild_id, tournament_id, id),
    CONSTRAINT auction_actions_sequence_key UNIQUE (guild_id, tournament_id, sequence),
    CONSTRAINT auction_actions_id_kind_key UNIQUE (guild_id, tournament_id, id, kind),
    CONSTRAINT auction_actions_team_kind_key
        UNIQUE (guild_id, tournament_id, id, team_id, kind),
    CONSTRAINT auction_actions_target_team_kind_key
        UNIQUE (guild_id, tournament_id, id, team_id, target_user_id, kind),
    CONSTRAINT auction_actions_tournament_fk
        FOREIGN KEY (guild_id, tournament_id)
        REFERENCES auction_tournaments (guild_id, id) ON DELETE RESTRICT,
    CONSTRAINT auction_actions_target_fk
        FOREIGN KEY (guild_id, tournament_id, target_user_id)
        REFERENCES auction_bid_candidates (guild_id, tournament_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT auction_actions_team_fk
        FOREIGN KEY (guild_id, tournament_id, team_id)
        REFERENCES auction_teams (guild_id, tournament_id, id) ON DELETE RESTRICT,
    CONSTRAINT auction_actions_lot_target_fk
        FOREIGN KEY (guild_id, tournament_id, lot_id, target_user_id)
        REFERENCES auction_lots (guild_id, tournament_id, id, target_user_id) ON DELETE RESTRICT,
    CONSTRAINT auction_actions_reversed_action_fk
        FOREIGN KEY (guild_id, tournament_id, reverses_action_id, reversed_action_kind)
        REFERENCES auction_actions (guild_id, tournament_id, id, kind) ON DELETE RESTRICT,
    CONSTRAINT auction_actions_sequence_check CHECK (sequence > 0),
    CONSTRAINT auction_actions_kind_check CHECK (
        kind IN (
            'DRAW_NEXT',
            'MARK_UNSOLD',
            'AWARD_BID',
            'ASSIGN_CURRENT',
            'ASSIGN_UNSOLD',
            'RESTORE_UNSOLD',
            'REVERT_LAST'
        )
    ),
    CONSTRAINT auction_actions_points_check CHECK (points >= 0),
    CONSTRAINT auction_actions_reversed_kind_check CHECK (
        reversed_action_kind IN (
            'DRAW_NEXT',
            'MARK_UNSOLD',
            'AWARD_BID',
            'ASSIGN_CURRENT',
            'ASSIGN_UNSOLD',
            'RESTORE_UNSOLD'
        )
    ),
    CONSTRAINT auction_actions_actor_type_check
        CHECK (actor_type IN ('USER', 'BOT', 'SYSTEM', 'MIGRATION')),
    CONSTRAINT auction_actions_reversal_shape_check CHECK (
        (kind = 'REVERT_LAST'
            AND reverses_action_id IS NOT NULL
            AND reversed_action_kind IS NOT NULL)
        OR (kind <> 'REVERT_LAST'
            AND reverses_action_id IS NULL
            AND reversed_action_kind IS NULL)
    ),
    CONSTRAINT auction_actions_no_self_reversal_check
        CHECK (reverses_action_id IS NULL OR reverses_action_id <> id),
    CONSTRAINT auction_actions_payload_check CHECK (
        (kind = 'REVERT_LAST'
            AND points IS NULL
            AND (lot_id IS NULL OR target_user_id IS NOT NULL))
        OR (target_user_id IS NOT NULL
            AND lot_id IS NOT NULL
            AND (
                (kind IN ('DRAW_NEXT', 'MARK_UNSOLD', 'RESTORE_UNSOLD')
                    AND team_id IS NULL
                    AND points IS NULL)
                OR (kind IN ('ASSIGN_CURRENT', 'ASSIGN_UNSOLD')
                    AND team_id IS NOT NULL
                    AND points IS NULL)
                OR (kind = 'AWARD_BID'
                    AND team_id IS NOT NULL
                    AND points IS NOT NULL)
            ))
    )
);

CREATE UNIQUE INDEX auction_actions_one_reversal_idx
    ON auction_actions (guild_id, tournament_id, reverses_action_id)
    WHERE reverses_action_id IS NOT NULL;

CREATE TABLE auction_team_members (
    guild_id text NOT NULL,
    tournament_id uuid NOT NULL,
    team_id uuid NOT NULL,
    id uuid NOT NULL,
    user_id text NOT NULL,
    roster_slot smallint NOT NULL,
    acquired_via text NOT NULL,
    declared_lane text,
    mmr_at_acquisition double precision,
    snapshot_origin text NOT NULL,
    acquisition_action_id uuid,
    acquisition_action_kind text,
    joined_at timestamptz NOT NULL,
    released_at timestamptz,
    CONSTRAINT auction_team_members_pkey PRIMARY KEY (guild_id, tournament_id, id),
    CONSTRAINT auction_team_members_lineup_key
        UNIQUE (guild_id, tournament_id, team_id, id, user_id),
    CONSTRAINT auction_team_members_team_fk
        FOREIGN KEY (guild_id, tournament_id, team_id)
        REFERENCES auction_teams (guild_id, tournament_id, id) ON DELETE RESTRICT,
    CONSTRAINT auction_team_members_member_fk
        FOREIGN KEY (guild_id, user_id)
        REFERENCES guild_members (guild_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT auction_team_members_acquisition_action_fk
        FOREIGN KEY (
            guild_id,
            tournament_id,
            acquisition_action_id,
            team_id,
            user_id,
            acquisition_action_kind
        )
        REFERENCES auction_actions (
            guild_id,
            tournament_id,
            id,
            team_id,
            target_user_id,
            kind
        ) ON DELETE RESTRICT,
    CONSTRAINT auction_team_members_roster_slot_check CHECK (roster_slot BETWEEN 1 AND 5),
    CONSTRAINT auction_team_members_acquired_via_check
        CHECK (acquired_via IN ('CAPTAIN', 'BID', 'MANUAL')),
    CONSTRAINT auction_team_members_lane_check
        CHECK (declared_lane IN ('TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT')),
    CONSTRAINT auction_team_members_origin_check
        CHECK (snapshot_origin IN ('CAPTURED', 'INFERRED', 'UNAVAILABLE')),
    CONSTRAINT auction_team_members_action_kind_check CHECK (
        acquisition_action_kind IN ('AWARD_BID', 'ASSIGN_CURRENT', 'ASSIGN_UNSOLD')
    ),
    CONSTRAINT auction_team_members_mmr_finite_check CHECK (
        mmr_at_acquisition IS NULL
        OR mmr_at_acquisition NOT IN (
            'NaN'::double precision,
            'Infinity'::double precision,
            '-Infinity'::double precision
        )
    ),
    CONSTRAINT auction_team_members_acquisition_check CHECK (
        (acquired_via = 'CAPTAIN'
            AND acquisition_action_id IS NULL
            AND acquisition_action_kind IS NULL)
        OR (acquired_via = 'BID'
            AND acquisition_action_id IS NOT NULL
            AND acquisition_action_kind = 'AWARD_BID')
        OR (acquired_via = 'MANUAL'
            AND acquisition_action_id IS NOT NULL
            AND acquisition_action_kind IN ('ASSIGN_CURRENT', 'ASSIGN_UNSOLD'))
    ),
    CONSTRAINT auction_team_members_snapshot_check CHECK (
        (snapshot_origin IN ('CAPTURED', 'INFERRED')
            AND declared_lane IS NOT NULL
            AND mmr_at_acquisition IS NOT NULL)
        OR (snapshot_origin = 'UNAVAILABLE'
            AND (declared_lane IS NULL OR mmr_at_acquisition IS NULL))
    )
);

CREATE UNIQUE INDEX auction_team_members_one_active_user_idx
    ON auction_team_members (guild_id, tournament_id, user_id)
    WHERE released_at IS NULL;

CREATE UNIQUE INDEX auction_team_members_one_active_slot_idx
    ON auction_team_members (guild_id, tournament_id, team_id, roster_slot)
    WHERE released_at IS NULL;

CREATE UNIQUE INDEX auction_team_members_one_active_captain_idx
    ON auction_team_members (guild_id, tournament_id, team_id)
    WHERE acquired_via = 'CAPTAIN' AND released_at IS NULL;

CREATE UNIQUE INDEX auction_team_members_acquisition_action_once_idx
    ON auction_team_members (guild_id, tournament_id, acquisition_action_id)
    WHERE acquisition_action_id IS NOT NULL;

CREATE TABLE auction_point_ledger (
    guild_id text NOT NULL,
    tournament_id uuid NOT NULL,
    id uuid NOT NULL,
    team_id uuid NOT NULL,
    point_revision bigint NOT NULL,
    reason text NOT NULL,
    source_action_id uuid,
    source_action_kind text,
    points_before integer NOT NULL,
    points_after integer NOT NULL,
    delta integer NOT NULL,
    reverses_entry_id uuid,
    reversed_reason text,
    actor_type text NOT NULL,
    actor_id text NOT NULL,
    created_at timestamptz NOT NULL,
    CONSTRAINT auction_point_ledger_pkey PRIMARY KEY (guild_id, tournament_id, id),
    CONSTRAINT auction_point_ledger_team_entry_key
        UNIQUE (guild_id, tournament_id, id, team_id),
    CONSTRAINT auction_point_ledger_team_reason_key
        UNIQUE (guild_id, tournament_id, id, team_id, reason),
    CONSTRAINT auction_point_ledger_revision_key
        UNIQUE (guild_id, tournament_id, team_id, point_revision),
    CONSTRAINT auction_point_ledger_team_fk
        FOREIGN KEY (guild_id, tournament_id, team_id)
        REFERENCES auction_teams (guild_id, tournament_id, id) ON DELETE RESTRICT,
    CONSTRAINT auction_point_ledger_source_action_fk
        FOREIGN KEY (guild_id, tournament_id, source_action_id, team_id, source_action_kind)
        REFERENCES auction_actions (guild_id, tournament_id, id, team_id, kind)
        ON DELETE RESTRICT,
    CONSTRAINT auction_point_ledger_reversed_entry_fk
        FOREIGN KEY (guild_id, tournament_id, reverses_entry_id, team_id, reversed_reason)
        REFERENCES auction_point_ledger (guild_id, tournament_id, id, team_id, reason)
        ON DELETE RESTRICT,
    CONSTRAINT auction_point_ledger_point_revision_check CHECK (point_revision >= 0),
    CONSTRAINT auction_point_ledger_reason_check
        CHECK (reason IN ('INITIAL', 'BID_DEBIT', 'REVERSAL')),
    CONSTRAINT auction_point_ledger_source_action_kind_check
        CHECK (source_action_kind IN ('AWARD_BID', 'REVERT_LAST')),
    CONSTRAINT auction_point_ledger_points_before_check CHECK (points_before >= 0),
    CONSTRAINT auction_point_ledger_points_after_check CHECK (points_after >= 0),
    CONSTRAINT auction_point_ledger_reversed_reason_check
        CHECK (reversed_reason = 'BID_DEBIT'),
    CONSTRAINT auction_point_ledger_actor_type_check
        CHECK (actor_type IN ('USER', 'BOT', 'SYSTEM', 'MIGRATION')),
    CONSTRAINT auction_point_ledger_source_shape_check CHECK (
        (reason = 'INITIAL'
            AND source_action_id IS NULL
            AND source_action_kind IS NULL
            AND point_revision = 0)
        OR (reason = 'BID_DEBIT'
            AND source_action_id IS NOT NULL
            AND source_action_kind = 'AWARD_BID'
            AND point_revision > 0)
        OR (reason = 'REVERSAL'
            AND source_action_id IS NOT NULL
            AND source_action_kind = 'REVERT_LAST'
            AND point_revision > 0)
    ),
    CONSTRAINT auction_point_ledger_reversal_shape_check CHECK (
        (reason = 'REVERSAL'
            AND reverses_entry_id IS NOT NULL
            AND reversed_reason = 'BID_DEBIT')
        OR (reason <> 'REVERSAL'
            AND reverses_entry_id IS NULL
            AND reversed_reason IS NULL)
    ),
    CONSTRAINT auction_point_ledger_no_self_reversal_check
        CHECK (reverses_entry_id IS NULL OR reverses_entry_id <> id),
    CONSTRAINT auction_point_ledger_delta_check
        CHECK (points_after = points_before + delta)
);

CREATE UNIQUE INDEX auction_point_ledger_one_initial_idx
    ON auction_point_ledger (guild_id, tournament_id, team_id)
    WHERE reason = 'INITIAL';

CREATE UNIQUE INDEX auction_point_ledger_source_action_once_idx
    ON auction_point_ledger (guild_id, tournament_id, source_action_id, team_id)
    WHERE source_action_id IS NOT NULL;

CREATE UNIQUE INDEX auction_point_ledger_one_reversal_idx
    ON auction_point_ledger (guild_id, tournament_id, reverses_entry_id)
    WHERE reverses_entry_id IS NOT NULL;

CREATE TABLE auction_bracket_slots (
    guild_id text NOT NULL,
    tournament_id uuid NOT NULL,
    slot_key text NOT NULL,
    team_id uuid,
    updated_by text,
    updated_at timestamptz NOT NULL,
    CONSTRAINT auction_bracket_slots_pkey PRIMARY KEY (guild_id, tournament_id, slot_key),
    CONSTRAINT auction_bracket_slots_tournament_fk
        FOREIGN KEY (guild_id, tournament_id)
        REFERENCES auction_tournaments (guild_id, id) ON DELETE RESTRICT,
    CONSTRAINT auction_bracket_slots_team_fk
        FOREIGN KEY (guild_id, tournament_id, team_id)
        REFERENCES auction_teams (guild_id, tournament_id, id) ON DELETE RESTRICT,
    CONSTRAINT auction_bracket_slots_updated_by_fk
        FOREIGN KEY (guild_id, updated_by)
        REFERENCES guild_members (guild_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT auction_bracket_slots_slot_key_check
        CHECK (slot_key IN ('A_0', 'A_1', 'B_0', 'B_1'))
);

CREATE UNIQUE INDEX auction_bracket_slots_one_team_idx
    ON auction_bracket_slots (guild_id, tournament_id, team_id)
    WHERE team_id IS NOT NULL;

CREATE TABLE auction_matches (
    guild_id text NOT NULL,
    tournament_id uuid NOT NULL,
    id uuid NOT NULL,
    season_id uuid NOT NULL,
    match_key text NOT NULL,
    best_of smallint NOT NULL,
    left_team_id uuid,
    right_team_id uuid,
    status text NOT NULL,
    winner_team_id uuid,
    revision bigint NOT NULL,
    lineups_locked_at timestamptz,
    lineups_locked_by text,
    started_at timestamptz,
    ended_at timestamptz,
    updated_by text,
    updated_at timestamptz NOT NULL,
    deleted_at timestamptz,
    CONSTRAINT auction_matches_pkey PRIMARY KEY (guild_id, tournament_id, id),
    CONSTRAINT auction_matches_guild_id_key UNIQUE (guild_id, id),
    CONSTRAINT auction_matches_season_id_key UNIQUE (guild_id, season_id, id),
    CONSTRAINT auction_matches_tournament_fk
        FOREIGN KEY (guild_id, season_id, tournament_id)
        REFERENCES auction_tournaments (guild_id, season_id, id) ON DELETE RESTRICT,
    CONSTRAINT auction_matches_left_team_fk
        FOREIGN KEY (guild_id, tournament_id, left_team_id)
        REFERENCES auction_teams (guild_id, tournament_id, id) ON DELETE RESTRICT,
    CONSTRAINT auction_matches_right_team_fk
        FOREIGN KEY (guild_id, tournament_id, right_team_id)
        REFERENCES auction_teams (guild_id, tournament_id, id) ON DELETE RESTRICT,
    CONSTRAINT auction_matches_winner_team_fk
        FOREIGN KEY (guild_id, tournament_id, winner_team_id)
        REFERENCES auction_teams (guild_id, tournament_id, id) ON DELETE RESTRICT,
    CONSTRAINT auction_matches_lineups_locked_by_fk
        FOREIGN KEY (guild_id, lineups_locked_by)
        REFERENCES guild_members (guild_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT auction_matches_updated_by_fk
        FOREIGN KEY (guild_id, updated_by)
        REFERENCES guild_members (guild_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT auction_matches_match_key_check
        CHECK (match_key IN ('SEMIFINAL_A', 'SEMIFINAL_B', 'FINAL')),
    CONSTRAINT auction_matches_best_of_check CHECK (best_of IN (1, 3, 5)),
    CONSTRAINT auction_matches_status_check CHECK (
        status IN ('LINEUP_PENDING', 'READY', 'IN_PROGRESS', 'COMPLETED', 'CANCELLED')
    ),
    CONSTRAINT auction_matches_revision_check CHECK (revision >= 0),
    CONSTRAINT auction_matches_distinct_teams_check CHECK (
        left_team_id IS NULL OR right_team_id IS NULL OR left_team_id <> right_team_id
    ),
    CONSTRAINT auction_matches_winner_check CHECK (
        winner_team_id IS NULL
        OR (left_team_id IS NOT NULL
            AND right_team_id IS NOT NULL
            AND (winner_team_id = left_team_id OR winner_team_id = right_team_id))
    ),
    CONSTRAINT auction_matches_terminal_state_check CHECK (
        (status IN ('LINEUP_PENDING', 'READY', 'IN_PROGRESS')
            AND winner_team_id IS NULL
            AND ended_at IS NULL)
        OR (status = 'COMPLETED'
            AND winner_team_id IS NOT NULL
            AND ended_at IS NOT NULL)
        OR (status = 'CANCELLED'
            AND winner_team_id IS NULL
            AND ended_at IS NOT NULL)
    )
);

CREATE UNIQUE INDEX auction_matches_one_active_key_idx
    ON auction_matches (guild_id, tournament_id, match_key)
    WHERE status <> 'CANCELLED' AND deleted_at IS NULL;

CREATE TABLE auction_match_lineups (
    guild_id text NOT NULL,
    tournament_id uuid NOT NULL,
    match_id uuid NOT NULL,
    team_id uuid NOT NULL,
    lane text NOT NULL,
    team_member_id uuid NOT NULL,
    user_id text NOT NULL,
    CONSTRAINT auction_match_lineups_pkey
        PRIMARY KEY (guild_id, tournament_id, match_id, team_id, lane),
    CONSTRAINT auction_match_lineups_user_key
        UNIQUE (guild_id, tournament_id, match_id, user_id),
    CONSTRAINT auction_match_lineups_match_fk
        FOREIGN KEY (guild_id, tournament_id, match_id)
        REFERENCES auction_matches (guild_id, tournament_id, id) ON DELETE RESTRICT,
    CONSTRAINT auction_match_lineups_team_member_fk
        FOREIGN KEY (guild_id, tournament_id, team_id, team_member_id, user_id)
        REFERENCES auction_team_members (guild_id, tournament_id, team_id, id, user_id)
        ON DELETE RESTRICT,
    CONSTRAINT auction_match_lineups_lane_check
        CHECK (lane IN ('TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT'))
);
