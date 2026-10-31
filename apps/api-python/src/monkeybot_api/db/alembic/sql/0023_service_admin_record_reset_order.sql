DROP FUNCTION public.service_admin_reset_append_only_records(text);

CREATE FUNCTION public.service_admin_reset_guild_records(target_guild_id text)
RETURNS bigint
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $function$
DECLARE
    deleted_rows bigint := 0;
    affected_rows bigint;
BEGIN
    IF target_guild_id IS NULL OR NOT public.is_discord_snowflake(target_guild_id) THEN
        RAISE EXCEPTION 'invalid guild id for service-admin reset'
            USING ERRCODE = '22023';
    END IF;

    PERFORM 1
    FROM public.guilds
    WHERE guild_id = target_guild_id
    FOR UPDATE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'guild not found for service-admin reset'
            USING ERRCODE = 'P0002';
    END IF;

    UPDATE public.auction_tournaments
    SET champion_team_id = NULL
    WHERE guild_id = target_guild_id
      AND champion_team_id IS NOT NULL;

    DELETE FROM public.admin_audit_log WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.revert_checkpoints WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.bot_notification_outbox WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.outbox_events WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.guild_data_imports WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.guild_full_data_imports WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.idempotency_records WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.mmr_ledger_entries WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.game_participant_stats WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.game_bans WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.game_participants WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.games WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.game_input_bans WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.game_input_picks WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.game_inputs WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.auction_match_lineups WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.auction_matches WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.auction_bracket_slots WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.auction_point_ledger WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.auction_team_members WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.auction_actions WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.auction_lots WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.auction_bid_candidates WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.auction_teams WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.auction_tournaments WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.auction_setup_teams WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.auction_setup_drafts WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.auction_recruitment_participants WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.auction_recruitments WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.series_participants WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.series WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.recruitment_entry_assignments WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.recruitment_preferred_lanes WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.recruitment_entry_drafts WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.recruitment_participants WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.recruitments WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.user_lane_mmr WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.guild_counters WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    DELETE FROM public.seasons WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS affected_rows = ROW_COUNT;
    deleted_rows := deleted_rows + affected_rows;

    RETURN deleted_rows;
END;
$function$;

REVOKE ALL PRIVILEGES
    ON FUNCTION public.service_admin_reset_guild_records(text)
    FROM PUBLIC;

COMMENT ON FUNCTION public.service_admin_reset_guild_records(text) IS
    'Guild-scoped ordered purge capability used only by the guarded service-admin test reset route';
