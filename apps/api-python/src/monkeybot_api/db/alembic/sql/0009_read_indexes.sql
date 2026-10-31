CREATE INDEX guild_members_status_activity_idx
    ON guild_members (guild_id, status, last_active_at DESC);

CREATE INDEX guild_members_display_name_idx
    ON guild_members (guild_id, lower(display_name));

CREATE INDEX guild_members_user_status_idx
    ON guild_members (user_id, status, guild_id);

CREATE INDEX riot_accounts_user_created_idx
    ON riot_accounts (user_id, created_at);

CREATE INDEX oauth_login_attempts_expires_idx
    ON oauth_login_attempts (expires_at);

CREATE INDEX user_sessions_active_user_idx
    ON user_sessions (user_id, created_at DESC)
    WHERE revoked_at IS NULL;

CREATE INDEX user_sessions_expires_idx
    ON user_sessions (expires_at);

CREATE INDEX idempotency_records_expires_idx
    ON idempotency_records (expires_at);

CREATE INDEX recruitments_status_updated_idx
    ON recruitments (guild_id, status, updated_at DESC);

CREATE INDEX recruitments_season_created_idx
    ON recruitments (guild_id, season_id, created_at DESC);

CREATE INDEX auction_recruitments_status_updated_idx
    ON auction_recruitments (guild_id, status, updated_at DESC);

CREATE INDEX auction_recruitments_season_created_idx
    ON auction_recruitments (guild_id, season_id, created_at DESC);

CREATE INDEX series_status_updated_idx
    ON series (guild_id, status, updated_at DESC)
    WHERE deleted_at IS NULL;

CREATE INDEX series_season_started_idx
    ON series (guild_id, season_id, started_at DESC)
    WHERE deleted_at IS NULL;

CREATE INDEX auction_tournaments_status_updated_idx
    ON auction_tournaments (guild_id, status, updated_at DESC)
    WHERE deleted_at IS NULL;

CREATE INDEX auction_tournaments_season_started_idx
    ON auction_tournaments (guild_id, season_id, started_at DESC)
    WHERE deleted_at IS NULL;

CREATE INDEX recruitment_participants_user_joined_idx
    ON recruitment_participants (guild_id, user_id, joined_at DESC);

CREATE INDEX auction_recruitment_participants_user_joined_idx
    ON auction_recruitment_participants (guild_id, user_id, joined_at DESC);

CREATE INDEX series_participants_user_series_idx
    ON series_participants (guild_id, user_id, series_id);

CREATE INDEX auction_team_members_user_tournament_idx
    ON auction_team_members (guild_id, user_id, tournament_id);

CREATE INDEX auction_bid_candidates_state_queue_idx
    ON auction_bid_candidates (guild_id, tournament_id, state, queue_position);

CREATE INDEX auction_point_ledger_team_created_idx
    ON auction_point_ledger (guild_id, tournament_id, team_id, created_at DESC);

CREATE INDEX auction_matches_status_updated_idx
    ON auction_matches (guild_id, tournament_id, status, updated_at DESC)
    WHERE deleted_at IS NULL;

CREATE INDEX games_season_confirmed_idx
    ON games (guild_id, season_id, confirmed_at DESC)
    WHERE status = 'CONFIRMED';

CREATE INDEX game_participants_user_game_idx
    ON game_participants (guild_id, user_id, game_id);

CREATE INDEX game_participants_user_lane_game_idx
    ON game_participants (guild_id, user_id, lane, game_id);

CREATE INDEX game_participants_champion_game_idx
    ON game_participants (guild_id, champion_id, game_id)
    WHERE champion_id IS NOT NULL;

CREATE INDEX mmr_ledger_entries_user_history_idx
    ON mmr_ledger_entries (
        guild_id,
        season_id,
        user_id,
        lane,
        created_at DESC,
        id DESC
    );

CREATE INDEX mmr_ledger_entries_game_idx
    ON mmr_ledger_entries (guild_id, game_id)
    WHERE game_id IS NOT NULL;

CREATE INDEX user_lane_mmr_ranking_idx
    ON user_lane_mmr (guild_id, season_id, lane, mmr DESC, user_id);

CREATE INDEX admin_audit_log_created_idx
    ON admin_audit_log (guild_id, created_at DESC, id DESC);

CREATE INDEX admin_audit_log_action_created_idx
    ON admin_audit_log (guild_id, action, created_at DESC);

CREATE INDEX admin_audit_log_principal_created_idx
    ON admin_audit_log (guild_id, principal_id, created_at DESC);

CREATE INDEX outbox_events_available_idx
    ON outbox_events (available_at, id)
    WHERE published_at IS NULL;

CREATE INDEX outbox_events_lease_idx
    ON outbox_events (lease_until)
    WHERE published_at IS NULL AND lease_until IS NOT NULL;

CREATE INDEX outbox_events_aggregate_revision_idx
    ON outbox_events (guild_id, aggregate_type, aggregate_id, aggregate_revision);

CREATE INDEX bot_notification_outbox_available_idx
    ON bot_notification_outbox (available_at, id)
    WHERE delivered_at IS NULL AND failed_at IS NULL;

CREATE INDEX bot_notification_outbox_lease_idx
    ON bot_notification_outbox (lease_until)
    WHERE delivered_at IS NULL AND failed_at IS NULL AND lease_until IS NOT NULL;

-- Non-partial FK support for historical rows that are excluded from live/read indexes.
CREATE INDEX user_sessions_rotated_from_idx
    ON user_sessions (rotated_from_id)
    WHERE rotated_from_id IS NOT NULL;

CREATE INDEX user_guild_preferences_membership_idx
    ON user_guild_preferences (default_guild_id, user_id);

CREATE INDEX series_source_recruitment_fk_idx
    ON series (guild_id, season_id, source_recruitment_id);

CREATE INDEX auction_tournaments_source_recruitment_fk_idx
    ON auction_tournaments (guild_id, season_id, source_recruitment_id);

CREATE INDEX game_inputs_series_fk_idx
    ON game_inputs (guild_id, series_id);

CREATE INDEX game_inputs_auction_match_fk_idx
    ON game_inputs (guild_id, auction_match_id);

CREATE INDEX games_series_fk_idx
    ON games (guild_id, season_id, series_id);

CREATE INDEX games_auction_match_fk_idx
    ON games (guild_id, season_id, auction_match_id);

CREATE INDEX game_input_picks_member_fk_idx
    ON game_input_picks (guild_id, user_id, game_input_id);

CREATE INDEX user_lane_mmr_member_fk_idx
    ON user_lane_mmr (guild_id, user_id, season_id, lane);

CREATE INDEX mmr_ledger_entries_opponent_member_fk_idx
    ON mmr_ledger_entries (guild_id, opponent_user_id);

CREATE INDEX mmr_ledger_entries_game_user_fk_idx
    ON mmr_ledger_entries (guild_id, game_id, user_id);

CREATE INDEX mmr_ledger_entries_game_opponent_fk_idx
    ON mmr_ledger_entries (guild_id, game_id, opponent_user_id);

CREATE INDEX guild_config_updated_by_fk_idx
    ON guild_config (guild_id, updated_by);

CREATE INDEX guild_admin_state_updated_by_fk_idx
    ON guild_admin_state (guild_id, updated_by);

CREATE INDEX seasons_created_by_fk_idx
    ON seasons (guild_id, created_by);

CREATE INDEX recruitments_created_by_fk_idx
    ON recruitments (guild_id, created_by);

CREATE INDEX recruitments_updated_by_fk_idx
    ON recruitments (guild_id, updated_by);

CREATE INDEX recruitment_entry_drafts_updated_by_fk_idx
    ON recruitment_entry_drafts (guild_id, updated_by);

CREATE INDEX series_created_by_fk_idx
    ON series (guild_id, created_by);

CREATE INDEX series_updated_by_fk_idx
    ON series (guild_id, updated_by);

CREATE INDEX auction_recruitments_created_by_fk_idx
    ON auction_recruitments (guild_id, created_by);

CREATE INDEX auction_recruitments_updated_by_fk_idx
    ON auction_recruitments (guild_id, updated_by);

CREATE INDEX auction_setup_drafts_updated_by_fk_idx
    ON auction_setup_drafts (guild_id, updated_by);

CREATE INDEX auction_tournaments_bracket_locked_by_fk_idx
    ON auction_tournaments (guild_id, bracket_locked_by);

CREATE INDEX auction_tournaments_champion_team_fk_idx
    ON auction_tournaments (guild_id, id, champion_team_id);

CREATE INDEX auction_tournaments_created_by_fk_idx
    ON auction_tournaments (guild_id, created_by);

CREATE INDEX auction_tournaments_updated_by_fk_idx
    ON auction_tournaments (guild_id, updated_by);

CREATE INDEX auction_bid_candidates_member_fk_idx
    ON auction_bid_candidates (guild_id, user_id);

CREATE INDEX auction_lots_target_fk_idx
    ON auction_lots (guild_id, tournament_id, target_user_id);

CREATE INDEX auction_actions_lot_target_fk_idx
    ON auction_actions (guild_id, tournament_id, lot_id, target_user_id);

CREATE INDEX auction_actions_reversed_action_fk_idx
    ON auction_actions (
        guild_id,
        tournament_id,
        reverses_action_id,
        reversed_action_kind
    );

CREATE INDEX auction_actions_target_fk_idx
    ON auction_actions (guild_id, tournament_id, target_user_id);

CREATE INDEX auction_actions_team_fk_idx
    ON auction_actions (guild_id, tournament_id, team_id);

CREATE INDEX auction_team_members_acquisition_action_fk_idx
    ON auction_team_members (
        guild_id,
        tournament_id,
        acquisition_action_id,
        team_id,
        user_id,
        acquisition_action_kind
    );

CREATE INDEX auction_point_ledger_reversed_entry_fk_idx
    ON auction_point_ledger (
        guild_id,
        tournament_id,
        reverses_entry_id,
        team_id,
        reversed_reason
    );

CREATE INDEX auction_point_ledger_source_action_fk_idx
    ON auction_point_ledger (
        guild_id,
        tournament_id,
        source_action_id,
        team_id,
        source_action_kind
    );

CREATE INDEX auction_bracket_slots_team_fk_idx
    ON auction_bracket_slots (guild_id, tournament_id, team_id);

CREATE INDEX auction_bracket_slots_updated_by_fk_idx
    ON auction_bracket_slots (guild_id, updated_by);

CREATE INDEX auction_matches_left_team_fk_idx
    ON auction_matches (guild_id, tournament_id, left_team_id);

CREATE INDEX auction_matches_lineups_locked_by_fk_idx
    ON auction_matches (guild_id, lineups_locked_by);

CREATE INDEX auction_matches_right_team_fk_idx
    ON auction_matches (guild_id, tournament_id, right_team_id);

CREATE INDEX auction_matches_tournament_fk_idx
    ON auction_matches (guild_id, season_id, tournament_id);

CREATE INDEX auction_matches_updated_by_fk_idx
    ON auction_matches (guild_id, updated_by);

CREATE INDEX auction_matches_winner_team_fk_idx
    ON auction_matches (guild_id, tournament_id, winner_team_id);

CREATE INDEX auction_match_lineups_team_member_fk_idx
    ON auction_match_lineups (
        guild_id,
        tournament_id,
        team_id,
        team_member_id,
        user_id
    );

CREATE INDEX game_inputs_updated_by_fk_idx
    ON game_inputs (guild_id, updated_by);

CREATE INDEX games_auction_input_fk_idx
    ON games (
        guild_id,
        source_input_id,
        auction_match_id,
        game_number,
        side_preset,
        winning_side
    );

CREATE INDEX games_series_input_fk_idx
    ON games (
        guild_id,
        source_input_id,
        series_id,
        game_number,
        side_preset,
        winning_side
    );

CREATE INDEX mmr_ledger_entries_game_fk_idx
    ON mmr_ledger_entries (guild_id, season_id, game_id);

CREATE INDEX mmr_ledger_entries_member_fk_idx
    ON mmr_ledger_entries (guild_id, user_id);

CREATE INDEX mmr_ledger_entries_reversal_fk_idx
    ON mmr_ledger_entries (
        guild_id,
        reverses_entry_id,
        season_id,
        user_id,
        lane,
        reversed_source_type
    );

CREATE INDEX admin_audit_log_checkpoint_fk_idx
    ON admin_audit_log (guild_id, checkpoint_id);
