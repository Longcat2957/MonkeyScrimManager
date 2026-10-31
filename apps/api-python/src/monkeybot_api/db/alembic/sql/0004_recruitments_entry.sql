CREATE TABLE recruitments (
    guild_id text NOT NULL,
    id uuid NOT NULL,
    season_id uuid NOT NULL,
    display_number bigint NOT NULL,
    title text NOT NULL,
    target_count smallint NOT NULL DEFAULT 10,
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
    CONSTRAINT recruitments_pkey PRIMARY KEY (guild_id, id),
    CONSTRAINT recruitments_display_number_key UNIQUE (guild_id, display_number),
    CONSTRAINT recruitments_season_id_key UNIQUE (guild_id, season_id, id),
    CONSTRAINT recruitments_season_fk
        FOREIGN KEY (guild_id, season_id)
        REFERENCES seasons (guild_id, id) ON DELETE RESTRICT,
    CONSTRAINT recruitments_created_by_fk
        FOREIGN KEY (guild_id, created_by)
        REFERENCES guild_members (guild_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT recruitments_updated_by_fk
        FOREIGN KEY (guild_id, updated_by)
        REFERENCES guild_members (guild_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT recruitments_channel_id_check
        CHECK (public.is_discord_snowflake(channel_id)),
    CONSTRAINT recruitments_message_id_check
        CHECK (public.is_discord_snowflake(message_id)),
    CONSTRAINT recruitments_display_number_check CHECK (display_number > 0),
    CONSTRAINT recruitments_target_count_check CHECK (target_count = 10),
    CONSTRAINT recruitments_status_check
        CHECK (status IN ('OPEN', 'CLOSED', 'CONVERTED', 'CANCELLED')),
    CONSTRAINT recruitments_revision_check CHECK (revision >= 0)
);

CREATE TABLE recruitment_participants (
    guild_id text NOT NULL,
    recruitment_id uuid NOT NULL,
    user_id text NOT NULL,
    status text NOT NULL,
    joined_at timestamptz NOT NULL,
    withdrawn_at timestamptz,
    CONSTRAINT recruitment_participants_pkey PRIMARY KEY (guild_id, recruitment_id, user_id),
    CONSTRAINT recruitment_participants_recruitment_fk
        FOREIGN KEY (guild_id, recruitment_id)
        REFERENCES recruitments (guild_id, id) ON DELETE RESTRICT,
    CONSTRAINT recruitment_participants_member_fk
        FOREIGN KEY (guild_id, user_id)
        REFERENCES guild_members (guild_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT recruitment_participants_status_check
        CHECK (status IN ('JOINED', 'WITHDRAWN'))
);

CREATE TABLE recruitment_preferred_lanes (
    guild_id text NOT NULL,
    recruitment_id uuid NOT NULL,
    user_id text NOT NULL,
    priority smallint NOT NULL,
    lane text NOT NULL,
    CONSTRAINT recruitment_preferred_lanes_pkey
        PRIMARY KEY (guild_id, recruitment_id, user_id, priority),
    CONSTRAINT recruitment_preferred_lanes_lane_key
        UNIQUE (guild_id, recruitment_id, user_id, lane),
    CONSTRAINT recruitment_preferred_lanes_participant_fk
        FOREIGN KEY (guild_id, recruitment_id, user_id)
        REFERENCES recruitment_participants (guild_id, recruitment_id, user_id) ON DELETE CASCADE,
    CONSTRAINT recruitment_preferred_lanes_priority_check CHECK (priority BETWEEN 1 AND 5),
    CONSTRAINT recruitment_preferred_lanes_lane_check
        CHECK (lane IN ('TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT'))
);

CREATE TABLE recruitment_entry_drafts (
    guild_id text NOT NULL,
    recruitment_id uuid NOT NULL,
    revision bigint NOT NULL,
    updated_by text,
    updated_at timestamptz NOT NULL,
    CONSTRAINT recruitment_entry_drafts_pkey PRIMARY KEY (guild_id, recruitment_id),
    CONSTRAINT recruitment_entry_drafts_recruitment_fk
        FOREIGN KEY (guild_id, recruitment_id)
        REFERENCES recruitments (guild_id, id) ON DELETE RESTRICT,
    CONSTRAINT recruitment_entry_drafts_updated_by_fk
        FOREIGN KEY (guild_id, updated_by)
        REFERENCES guild_members (guild_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT recruitment_entry_drafts_revision_check CHECK (revision >= 0)
);

CREATE TABLE recruitment_entry_assignments (
    guild_id text NOT NULL,
    recruitment_id uuid NOT NULL,
    user_id text NOT NULL,
    team text NOT NULL,
    lane text NOT NULL,
    CONSTRAINT recruitment_entry_assignments_pkey
        PRIMARY KEY (guild_id, recruitment_id, user_id),
    CONSTRAINT recruitment_entry_assignments_slot_key
        UNIQUE (guild_id, recruitment_id, team, lane),
    CONSTRAINT recruitment_entry_assignments_draft_fk
        FOREIGN KEY (guild_id, recruitment_id)
        REFERENCES recruitment_entry_drafts (guild_id, recruitment_id) ON DELETE CASCADE,
    CONSTRAINT recruitment_entry_assignments_participant_fk
        FOREIGN KEY (guild_id, recruitment_id, user_id)
        REFERENCES recruitment_participants (guild_id, recruitment_id, user_id) ON DELETE RESTRICT,
    CONSTRAINT recruitment_entry_assignments_team_check
        CHECK (team IN ('TEAM_1', 'TEAM_2')),
    CONSTRAINT recruitment_entry_assignments_lane_check
        CHECK (lane IN ('TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT'))
);
