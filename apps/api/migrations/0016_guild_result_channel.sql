ALTER TABLE guild_config
    ADD COLUMN result_channel_id text,
    ADD CONSTRAINT guild_config_result_channel_id_check
        CHECK (public.is_discord_snowflake(result_channel_id));
