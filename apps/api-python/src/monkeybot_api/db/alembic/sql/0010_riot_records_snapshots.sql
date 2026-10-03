CREATE TABLE riot_records_snapshots (
    platform text NOT NULL,
    puuid text NOT NULL,
    routing_region text NOT NULL,
    game_name text NOT NULL,
    tag_line text NOT NULL,
    normalized_game_name text NOT NULL,
    normalized_tag_line text NOT NULL,
    schema_version integer NOT NULL,
    snapshot jsonb NOT NULL,
    refreshed_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL,
    CONSTRAINT riot_records_snapshots_pkey PRIMARY KEY (platform, puuid),
    CONSTRAINT riot_records_snapshots_platform_check CHECK (
        platform IN (
            'BR1', 'EUN1', 'EUW1', 'JP1', 'KR', 'LA1', 'LA2', 'NA1',
            'OC1', 'PH2', 'RU', 'SG2', 'TH2', 'TR1', 'TW2', 'VN2'
        )
    ),
    CONSTRAINT riot_records_snapshots_routing_region_check CHECK (
        routing_region IN ('AMERICAS', 'ASIA', 'EUROPE', 'SEA')
    ),
    CONSTRAINT riot_records_snapshots_identity_check CHECK (
        char_length(puuid) BETWEEN 1 AND 256
        AND char_length(game_name) BETWEEN 1 AND 64
        AND char_length(tag_line) BETWEEN 1 AND 16
        AND char_length(normalized_game_name) BETWEEN 1 AND 64
        AND char_length(normalized_tag_line) BETWEEN 1 AND 16
    ),
    CONSTRAINT riot_records_snapshots_schema_version_check CHECK (schema_version > 0),
    CONSTRAINT riot_records_snapshots_snapshot_check CHECK (jsonb_typeof(snapshot) = 'object')
);

CREATE TABLE riot_records_lookup_aliases (
    platform text NOT NULL,
    normalized_game_name text NOT NULL,
    normalized_tag_line text NOT NULL,
    snapshot_platform text NOT NULL,
    snapshot_puuid text NOT NULL,
    resolved_at timestamptz NOT NULL,
    CONSTRAINT riot_records_lookup_aliases_pkey PRIMARY KEY (
        platform,
        normalized_game_name,
        normalized_tag_line
    ),
    CONSTRAINT riot_records_lookup_aliases_snapshot_fk FOREIGN KEY (
        snapshot_platform,
        snapshot_puuid
    ) REFERENCES riot_records_snapshots (platform, puuid) ON DELETE CASCADE,
    CONSTRAINT riot_records_lookup_aliases_platform_check CHECK (
        platform IN (
            'BR1', 'EUN1', 'EUW1', 'JP1', 'KR', 'LA1', 'LA2', 'NA1',
            'OC1', 'PH2', 'RU', 'SG2', 'TH2', 'TR1', 'TW2', 'VN2'
        )
        AND platform = snapshot_platform
    ),
    CONSTRAINT riot_records_lookup_aliases_identity_check CHECK (
        char_length(normalized_game_name) BETWEEN 1 AND 64
        AND char_length(normalized_tag_line) BETWEEN 1 AND 16
    )
);

CREATE INDEX riot_records_lookup_aliases_snapshot_idx
    ON riot_records_lookup_aliases (snapshot_platform, snapshot_puuid);

CREATE TABLE riot_records_recent_pages (
    platform text NOT NULL,
    puuid text NOT NULL,
    page_offset integer NOT NULL,
    requested_count integer NOT NULL,
    schema_version integer NOT NULL,
    page jsonb NOT NULL,
    fetched_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL,
    CONSTRAINT riot_records_recent_pages_pkey PRIMARY KEY (
        platform,
        puuid,
        page_offset,
        requested_count
    ),
    CONSTRAINT riot_records_recent_pages_snapshot_fk FOREIGN KEY (platform, puuid)
        REFERENCES riot_records_snapshots (platform, puuid) ON DELETE CASCADE,
    CONSTRAINT riot_records_recent_pages_page_key_check CHECK (
        page_offset >= 0 AND requested_count > 0
    ),
    CONSTRAINT riot_records_recent_pages_schema_version_check CHECK (schema_version > 0),
    CONSTRAINT riot_records_recent_pages_page_check CHECK (jsonb_typeof(page) = 'object')
);

CREATE TABLE riot_records_refresh_gates (
    platform text NOT NULL,
    normalized_game_name text NOT NULL,
    normalized_tag_line text NOT NULL,
    lease_token uuid,
    lease_acquired_at timestamptz,
    lease_until timestamptz,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL,
    CONSTRAINT riot_records_refresh_gates_pkey PRIMARY KEY (
        platform,
        normalized_game_name,
        normalized_tag_line
    ),
    CONSTRAINT riot_records_refresh_gates_platform_check CHECK (
        platform IN (
            'BR1', 'EUN1', 'EUW1', 'JP1', 'KR', 'LA1', 'LA2', 'NA1',
            'OC1', 'PH2', 'RU', 'SG2', 'TH2', 'TR1', 'TW2', 'VN2'
        )
    ),
    CONSTRAINT riot_records_refresh_gates_identity_check CHECK (
        char_length(normalized_game_name) BETWEEN 1 AND 64
        AND char_length(normalized_tag_line) BETWEEN 1 AND 16
    ),
    CONSTRAINT riot_records_refresh_gates_lease_check CHECK (
        num_nonnulls(lease_token, lease_acquired_at, lease_until) IN (0, 3)
        AND (lease_until IS NULL OR lease_until > lease_acquired_at)
    )
);
