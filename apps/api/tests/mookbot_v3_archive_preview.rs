use std::{env, error::Error, fs, time::Duration};

use monkeybot_api::{
    domain::{GuildId, UserId},
    infrastructure::postgres::MIGRATOR,
    repository::guild_full_data_transfer,
};
use serde_json::Value;
use sqlx::{PgPool, postgres::PgPoolOptions};
use testcontainers::{
    ContainerAsync, GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use time::OffsetDateTime;
use uuid::Uuid;

const POSTGRES_IMAGE: &str = "postgres";
const POSTGRES_TAG: &str = "18.4-alpine3.23";
const POSTGRES_PASSWORD: &str = "monkeybot-v3-preview-password";
const DATABASE: &str = "monkeybot_v3_archive_preview";
const OPERATOR_ROLE_ID: &str = "999999999999999999";
const DEFAULT_TARGET_GUILD_ID: &str = "888888888888888888";

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

/// Local migration gate for a PII-bearing archive that is intentionally excluded from Git.
///
/// This test invokes only the preview path. The repository inserts the archive under a
/// PostgreSQL savepoint and rolls every imported domain row back before saving the preview.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires MONKEYBOT_V3_ARCHIVE_PATH and performs preview-only constraint validation"]
async fn mookbot_v3_archive_passes_preview_without_importing_domain_rows() -> TestResult {
    tokio::time::timeout(Duration::from_secs(120), run_preview())
        .await
        .map_err(|_| "MOOKBOT_v3 archive preview exceeded 120 seconds")??;
    Ok(())
}

async fn run_preview() -> TestResult {
    let archive_path = env::var("MONKEYBOT_V3_ARCHIVE_PATH")?;
    let archive: Value = serde_json::from_slice(&fs::read(archive_path)?)?;
    let source_guild_id_text = archive["source"]["guildId"]
        .as_str()
        .ok_or("archive source guildId is missing")?
        .to_owned();
    let source_guild_name = archive["source"]["guildName"]
        .as_str()
        .ok_or("archive source guildName is missing")?
        .to_owned();
    let target_guild_id_text = env::var("MONKEYBOT_V3_TARGET_GUILD_ID")
        .unwrap_or_else(|_| DEFAULT_TARGET_GUILD_ID.to_owned());
    let operator_id_text = archive["tables"]["guild_members"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["status"] == "ACTIVE"))
        .and_then(|row| row["user_id"].as_str())
        .ok_or("archive has no active guild member to use as the preview operator")?
        .to_owned();

    let server = TestPostgres::start().await?;
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&server.database_url())
        .await?;
    MIGRATOR.run(&pool).await?;
    seed_preview_target(
        &pool,
        &target_guild_id_text,
        &format!("{source_guild_name} migration target"),
        &operator_id_text,
    )
    .await?;
    let expected_domain_rows = domain_row_count(&pool, &target_guild_id_text).await?;
    assert!(expected_domain_rows > 1);

    let guild_id = GuildId::parse(target_guild_id_text.clone())?;
    let operator_id = UserId::parse(operator_id_text)?;
    let preview = guild_full_data_transfer::preview(
        &pool,
        &guild_id,
        &operator_id,
        archive,
        "mookbot-v3-preview-validation",
    )
    .await?;

    assert!(preview.conflicts.is_empty());
    assert!(preview.summary.total_rows > 0);
    assert_eq!(preview.source.guild_id.as_str(), source_guild_id_text);
    assert_eq!(
        preview.remaps_source_guild,
        source_guild_id_text != target_guild_id_text
    );
    assert!(preview.uses_target_active_season);
    assert!(preview.rebases_display_numbers);
    assert_eq!(
        domain_row_count(&pool, &target_guild_id_text).await?,
        expected_domain_rows
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM guild_full_data_imports
             WHERE guild_id = $1 AND status = 'PREVIEWED' AND archive_payload IS NOT NULL",
        )
        .bind(&target_guild_id_text)
        .fetch_one(&pool)
        .await?,
        1
    );

    pool.close().await;
    drop(server);
    Ok(())
}

async fn seed_preview_target(
    pool: &PgPool,
    guild_id: &str,
    guild_name: &str,
    operator_id: &str,
) -> TestResult {
    let now = OffsetDateTime::now_utc();
    sqlx::query(
        "INSERT INTO users (
             user_id, display_name, avatar_hash, created_at, updated_at, deleted_at, identity_kind
         ) VALUES ($1, 'Migration Preview Operator', NULL, $2, $2, NULL, 'DISCORD')",
    )
    .bind(operator_id)
    .bind(now)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO user_discord_identities (
             user_id, discord_id, identity_kind, created_at, updated_at
         ) VALUES ($1, $1, 'DISCORD', $2, $2)",
    )
    .bind(operator_id)
    .bind(now)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO guilds (
             guild_id, discord_name, icon_hash, status, installed_by,
             installed_at, activated_at, left_at, updated_at
         ) VALUES ($1, $2, NULL, 'ACTIVE', $3, $4, $4, NULL, $4)",
    )
    .bind(guild_id)
    .bind(guild_name)
    .bind(operator_id)
    .bind(now)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO guild_members (
             guild_id, user_id, display_name, status, main_lane, first_seen_at,
             registered_at, discord_joined_at, last_active_at, last_synced_at, inactivated_at
         ) VALUES ($1, $2, 'Migration Preview Operator', 'ACTIVE', NULL, $3, $3, NULL, $3, NULL, NULL)",
    )
    .bind(guild_id)
    .bind(operator_id)
    .bind(now)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO guild_config (
             guild_id, display_name, operator_role_id, recruitment_channel_id,
             log_channel_id, is_externally_public, revision, updated_by, updated_at
         ) VALUES ($1, $2, $3, NULL, NULL, false, 0, $4, $5)",
    )
    .bind(guild_id)
    .bind(guild_name)
    .bind(OPERATOR_ROLE_ID)
    .bind(operator_id)
    .bind(now)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO guild_member_discord_roles (guild_id, user_id, role_id, last_synced_at)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(guild_id)
    .bind(operator_id)
    .bind(OPERATOR_ROLE_ID)
    .bind(now)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO seasons (
             guild_id, id, name, status, started_at, ended_at, created_by, created_at
         ) VALUES ($1, $2, '시즌 1', 'ACTIVE', $3, NULL, $4, $3)",
    )
    .bind(guild_id)
    .bind(Uuid::now_v7())
    .bind(now)
    .bind(operator_id)
    .execute(pool)
    .await?;
    let closed_season_id = Uuid::now_v7();
    let closed_at = now - time::Duration::minutes(1);
    sqlx::query(
        "INSERT INTO seasons (
             guild_id, id, name, status, started_at, ended_at, created_by, created_at
         ) VALUES ($1, $2, '이전 시즌', 'CLOSED', $3, $4, $5, $3)",
    )
    .bind(guild_id)
    .bind(closed_season_id)
    .bind(now - time::Duration::hours(1))
    .bind(closed_at)
    .bind(operator_id)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO recruitments (
             guild_id, id, season_id, display_number, title, target_count, status, revision,
             channel_id, message_id, created_by, created_at, updated_by, updated_at,
             closed_at, converted_at, cancelled_at
         ) VALUES (
             $1, $2, $3, 1, '이전 시즌 모집', 10, 'CLOSED', 0,
             NULL, NULL, $4, $5, NULL, $6, $6, NULL, NULL
         )",
    )
    .bind(guild_id)
    .bind(Uuid::now_v7())
    .bind(closed_season_id)
    .bind(operator_id)
    .bind(now - time::Duration::hours(1))
    .bind(closed_at)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO user_lane_mmr (
             guild_id, season_id, user_id, lane, mmr, games_played, wins, revision, updated_at
         ) VALUES ($1, $2, $3, 'TOP', 1500, 0, 0, 0, $4)",
    )
    .bind(guild_id)
    .bind(closed_season_id)
    .bind(operator_id)
    .bind(closed_at)
    .execute(pool)
    .await?;
    Ok(())
}

async fn domain_row_count(pool: &PgPool, guild_id: &str) -> TestResult<i64> {
    Ok(sqlx::query_scalar(
        "SELECT
             (SELECT count(*) FROM seasons WHERE guild_id = $1)
             + (SELECT count(*) FROM recruitments WHERE guild_id = $1)
             + (SELECT count(*) FROM series WHERE guild_id = $1)
             + (SELECT count(*) FROM auction_tournaments WHERE guild_id = $1)
             + (SELECT count(*) FROM games WHERE guild_id = $1)
             + (SELECT count(*) FROM user_lane_mmr WHERE guild_id = $1)
             + (SELECT count(*) FROM mmr_ledger_entries WHERE guild_id = $1)",
    )
    .bind(guild_id)
    .fetch_one(pool)
    .await?)
}

struct TestPostgres {
    _container: ContainerAsync<GenericImage>,
    host: String,
    port: u16,
}

impl TestPostgres {
    async fn start() -> TestResult<Self> {
        let container = GenericImage::new(POSTGRES_IMAGE, POSTGRES_TAG)
            .with_exposed_port(5432.tcp())
            .with_wait_for(WaitFor::message_on_stderr(
                "database system is ready to accept connections",
            ))
            .with_env_var("POSTGRES_USER", "postgres")
            .with_env_var("POSTGRES_PASSWORD", POSTGRES_PASSWORD)
            .with_env_var("POSTGRES_DB", DATABASE)
            .start()
            .await?;
        let host = container.get_host().await?.to_string();
        let port = container.get_host_port_ipv4(5432.tcp()).await?;
        Ok(Self {
            _container: container,
            host,
            port,
        })
    }

    fn database_url(&self) -> String {
        format!(
            "postgresql://postgres:{POSTGRES_PASSWORD}@{}:{}/{DATABASE}",
            self.host, self.port
        )
    }
}
