CREATE FUNCTION public.service_admin_reset_append_only_records(target_guild_id text)
RETURNS TABLE (
    admin_audit_log_rows bigint,
    mmr_ledger_entries_rows bigint,
    auction_actions_rows bigint,
    auction_point_ledger_rows bigint
)
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $function$
BEGIN
    IF target_guild_id IS NULL OR NOT public.is_discord_snowflake(target_guild_id) THEN
        RAISE EXCEPTION 'invalid guild id for service-admin reset'
            USING ERRCODE = '22023';
    END IF;

    DELETE FROM public.admin_audit_log WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS admin_audit_log_rows = ROW_COUNT;

    DELETE FROM public.mmr_ledger_entries WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS mmr_ledger_entries_rows = ROW_COUNT;

    DELETE FROM public.auction_actions WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS auction_actions_rows = ROW_COUNT;

    DELETE FROM public.auction_point_ledger WHERE guild_id = target_guild_id;
    GET DIAGNOSTICS auction_point_ledger_rows = ROW_COUNT;

    RETURN NEXT;
END;
$function$;

REVOKE ALL PRIVILEGES
    ON FUNCTION public.service_admin_reset_append_only_records(text)
    FROM PUBLIC;

COMMENT ON FUNCTION public.service_admin_reset_append_only_records(text) IS
    'Narrow append-only purge capability used only by the guarded service-admin test reset route';
