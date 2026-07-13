use std::{
    collections::HashMap,
    error::Error,
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{Router, routing::get};
use monkeybot_api::{
    bootstrap::{BootstrapError, serve_with_shutdown},
    config::Config,
    domain::{DiscordUserId, GuildId, IdempotencyRecordId, Lane, SeasonId},
    infrastructure::postgres::{self, MIGRATOR, PostgresReadinessProbe},
    repository::{
        TenantScope,
        idempotency::{
            self, AcquireIdempotency, AcquireIdempotencyOutcome, CompleteIdempotency,
            CompleteIdempotencyOutcome, IdempotencyPrincipalType, IdempotentResponse,
        },
        mmr::{MmrLaneKey, ensure_and_lock_mmr_rows},
        outbox::{self, AckOutbox, ClaimOutbox, NackOutbox},
        riot_records::{
            self, AcquireRiotRecordsRefresh, CanonicalRiotRecordsIdentity,
            RIOT_RECORDS_REFRESH_LEASE_DURATION, RiotRecordsLookup, RiotRecordsRepositoryError,
        },
    },
    state::{AppState, ComponentStatus, DependencyProbe},
};
use serde_json::json;
use sqlx::{PgPool, postgres::PgPoolOptions};
use testcontainers::{
    ContainerAsync, GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use time::OffsetDateTime;
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
    sync::{Barrier, Notify, oneshot},
};
use uuid::Uuid;

const POSTGRES_IMAGE: &str = "postgres";
const POSTGRES_TAG: &str = "18.4-alpine3.23";
const POSTGRES_PASSWORD: &str = "monkeybot-integration-password";

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn postgres_foundation_contract() -> TestResult {
    tokio::time::timeout(Duration::from_secs(120), run_postgres_foundation_contract())
        .await
        .map_err(|_| "PostgreSQL integration test exceeded 120 seconds")??;
    Ok(())
}

async fn run_postgres_foundation_contract() -> TestResult {
    let server = TestPostgres::start().await?;
    let admin = PgPoolOptions::new()
        .max_connections(5)
        .connect(&server.database_url("postgres"))
        .await?;

    create_database(&admin, "monkeybot_full").await?;
    let migration_pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&server.database_url("monkeybot_full"))
        .await?;
    MIGRATOR.run(&migration_pool).await?;
    migration_pool.close().await;

    let runtime_config = runtime_config(server.database_url("monkeybot_full"))?;
    let full = postgres::connect(
        runtime_config
            .database()
            .expect("integration runtime database config"),
    )
    .await?;
    assert_runtime_connection_settings(&full).await?;
    assert_latest_schema(&full).await?;
    assert_snowflake_constraints(&full).await?;
    assert_riot_origin_identity_constraint(&full).await?;
    assert_fk_indexes_cover_child_columns(&full).await?;
    assert_constraints_and_tenant_isolation(&full).await?;
    assert_idempotency_contract(&full).await?;
    assert_expired_idempotency_reuse(&full).await?;
    assert_concurrent_idempotency_contract(&full).await?;
    assert_structured_idempotency_lock_key(&full).await?;
    assert_outbox_uses_database_clock(&full).await?;
    assert_outbox_claim_and_stale_ack(&full).await?;
    assert_concurrent_mmr_initialization(&full).await?;
    assert_riot_records_snapshot_contract(&full).await?;
    assert_concurrent_riot_records_refresh_contract(&full).await?;

    let snapshot = PostgresReadinessProbe::new(full.clone(), Duration::from_secs(1))
        .check()
        .await;
    assert_eq!(snapshot.postgres, ComponentStatus::Available);
    assert_eq!(snapshot.migration, ComponentStatus::Available);
    assert_checksum_mismatch_is_not_ready(&full).await?;

    create_database(&admin, "monkeybot_n_minus_one").await?;
    let n_minus_one = PgPoolOptions::new()
        .max_connections(2)
        .connect(&server.database_url("monkeybot_n_minus_one"))
        .await?;
    MIGRATOR.run_to(10, &n_minus_one).await?;
    let snapshot = PostgresReadinessProbe::new(n_minus_one.clone(), Duration::from_secs(1))
        .check()
        .await;
    assert_eq!(snapshot.postgres, ComponentStatus::Available);
    assert_eq!(snapshot.migration, ComponentStatus::Unavailable);

    assert_riot_origin_identity_upgrade(&admin, &server).await?;

    create_database(&admin, "monkeybot_empty").await?;
    let empty = PgPoolOptions::new()
        .max_connections(1)
        .connect(&server.database_url("monkeybot_empty"))
        .await?;
    let snapshot = PostgresReadinessProbe::new(empty.clone(), Duration::from_secs(1))
        .check()
        .await;
    assert_eq!(snapshot.postgres, ComponentStatus::Available);
    assert_eq!(snapshot.migration, ComponentStatus::Unavailable);

    empty.close().await;
    n_minus_one.close().await;
    assert_pool_close_uses_remaining_shutdown_deadline(runtime_config, &full).await?;
    admin.close().await;
    drop(server);
    Ok(())
}

fn runtime_config(database_url: String) -> Result<Config, Box<dyn Error + Send + Sync>> {
    let values = HashMap::from([
        ("MONKEYBOT_ENV".to_owned(), "test".to_owned()),
        (
            "MONKEYBOT_BIND_ADDR".to_owned(),
            "127.0.0.1:30020".to_owned(),
        ),
        (
            "MONKEYBOT_PUBLIC_ORIGIN".to_owned(),
            "http://localhost:5173".to_owned(),
        ),
        ("MONKEYBOT_DATABASE_URL".to_owned(), database_url),
        (
            "MONKEYBOT_DATABASE_MAX_CONNECTIONS".to_owned(),
            "4".to_owned(),
        ),
        (
            "MONKEYBOT_DATABASE_MIN_CONNECTIONS".to_owned(),
            "1".to_owned(),
        ),
        (
            "MONKEYBOT_DATABASE_STATEMENT_TIMEOUT_MS".to_owned(),
            "3000".to_owned(),
        ),
        (
            "MONKEYBOT_DATABASE_LOCK_TIMEOUT_MS".to_owned(),
            "1000".to_owned(),
        ),
        (
            "MONKEYBOT_DATABASE_IDLE_TRANSACTION_TIMEOUT_MS".to_owned(),
            "10000".to_owned(),
        ),
        ("MONKEYBOT_SHUTDOWN_TIMEOUT_MS".to_owned(), "400".to_owned()),
    ]);
    Ok(Config::from_map(&values)?)
}

async fn assert_pool_close_uses_remaining_shutdown_deadline(
    config: Config,
    pool: &PgPool,
) -> TestResult {
    let held_connection = pool.acquire().await?;
    let request_started = Arc::new(Notify::new());
    let release_request = Arc::new(Notify::new());
    let route_started = request_started.clone();
    let route_release = release_request.clone();
    let app = Router::new().route(
        "/slow",
        get(move || {
            let started = route_started.clone();
            let release = route_release.clone();
            async move {
                started.notify_one();
                release.notified().await;
                "ok"
            }
        }),
    );
    let state = AppState::with_postgres(
        config,
        pool.clone(),
        Arc::new(PostgresReadinessProbe::new(
            pool.clone(),
            Duration::from_secs(1),
        )),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        serve_with_shutdown(listener, app, state, async {
            let _ = shutdown_rx.await;
        })
        .await
    });

    let mut stream = TcpStream::connect(address).await?;
    stream
        .write_all(b"GET /slow HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await?;
    request_started.notified().await;
    let shutdown_started = Instant::now();
    shutdown_tx
        .send(())
        .map_err(|()| "shutdown receiver disappeared")?;
    tokio::time::sleep(Duration::from_millis(250)).await;
    release_request.notify_one();

    let result = server.await?;
    let elapsed = shutdown_started.elapsed();
    assert!(matches!(result, Err(BootstrapError::DrainTimeout(_))));
    assert!(
        elapsed < Duration::from_millis(550),
        "pool close received a second full shutdown timeout: {elapsed:?}"
    );
    assert!(pool.is_closed(), "pool close path was not entered");

    drop(stream);
    drop(held_connection);
    pool.close().await;
    Ok(())
}

async fn assert_runtime_connection_settings(pool: &PgPool) -> TestResult {
    let settings = sqlx::query_as::<_, (String, String, String, bool, bool, bool)>(
        "SELECT current_setting('application_name'),
                current_setting('search_path'),
                current_setting('TimeZone'),
                current_setting('statement_timeout')::interval = INTERVAL '3 seconds',
                current_setting('lock_timeout')::interval = INTERVAL '1 second',
                current_setting('idle_in_transaction_session_timeout')::interval
                    = INTERVAL '10 seconds'",
    )
    .fetch_one(pool)
    .await?;

    assert_eq!(settings.0, "monkeybot-api-test");
    assert_eq!(settings.1, "public, pg_catalog");
    assert_eq!(settings.2, "UTC");
    assert!(settings.3, "statement_timeout was not applied");
    assert!(settings.4, "lock_timeout was not applied");
    assert!(
        settings.5,
        "idle_in_transaction_session_timeout was not applied"
    );
    Ok(())
}

async fn assert_checksum_mismatch_is_not_ready(pool: &PgPool) -> TestResult {
    let latest_version = MIGRATOR
        .iter()
        .next_back()
        .expect("at least one embedded migration")
        .version;
    let original_checksum = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT checksum FROM public._sqlx_migrations WHERE version = $1",
    )
    .bind(latest_version)
    .fetch_one(pool)
    .await?;

    sqlx::query("UPDATE public._sqlx_migrations SET checksum = $1 WHERE version = $2")
        .bind([0_u8])
        .bind(latest_version)
        .execute(pool)
        .await?;
    let snapshot = PostgresReadinessProbe::new(pool.clone(), Duration::from_secs(1))
        .check()
        .await;
    assert_eq!(snapshot.postgres, ComponentStatus::Available);
    assert_eq!(snapshot.migration, ComponentStatus::Unavailable);

    sqlx::query("UPDATE public._sqlx_migrations SET checksum = $1 WHERE version = $2")
        .bind(original_checksum)
        .bind(latest_version)
        .execute(pool)
        .await?;
    let snapshot = PostgresReadinessProbe::new(pool.clone(), Duration::from_secs(1))
        .check()
        .await;
    assert_eq!(snapshot.postgres, ComponentStatus::Available);
    assert_eq!(snapshot.migration, ComponentStatus::Available);
    Ok(())
}

async fn assert_latest_schema(pool: &PgPool) -> TestResult {
    let migration_count =
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM public._sqlx_migrations WHERE success")
            .fetch_one(pool)
            .await?;
    assert_eq!(migration_count, MIGRATOR.iter().count() as i64);

    let table_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*)
         FROM information_schema.tables
         WHERE table_schema = 'public' AND table_type = 'BASE TABLE'",
    )
    .fetch_one(pool)
    .await?;
    assert_eq!(table_count, 63, "unexpected public table count");
    Ok(())
}

async fn assert_riot_records_snapshot_contract(pool: &PgPool) -> TestResult {
    let lookup = RiotRecordsLookup::new("KR", "Longcat", "KR1")?;
    assert!(riot_records::get_snapshot(pool, &lookup).await?.is_none());

    let lease = match riot_records::acquire_refresh(pool, &lookup).await? {
        AcquireRiotRecordsRefresh::Acquired(lease) => lease,
        outcome => {
            return Err(format!("initial Riot refresh was not acquired: {outcome:?}").into());
        }
    };
    assert_eq!(
        lease.expires_at - lease.acquired_at,
        time::Duration::seconds(RIOT_RECORDS_REFRESH_LEASE_DURATION.as_secs() as i64)
    );
    match riot_records::acquire_refresh(pool, &lookup).await? {
        AcquireRiotRecordsRefresh::InProgress { snapshot: None, .. } => {}
        outcome => {
            return Err(format!("concurrent cache miss was not coalesced: {outcome:?}").into());
        }
    }

    let identity = CanonicalRiotRecordsIdentity::new("KR", "test-puuid", "Longcat", "KR1")?;
    let provider_neutral = json!({
        "account": { "gameName": "Longcat", "tagLine": "KR1" },
        "recentMatches": [{ "matchId": "KR_1" }],
        "sample": { "recentMatches": { "requested": 10, "returned": 1 } }
    });
    let stored = riot_records::complete_refresh(pool, &lease, &identity, &provider_neutral).await?;
    assert_eq!(stored.platform, "KR");
    assert_eq!(stored.schema_version, 1);

    let cached = riot_records::get_snapshot(pool, &lookup)
        .await?
        .ok_or("completed Riot snapshot was not readable")?;
    assert_eq!(cached.snapshot, provider_neutral);
    assert_eq!(cached.puuid(), "test-puuid");
    assert!(!format!("{cached:?}").contains("test-puuid"));
    assert_eq!(cached.game_name, "Longcat");
    assert_eq!(cached.tag_line, "KR1");
    let recent_page = json!({
        "offset": 10,
        "requested": 10,
        "matches": [{ "matchId": "KR_11" }]
    });
    assert!(
        riot_records::get_recent_page(pool, &lookup, 10, 10)
            .await?
            .is_none()
    );
    let stored_page = riot_records::store_recent_page(pool, &lookup, 10, 10, &recent_page).await?;
    assert_eq!(stored_page.offset, 10);
    assert_eq!(stored_page.requested_count, 10);
    let cached_page = riot_records::get_recent_page(pool, &lookup, 10, 10)
        .await?
        .ok_or("stored recent page was not readable")?;
    assert_eq!(cached_page.page, recent_page);
    assert!(
        riot_records::get_recent_page(pool, &lookup, 10, 20)
            .await?
            .is_none(),
        "page cache keys must preserve the exact requested count"
    );
    match riot_records::acquire_refresh(pool, &lookup).await? {
        AcquireRiotRecordsRefresh::Cooldown { snapshot, retry_at } => {
            assert_eq!(snapshot.snapshot, provider_neutral);
            assert!(retry_at > snapshot.refreshed_at);
        }
        outcome => return Err(format!("30-second cooldown was not enforced: {outcome:?}").into()),
    }

    sqlx::query(
        "UPDATE riot_records_snapshots
         SET refreshed_at = clock_timestamp() - INTERVAL '31 seconds'
         WHERE platform = 'KR' AND puuid = 'test-puuid'",
    )
    .execute(pool)
    .await?;
    let before_failure = riot_records::get_snapshot(pool, &lookup)
        .await?
        .ok_or("aged Riot snapshot disappeared")?;
    let failed_lease = match riot_records::acquire_refresh(pool, &lookup).await? {
        AcquireRiotRecordsRefresh::Acquired(lease) => lease,
        outcome => return Err(format!("aged Riot refresh was not acquired: {outcome:?}").into()),
    };
    assert!(riot_records::abort_refresh(pool, &failed_lease).await?);
    let after_failure = riot_records::get_snapshot(pool, &lookup)
        .await?
        .ok_or("provider failure deleted the last good Riot snapshot")?;
    assert_eq!(after_failure, before_failure);

    let replacement = match riot_records::acquire_refresh(pool, &lookup).await? {
        AcquireRiotRecordsRefresh::Acquired(lease) => lease,
        outcome => {
            return Err(format!("post-failure Riot refresh was not acquired: {outcome:?}").into());
        }
    };
    riot_records::complete_refresh(
        pool,
        &replacement,
        &identity,
        &json!({ "generation": "new-top-snapshot" }),
    )
    .await?;
    assert!(
        riot_records::get_recent_page(pool, &lookup, 10, 10)
            .await?
            .is_none(),
        "a refreshed top snapshot must invalidate ordered recent pages"
    );
    Ok(())
}

async fn assert_concurrent_riot_records_refresh_contract(pool: &PgPool) -> TestResult {
    let lookup = RiotRecordsLookup::new("KR", "Concurrency", "KR1")?;
    let initial_lease = match riot_records::acquire_refresh(pool, &lookup).await? {
        AcquireRiotRecordsRefresh::Acquired(lease) => lease,
        outcome => {
            return Err(format!("initial concurrent probe was not acquired: {outcome:?}").into());
        }
    };
    assert!(riot_records::abort_refresh(pool, &initial_lease).await?);

    let barrier = Arc::new(Barrier::new(9));
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let pool = pool.clone();
        let lookup = lookup.clone();
        let barrier = barrier.clone();
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            riot_records::acquire_refresh(&pool, &lookup).await
        }));
    }
    barrier.wait().await;

    let mut acquired = Vec::new();
    let mut in_progress = 0;
    for task in tasks {
        match task.await?? {
            AcquireRiotRecordsRefresh::Acquired(lease) => acquired.push(lease),
            AcquireRiotRecordsRefresh::InProgress { snapshot: None, .. } => in_progress += 1,
            outcome => return Err(format!("unexpected concurrent outcome: {outcome:?}").into()),
        }
    }
    assert_eq!(
        acquired.len(),
        1,
        "exactly one provider call must be claimed"
    );
    assert_eq!(in_progress, 7);

    let expired_lease = acquired.pop().expect("one acquired lease");
    sqlx::query(
        "UPDATE riot_records_refresh_gates
         SET lease_acquired_at = clock_timestamp() - INTERVAL '2 seconds',
             lease_until = clock_timestamp() - INTERVAL '1 second'
         WHERE platform = 'KR'
           AND normalized_game_name = 'concurrency'
           AND normalized_tag_line = 'kr1'",
    )
    .execute(pool)
    .await?;
    let replacement_lease = match riot_records::acquire_refresh(pool, &lookup).await? {
        AcquireRiotRecordsRefresh::Acquired(lease) => lease,
        outcome => return Err(format!("expired lease was not replaceable: {outcome:?}").into()),
    };
    let identity =
        CanonicalRiotRecordsIdentity::new("KR", "concurrent-puuid", "Concurrency", "KR1")?;
    let late_completion = riot_records::complete_refresh(
        pool,
        &expired_lease,
        &identity,
        &json!({ "generation": "expired" }),
    )
    .await;
    assert!(matches!(
        late_completion,
        Err(RiotRecordsRepositoryError::LeaseLost)
    ));
    riot_records::complete_refresh(
        pool,
        &replacement_lease,
        &identity,
        &json!({ "generation": "current" }),
    )
    .await?;
    let snapshot = riot_records::get_snapshot(pool, &lookup)
        .await?
        .ok_or("replacement lease did not persist a snapshot")?;
    assert_eq!(snapshot.snapshot, json!({ "generation": "current" }));
    Ok(())
}

async fn assert_snowflake_constraints(pool: &PgPool) -> TestResult {
    for valid in ["1", "123456789012345678", "18446744073709551615"] {
        let accepted = sqlx::query_scalar::<_, bool>("SELECT public.is_discord_snowflake($1)")
            .bind(valid)
            .fetch_one(pool)
            .await?;
        assert!(accepted, "rejected canonical snowflake {valid}");
    }

    for invalid in [
        "",
        "0",
        "01",
        "-1",
        "not-a-snowflake",
        "18446744073709551616",
    ] {
        let accepted = sqlx::query_scalar::<_, bool>("SELECT public.is_discord_snowflake($1)")
            .bind(invalid)
            .fetch_one(pool)
            .await?;
        assert!(!accepted, "accepted invalid snowflake {invalid}");
    }

    sqlx::query(
        "INSERT INTO users (user_id, identity_kind, display_name, created_at, updated_at)
         VALUES ('dmy_01900000-0000-7000-8000-000000000001', 'DUMMY',
                 'Dummy User', clock_timestamp(), clock_timestamp())",
    )
    .execute(pool)
    .await?;

    let invalid_user = sqlx::query(
        "INSERT INTO users (user_id, display_name, created_at, updated_at)
         VALUES (' invalid', 'invalid', clock_timestamp(), clock_timestamp())",
    )
    .execute(pool)
    .await
    .expect_err("users.user_id must reject untrimmed identifiers");
    assert_eq!(constraint_name(&invalid_user), Some("users_user_id_check"));

    for (user_id, identity_kind) in [
        ("100000000000000098", "DUMMY"),
        ("dmy_01900000-0000-7000-8000-000000000002", "DISCORD"),
        ("dmy_01900000-0000-6000-8000-000000000003", "DUMMY"),
    ] {
        let invalid_identity = sqlx::query(
            "INSERT INTO users (
                 user_id, identity_kind, display_name, created_at, updated_at
             ) VALUES ($1, $2, 'invalid identity', clock_timestamp(), clock_timestamp())",
        )
        .bind(user_id)
        .bind(identity_kind)
        .execute(pool)
        .await
        .expect_err("users.user_id must match its identity kind");
        assert_eq!(
            constraint_name(&invalid_identity),
            Some("users_user_id_check")
        );
    }

    let invalid_dummy_discord_identity = sqlx::query(
        "INSERT INTO user_discord_identities (
             user_id, discord_id, created_at, updated_at
         ) VALUES (
             'dmy_01900000-0000-7000-8000-000000000001',
             '100000000000000099', clock_timestamp(), clock_timestamp()
         )",
    )
    .execute(pool)
    .await
    .expect_err("DUMMY users must not own Discord identities");
    assert_eq!(
        constraint_name(&invalid_dummy_discord_identity),
        Some("user_discord_identities_user_fk")
    );

    let invalid_guild = sqlx::query(
        "INSERT INTO guilds (
            guild_id, discord_name, status, installed_at, activated_at, updated_at
         ) VALUES ('18446744073709551616', 'invalid', 'ACTIVE',
                   clock_timestamp(), clock_timestamp(), clock_timestamp())",
    )
    .execute(pool)
    .await
    .expect_err("guilds.guild_id must enforce the snowflake constraint");
    assert_eq!(
        constraint_name(&invalid_guild),
        Some("guilds_guild_id_check")
    );
    Ok(())
}

async fn assert_riot_origin_identity_constraint(pool: &PgPool) -> TestResult {
    let discord_user_id = "100000000000000097";
    let dummy_user_id = "dmy_01900000-0000-7000-8000-000000000001";
    sqlx::query(
        "INSERT INTO users (user_id, display_name, created_at, updated_at)
         VALUES ($1, 'Riot Owner', clock_timestamp(), clock_timestamp())",
    )
    .bind(discord_user_id)
    .execute(pool)
    .await?;

    for (puuid, user_id, origin, synced) in [
        ("riot:valid-owner", discord_user_id, "RIOT", true),
        ("dummy:valid-owner", dummy_user_id, "DUMMY", false),
    ] {
        insert_riot_account(pool, puuid, user_id, origin, synced).await?;
    }
    let valid_pairs = sqlx::query_as::<_, (String, String)>(
        "SELECT origin, owner_identity_kind
         FROM riot_accounts
         WHERE puuid IN ('riot:valid-owner', 'dummy:valid-owner')
         ORDER BY origin",
    )
    .fetch_all(pool)
    .await?;
    assert_eq!(
        valid_pairs,
        vec![
            ("DUMMY".to_owned(), "DUMMY".to_owned()),
            ("RIOT".to_owned(), "DISCORD".to_owned()),
        ]
    );

    for (puuid, user_id, origin, synced) in [
        ("riot:dummy-owner", dummy_user_id, "RIOT", true),
        ("dummy:discord-owner", discord_user_id, "DUMMY", false),
    ] {
        let mismatch = insert_riot_account(pool, puuid, user_id, origin, synced)
            .await
            .expect_err("Riot account origin must match the owner's identity kind");
        assert_eq!(
            constraint_name(&mismatch),
            Some("riot_accounts_owner_identity_fk")
        );
    }
    Ok(())
}

async fn insert_riot_account(
    pool: &PgPool,
    puuid: &str,
    user_id: &str,
    origin: &str,
    synced: bool,
) -> Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO riot_accounts (
             puuid, user_id, platform, routing_region,
             game_name, tag_line, normalized_game_name, normalized_tag_line,
             profile_icon_id, is_main, main_position, main_position_updated_at,
             last_synced_at, created_at, updated_at, origin
         ) VALUES (
             $1, $2, 'KR', 'ASIA',
             $1, 'TEST', $1, 'test',
             NULL, false, NULL, NULL,
             CASE WHEN $4 THEN clock_timestamp() ELSE NULL END,
             clock_timestamp(), clock_timestamp(), $3
         )",
    )
    .bind(puuid)
    .bind(user_id)
    .bind(origin)
    .bind(synced)
    .execute(pool)
    .await
}

async fn assert_riot_origin_identity_upgrade(admin: &PgPool, server: &TestPostgres) -> TestResult {
    create_database(admin, "monkeybot_identity_upgrade").await?;
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&server.database_url("monkeybot_identity_upgrade"))
        .await?;
    MIGRATOR.run_to(12, &pool).await?;
    sqlx::query(
        "INSERT INTO users (user_id, display_name, created_at, updated_at)
         VALUES ('100000000000000096', 'Legacy Riot Owner', clock_timestamp(), clock_timestamp())",
    )
    .execute(&pool)
    .await?;
    sqlx::query(
        "INSERT INTO riot_accounts (
             puuid, user_id, platform, routing_region,
             game_name, tag_line, normalized_game_name, normalized_tag_line,
             profile_icon_id, is_main, main_position, main_position_updated_at,
             last_synced_at, created_at, updated_at
         ) VALUES (
             'legacy-puuid', '100000000000000096', 'KR', 'ASIA',
             'Legacy', 'KR1', 'legacy', 'kr1',
             NULL, true, NULL, NULL,
             clock_timestamp(), clock_timestamp(), clock_timestamp()
         )",
    )
    .execute(&pool)
    .await?;

    MIGRATOR.run(&pool).await?;
    let upgraded = sqlx::query_as::<_, (String, String)>(
        "SELECT origin, owner_identity_kind
         FROM riot_accounts
         WHERE puuid = 'legacy-puuid'",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(
        upgraded,
        ("RIOT".to_owned(), "DISCORD".to_owned()),
        "migrations 0013/0014 must classify existing Riot accounts without rewriting ownership"
    );
    pool.close().await;
    Ok(())
}

async fn assert_fk_indexes_cover_child_columns(pool: &PgPool) -> TestResult {
    let uncovered = sqlx::query_as::<_, (String, String)>(
        "SELECT constraint_table.relname, constraint_record.conname
         FROM pg_constraint AS constraint_record
         JOIN pg_class AS constraint_table ON constraint_table.oid = constraint_record.conrelid
         WHERE constraint_record.contype = 'f'
           AND constraint_record.connamespace = 'public'::regnamespace
           AND cardinality(constraint_record.conkey) > 1
           AND NOT EXISTS (
               SELECT 1
               FROM pg_index AS index_record
               WHERE index_record.indrelid = constraint_record.conrelid
                 AND index_record.indisvalid
                 AND index_record.indisready
                 AND index_record.indpred IS NULL
                 AND index_record.indnkeyatts >= cardinality(constraint_record.conkey)
                 AND NOT EXISTS (
                     SELECT 1
                     FROM generate_subscripts(constraint_record.conkey, 1) AS position
                     WHERE constraint_record.conkey[position]
                           <> index_record.indkey[position - 1]
                 )
           )
         ORDER BY constraint_table.relname, constraint_record.conname",
    )
    .fetch_all(pool)
    .await?;

    assert!(
        uncovered.is_empty(),
        "foreign keys without a child-leading index: {uncovered:?}"
    );
    Ok(())
}

async fn assert_constraints_and_tenant_isolation(pool: &PgPool) -> TestResult {
    let now = OffsetDateTime::from_unix_timestamp(1_750_000_000)?;
    let user_id = "100000000000000001";
    let guild_a = "200000000000000001";
    let guild_b = "200000000000000002";
    let season_a = Uuid::now_v7();
    let season_b = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO users (user_id, display_name, created_at, updated_at)
         VALUES ($1, 'Longcat', $2, $2)",
    )
    .bind(user_id)
    .bind(now)
    .execute(pool)
    .await?;
    for guild_id in [guild_a, guild_b] {
        sqlx::query(
            "INSERT INTO guilds (
                guild_id, discord_name, status, installed_by, installed_at, activated_at, updated_at
             ) VALUES ($1, $2, 'ACTIVE', $3, $4, $4, $4)",
        )
        .bind(guild_id)
        .bind(format!("guild-{guild_id}"))
        .bind(user_id)
        .bind(now)
        .execute(pool)
        .await?;
        sqlx::query(
            "INSERT INTO guild_members (
                guild_id, user_id, display_name, status, first_seen_at, registered_at
             ) VALUES ($1, $2, 'Longcat', 'ACTIVE', $3, $3)",
        )
        .bind(guild_id)
        .bind(user_id)
        .bind(now)
        .execute(pool)
        .await?;
    }
    for (guild_id, role_id) in [
        (guild_a, "300000000000000001"),
        (guild_b, "300000000000000002"),
    ] {
        sqlx::query(
            "INSERT INTO guild_config (
                guild_id, display_name, operator_role_id, recruitment_channel_id,
                entry_channel_id, log_channel_id, is_externally_public, revision,
                updated_by, updated_at
             ) VALUES ($1, $2, $3, NULL, NULL, NULL, false, 0, $4, $5)",
        )
        .bind(guild_id)
        .bind(format!("config-{guild_id}"))
        .bind(role_id)
        .bind(user_id)
        .bind(now)
        .execute(pool)
        .await?;
    }
    for (guild_id, season_id) in [(guild_a, season_a), (guild_b, season_b)] {
        sqlx::query(
            "INSERT INTO seasons (
                guild_id, id, name, status, started_at, created_by, created_at
             ) VALUES ($1, $2, 'Season 1', 'ACTIVE', $3, $4, $3)",
        )
        .bind(guild_id)
        .bind(season_id)
        .bind(now)
        .bind(user_id)
        .execute(pool)
        .await?;
    }

    let cross_guild = sqlx::query(
        "INSERT INTO recruitments (
            guild_id, id, season_id, display_number, title, status, revision,
            created_by, created_at, updated_at
         ) VALUES ($1, $2, $3, 1, 'cross guild', 'OPEN', 0, $4, $5, $5)",
    )
    .bind(guild_a)
    .bind(Uuid::now_v7())
    .bind(season_b)
    .bind(user_id)
    .bind(now)
    .execute(pool)
    .await
    .expect_err("cross-guild season reference must fail");
    assert_eq!(
        constraint_name(&cross_guild),
        Some("recruitments_season_fk")
    );

    let duplicate_active = sqlx::query(
        "INSERT INTO seasons (
            guild_id, id, name, status, started_at, created_by, created_at
         ) VALUES ($1, $2, 'Season 2', 'ACTIVE', $3, $4, $3)",
    )
    .bind(guild_a)
    .bind(Uuid::now_v7())
    .bind(now)
    .bind(user_id)
    .execute(pool)
    .await
    .expect_err("a guild may only have one active season");
    assert_eq!(
        constraint_name(&duplicate_active),
        Some("seasons_one_active_per_guild_idx")
    );

    let invalid_lane = sqlx::query(
        "UPDATE guild_members SET main_lane = 'FLEX' WHERE guild_id = $1 AND user_id = $2",
    )
    .bind(guild_a)
    .bind(user_id)
    .execute(pool)
    .await
    .expect_err("non-canonical lane must fail");
    assert_eq!(
        constraint_name(&invalid_lane),
        Some("guild_members_main_lane_check")
    );

    let invalid_entry_channel = sqlx::query(
        "UPDATE guild_config SET entry_channel_id = 'not-a-snowflake' WHERE guild_id = $1",
    )
    .bind(guild_a)
    .execute(pool)
    .await
    .expect_err("entry channel must be a Discord snowflake");
    assert_eq!(
        constraint_name(&invalid_entry_channel),
        Some("guild_config_entry_channel_id_check")
    );

    let everyone_role = sqlx::query(
        "INSERT INTO guild_discord_roles (
            guild_id, role_id, name, position, managed, updated_at
         ) VALUES ($1, $1, '@everyone', 0, false, $2)",
    )
    .bind(guild_a)
    .bind(now)
    .execute(pool)
    .await
    .expect_err("@everyone must not be cached as a selectable role");
    assert_eq!(
        constraint_name(&everyone_role),
        Some("guild_discord_roles_not_everyone_check")
    );
    Ok(())
}

async fn assert_idempotency_contract(pool: &PgPool) -> TestResult {
    let now = OffsetDateTime::from_unix_timestamp(1_750_000_100)?;
    let guild_id = GuildId::parse("200000000000000001")?;
    let scope = TenantScope::guild(guild_id);
    let record_id = IdempotencyRecordId::new();
    let request_hash = [7_u8; 32];

    let mut transaction = pool.begin().await?;
    let acquired = idempotency::acquire(
        &mut transaction,
        &AcquireIdempotency {
            record_id,
            scope: &scope,
            principal_type: IdempotencyPrincipalType::User,
            principal_id: "100000000000000001",
            method: "POST",
            route_template: "/api/guilds/{guildId}/series",
            idempotency_key: "interaction-1",
            request_hash: &request_hash,
            created_at: now,
            expires_at: now + time::Duration::hours(24),
        },
    )
    .await?;
    assert_eq!(acquired, AcquireIdempotencyOutcome::Acquired { record_id });
    let completed = idempotency::complete(
        &mut transaction,
        CompleteIdempotency {
            scope: &scope,
            record_id,
            response: IdempotentResponse {
                status_code: 201,
                body: Some(json!({ "id": "series-1" })),
            },
            completed_at: now + time::Duration::seconds(1),
        },
    )
    .await?;
    assert_eq!(completed, CompleteIdempotencyOutcome::Completed);
    transaction.commit().await?;

    let mut transaction = pool.begin().await?;
    let replay = idempotency::acquire(
        &mut transaction,
        &AcquireIdempotency {
            record_id: IdempotencyRecordId::new(),
            scope: &scope,
            principal_type: IdempotencyPrincipalType::User,
            principal_id: "100000000000000001",
            method: "POST",
            route_template: "/api/guilds/{guildId}/series",
            idempotency_key: "interaction-1",
            request_hash: &request_hash,
            created_at: now,
            expires_at: now + time::Duration::hours(24),
        },
    )
    .await?;
    assert_eq!(
        replay,
        AcquireIdempotencyOutcome::Replay {
            record_id,
            response: IdempotentResponse {
                status_code: 201,
                body: Some(json!({ "id": "series-1" })),
            },
        }
    );
    transaction.commit().await?;

    let different_hash = [8_u8; 32];
    let mut transaction = pool.begin().await?;
    let conflict = idempotency::acquire(
        &mut transaction,
        &AcquireIdempotency {
            record_id: IdempotencyRecordId::new(),
            scope: &scope,
            principal_type: IdempotencyPrincipalType::User,
            principal_id: "100000000000000001",
            method: "POST",
            route_template: "/api/guilds/{guildId}/series",
            idempotency_key: "interaction-1",
            request_hash: &different_hash,
            created_at: now,
            expires_at: now + time::Duration::hours(24),
        },
    )
    .await?;
    assert_eq!(conflict, AcquireIdempotencyOutcome::Conflict { record_id });
    transaction.rollback().await?;
    Ok(())
}

async fn assert_expired_idempotency_reuse(pool: &PgPool) -> TestResult {
    let now = OffsetDateTime::from_unix_timestamp(1_750_000_125)?;
    let expiry = now + time::Duration::hours(1);
    let scope = TenantScope::guild(GuildId::parse("200000000000000001")?);

    let completed_id = IdempotencyRecordId::new();
    let original_hash = [12_u8; 32];
    let mut original = pool.begin().await?;
    let acquired = idempotency::acquire(
        &mut original,
        &AcquireIdempotency {
            record_id: completed_id,
            scope: &scope,
            principal_type: IdempotencyPrincipalType::User,
            principal_id: "100000000000000001",
            method: "PATCH",
            route_template: "/api/guilds/{guildId}/members/{memberId}/mmr",
            idempotency_key: "expired-completed",
            request_hash: &original_hash,
            created_at: now,
            expires_at: expiry,
        },
    )
    .await?;
    assert_eq!(
        acquired,
        AcquireIdempotencyOutcome::Acquired {
            record_id: completed_id
        }
    );
    let completed = idempotency::complete(
        &mut original,
        CompleteIdempotency {
            scope: &scope,
            record_id: completed_id,
            response: IdempotentResponse {
                status_code: 200,
                body: Some(json!({ "revision": 1 })),
            },
            completed_at: now + time::Duration::seconds(1),
        },
    )
    .await?;
    assert_eq!(completed, CompleteIdempotencyOutcome::Completed);
    original.commit().await?;

    let replacement_id = IdempotencyRecordId::new();
    let replacement_hash = [13_u8; 32];
    let mut replacement = pool.begin().await?;
    let replaced = idempotency::acquire(
        &mut replacement,
        &AcquireIdempotency {
            record_id: replacement_id,
            scope: &scope,
            principal_type: IdempotencyPrincipalType::User,
            principal_id: "100000000000000001",
            method: "PATCH",
            route_template: "/api/guilds/{guildId}/members/{memberId}/mmr",
            idempotency_key: "expired-completed",
            request_hash: &replacement_hash,
            created_at: expiry,
            expires_at: expiry + time::Duration::hours(24),
        },
    )
    .await?;
    assert_eq!(
        replaced,
        AcquireIdempotencyOutcome::Acquired {
            record_id: replacement_id
        }
    );
    replacement.commit().await?;

    let stale_id = IdempotencyRecordId::new();
    let mut stale = pool.begin().await?;
    let acquired = idempotency::acquire(
        &mut stale,
        &AcquireIdempotency {
            record_id: stale_id,
            scope: &scope,
            principal_type: IdempotencyPrincipalType::User,
            principal_id: "100000000000000001",
            method: "PATCH",
            route_template: "/api/guilds/{guildId}/members/{memberId}/mmr",
            idempotency_key: "expired-in-progress",
            request_hash: &original_hash,
            created_at: now,
            expires_at: expiry,
        },
    )
    .await?;
    assert_eq!(
        acquired,
        AcquireIdempotencyOutcome::Acquired {
            record_id: stale_id
        }
    );
    stale.commit().await?;

    let stale_replacement_id = IdempotencyRecordId::new();
    let mut stale_replacement = pool.begin().await?;
    let replaced = idempotency::acquire(
        &mut stale_replacement,
        &AcquireIdempotency {
            record_id: stale_replacement_id,
            scope: &scope,
            principal_type: IdempotencyPrincipalType::User,
            principal_id: "100000000000000001",
            method: "PATCH",
            route_template: "/api/guilds/{guildId}/members/{memberId}/mmr",
            idempotency_key: "expired-in-progress",
            request_hash: &replacement_hash,
            created_at: expiry,
            expires_at: expiry + time::Duration::hours(24),
        },
    )
    .await?;
    assert_eq!(
        replaced,
        AcquireIdempotencyOutcome::Acquired {
            record_id: stale_replacement_id,
        }
    );
    stale_replacement.commit().await?;
    Ok(())
}

fn idempotency_command<'a>(
    record_id: IdempotencyRecordId,
    scope: &'a TenantScope,
    principal_id: &'a str,
    method: &'a str,
    idempotency_key: &'a str,
    request_hash: &'a [u8],
    now: OffsetDateTime,
) -> AcquireIdempotency<'a> {
    AcquireIdempotency {
        record_id,
        scope,
        principal_type: IdempotencyPrincipalType::User,
        principal_id,
        method,
        route_template: "/api/guilds/{guildId}/series",
        idempotency_key,
        request_hash,
        created_at: now,
        expires_at: now + time::Duration::hours(24),
    }
}

async fn assert_concurrent_idempotency_contract(pool: &PgPool) -> TestResult {
    let now = OffsetDateTime::from_unix_timestamp(1_750_000_150)?;
    let scope = TenantScope::guild(GuildId::parse("200000000000000001")?);
    let record_id = IdempotencyRecordId::new();
    let request_hash = [9_u8; 32];

    let mut leading = pool.begin().await?;
    let acquired = idempotency::acquire(
        &mut leading,
        &idempotency_command(
            record_id,
            &scope,
            "100000000000000001",
            "POST",
            "interaction-concurrent",
            &request_hash,
            now,
        ),
    )
    .await?;
    assert_eq!(acquired, AcquireIdempotencyOutcome::Acquired { record_id });

    let mut competing = pool.begin().await?;
    let in_progress = tokio::time::timeout(
        Duration::from_millis(500),
        idempotency::acquire(
            &mut competing,
            &idempotency_command(
                IdempotencyRecordId::new(),
                &scope,
                "100000000000000001",
                "POST",
                "interaction-concurrent",
                &request_hash,
                now,
            ),
        ),
    )
    .await
    .map_err(|_| "concurrent idempotency acquire waited for the leading transaction")??;
    assert_eq!(in_progress, AcquireIdempotencyOutcome::InProgress);
    competing.rollback().await?;

    let completed = idempotency::complete(
        &mut leading,
        CompleteIdempotency {
            scope: &scope,
            record_id,
            response: IdempotentResponse {
                status_code: 201,
                body: Some(json!({ "id": "series-concurrent" })),
            },
            completed_at: now + time::Duration::seconds(1),
        },
    )
    .await?;
    assert_eq!(completed, CompleteIdempotencyOutcome::Completed);
    leading.commit().await?;

    let mut replay_transaction = pool.begin().await?;
    let replay = idempotency::acquire(
        &mut replay_transaction,
        &idempotency_command(
            IdempotencyRecordId::new(),
            &scope,
            "100000000000000001",
            "POST",
            "interaction-concurrent",
            &request_hash,
            now,
        ),
    )
    .await?;
    assert_eq!(
        replay,
        AcquireIdempotencyOutcome::Replay {
            record_id,
            response: IdempotentResponse {
                status_code: 201,
                body: Some(json!({ "id": "series-concurrent" })),
            },
        }
    );
    replay_transaction.commit().await?;

    let conflicting_hash = [10_u8; 32];
    let mut conflict_transaction = pool.begin().await?;
    let conflict = idempotency::acquire(
        &mut conflict_transaction,
        &idempotency_command(
            IdempotencyRecordId::new(),
            &scope,
            "100000000000000001",
            "POST",
            "interaction-concurrent",
            &conflicting_hash,
            now,
        ),
    )
    .await?;
    assert_eq!(conflict, AcquireIdempotencyOutcome::Conflict { record_id });
    conflict_transaction.rollback().await?;
    Ok(())
}

async fn assert_structured_idempotency_lock_key(pool: &PgPool) -> TestResult {
    let now = OffsetDateTime::from_unix_timestamp(1_750_000_175)?;
    let scope = TenantScope::guild(GuildId::parse("200000000000000001")?);
    let request_hash = [11_u8; 32];

    let mut first = pool.begin().await?;
    let first_id = IdempotencyRecordId::new();
    let first_outcome = idempotency::acquire(
        &mut first,
        &idempotency_command(
            first_id,
            &scope,
            "actor|POST",
            "PATCH",
            "separator-collision",
            &request_hash,
            now,
        ),
    )
    .await?;
    assert_eq!(
        first_outcome,
        AcquireIdempotencyOutcome::Acquired {
            record_id: first_id
        }
    );

    let mut second = pool.begin().await?;
    let second_id = IdempotencyRecordId::new();
    let second_outcome = tokio::time::timeout(
        Duration::from_millis(500),
        idempotency::acquire(
            &mut second,
            &idempotency_command(
                second_id,
                &scope,
                "actor",
                "POST|PATCH",
                "separator-collision",
                &request_hash,
                now,
            ),
        ),
    )
    .await
    .map_err(|_| "structurally distinct idempotency command shared an advisory lock")??;
    assert_eq!(
        second_outcome,
        AcquireIdempotencyOutcome::Acquired {
            record_id: second_id
        }
    );
    second.rollback().await?;
    first.rollback().await?;
    Ok(())
}

async fn assert_outbox_claim_and_stale_ack(pool: &PgPool) -> TestResult {
    let now = OffsetDateTime::from_unix_timestamp(1_750_000_200)?;
    let guild_id = GuildId::parse("200000000000000001")?;
    for revision in 1_i64..=4 {
        sqlx::query(
            "INSERT INTO outbox_events (
                id, guild_id, topic, aggregate_type, aggregate_id, aggregate_revision,
                event_type, payload, occurred_at, available_at
             ) VALUES ($1, $2, 'series:test', 'series', $3, $4,
                       'snapshot_invalidated', $5, $6, $6)",
        )
        .bind(Uuid::now_v7())
        .bind(&guild_id)
        .bind(format!("series-{revision}"))
        .bind(revision)
        .bind(json!({ "revision": revision }))
        .bind(now)
        .execute(pool)
        .await?;
    }

    let barrier = std::sync::Arc::new(Barrier::new(3));
    let mut workers = Vec::new();
    for owner in ["relay-a", "relay-b"] {
        let pool = pool.clone();
        let guild_id = guild_id.clone();
        let barrier = barrier.clone();
        workers.push(tokio::spawn(async move {
            let mut transaction = pool.begin().await?;
            barrier.wait().await;
            let events = outbox::claim(
                &mut transaction,
                &ClaimOutbox {
                    guild_id: &guild_id,
                    lease_owner: owner,
                    lease_duration: Duration::from_millis(50),
                    limit: 2,
                },
            )
            .await?;
            transaction.commit().await?;
            Ok::<_, Box<dyn Error + Send + Sync>>(events)
        }));
    }
    barrier.wait().await;
    let mut first_claims = Vec::new();
    for worker in workers {
        first_claims.extend(worker.await??);
    }
    first_claims.sort_by_key(|event| event.id.as_uuid());
    first_claims.dedup_by_key(|event| event.id);
    assert_eq!(first_claims.len(), 4, "claims must be disjoint");

    tokio::time::sleep(Duration::from_millis(100)).await;
    let reclaim_time = now + time::Duration::seconds(2);
    let mut transaction = pool.begin().await?;
    let reclaimed = outbox::claim(
        &mut transaction,
        &ClaimOutbox {
            guild_id: &guild_id,
            lease_owner: "relay-reclaimer",
            lease_duration: Duration::from_secs(30),
            limit: 1,
        },
    )
    .await?;
    transaction.commit().await?;
    assert_eq!(reclaimed.len(), 1);
    let reclaimed = &reclaimed[0];
    let stale = first_claims
        .iter()
        .find(|event| event.id == reclaimed.id)
        .expect("reclaimed event must have an earlier lease");

    let mut transaction = pool.begin().await?;
    let stale_ack = outbox::ack(
        &mut transaction,
        &AckOutbox {
            guild_id: &guild_id,
            event_id: stale.id,
            lease_owner: &stale.lease.owner,
            lease_token: stale.lease.token,
            published_at: reclaim_time,
        },
    )
    .await?;
    assert_eq!(stale_ack, 0, "stale lease must not acknowledge a new claim");
    let fresh_ack = outbox::ack(
        &mut transaction,
        &AckOutbox {
            guild_id: &guild_id,
            event_id: reclaimed.id,
            lease_owner: &reclaimed.lease.owner,
            lease_token: reclaimed.lease.token,
            published_at: reclaim_time,
        },
    )
    .await?;
    assert_eq!(fresh_ack, 1);
    transaction.commit().await?;
    Ok(())
}

async fn assert_outbox_uses_database_clock(pool: &PgPool) -> TestResult {
    let guild_id = GuildId::parse("200000000000000002")?;
    let event_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO outbox_events (
            id, guild_id, topic, aggregate_type, aggregate_id, aggregate_revision,
            event_type, payload, occurred_at, available_at
         ) VALUES ($1, $2, 'series:test', 'series', 'database-clock', 1,
                   'snapshot_invalidated', '{}', clock_timestamp(),
                   clock_timestamp() + INTERVAL '1 hour')",
    )
    .bind(event_id)
    .bind(&guild_id)
    .execute(pool)
    .await?;

    let mut transaction = pool.begin().await?;
    let future_claim = outbox::claim(
        &mut transaction,
        &ClaimOutbox {
            guild_id: &guild_id,
            lease_owner: "relay-database-clock",
            lease_duration: Duration::from_secs(30),
            limit: 1,
        },
    )
    .await?;
    transaction.commit().await?;
    assert!(future_claim.is_empty(), "future event must not be claimed");

    sqlx::query(
        "UPDATE outbox_events
         SET available_at = clock_timestamp() - INTERVAL '1 microsecond'
         WHERE guild_id = $1 AND id = $2",
    )
    .bind(&guild_id)
    .bind(event_id)
    .execute(pool)
    .await?;
    let before_claim = sqlx::query_scalar::<_, OffsetDateTime>("SELECT clock_timestamp()")
        .fetch_one(pool)
        .await?;

    let mut transaction = pool.begin().await?;
    let ready_claim = outbox::claim(
        &mut transaction,
        &ClaimOutbox {
            guild_id: &guild_id,
            lease_owner: "relay-database-clock",
            lease_duration: Duration::from_secs(30),
            limit: 1,
        },
    )
    .await?;
    transaction.commit().await?;
    let after_claim = sqlx::query_scalar::<_, OffsetDateTime>("SELECT clock_timestamp()")
        .fetch_one(pool)
        .await?;

    assert_eq!(ready_claim.len(), 1);
    assert_eq!(ready_claim[0].id.as_uuid(), event_id);
    assert!(ready_claim[0].lease.until >= before_claim + time::Duration::seconds(30));
    assert!(ready_claim[0].lease.until <= after_claim + time::Duration::seconds(30));

    let before_nack = sqlx::query_scalar::<_, OffsetDateTime>("SELECT clock_timestamp()")
        .fetch_one(pool)
        .await?;
    let mut transaction = pool.begin().await?;
    let nacked = outbox::nack(
        &mut transaction,
        &NackOutbox {
            guild_id: &guild_id,
            event_id: ready_claim[0].id,
            lease_owner: &ready_claim[0].lease.owner,
            lease_token: ready_claim[0].lease.token,
            retry_delay: Duration::from_secs(30),
            last_error: "Valkey unavailable",
        },
    )
    .await?;
    assert_eq!(nacked, 1);
    transaction.commit().await?;
    let after_nack = sqlx::query_scalar::<_, OffsetDateTime>("SELECT clock_timestamp()")
        .fetch_one(pool)
        .await?;
    let nacked_row = sqlx::query_as::<
        _,
        (
            OffsetDateTime,
            i32,
            Option<String>,
            Option<Uuid>,
            Option<OffsetDateTime>,
            Option<String>,
        ),
    >(
        "SELECT available_at, attempts, lease_owner, lease_token, lease_until, last_error
         FROM outbox_events
         WHERE guild_id = $1 AND id = $2",
    )
    .bind(&guild_id)
    .bind(event_id)
    .fetch_one(pool)
    .await?;
    assert!(nacked_row.0 >= before_nack + time::Duration::seconds(30));
    assert!(nacked_row.0 <= after_nack + time::Duration::seconds(30));
    assert_eq!(nacked_row.1, 1);
    assert_eq!(nacked_row.2, None);
    assert_eq!(nacked_row.3, None);
    assert_eq!(nacked_row.4, None);
    assert_eq!(nacked_row.5.as_deref(), Some("Valkey unavailable"));

    let mut transaction = pool.begin().await?;
    let early_retry = outbox::claim(
        &mut transaction,
        &ClaimOutbox {
            guild_id: &guild_id,
            lease_owner: "relay-too-early",
            lease_duration: Duration::from_secs(30),
            limit: 1,
        },
    )
    .await?;
    transaction.rollback().await?;
    assert!(
        early_retry.is_empty(),
        "nacked event was immediately available"
    );
    Ok(())
}

async fn assert_concurrent_mmr_initialization(pool: &PgPool) -> TestResult {
    let now = OffsetDateTime::from_unix_timestamp(1_750_000_300)?;
    let guild_id = GuildId::parse("200000000000000001")?;
    let season_id = SeasonId::from_uuid(
        sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM seasons WHERE guild_id = $1 AND status = 'ACTIVE'",
        )
        .bind(&guild_id)
        .fetch_one(pool)
        .await?,
    );
    let user_id = DiscordUserId::parse("100000000000000001")?;
    let key = MmrLaneKey {
        user_id: user_id.into(),
        lane: Lane::Mid,
    };
    let barrier = std::sync::Arc::new(Barrier::new(3));
    let mut workers = Vec::new();
    for _ in 0..2 {
        let pool = pool.clone();
        let guild_id = guild_id.clone();
        let key = key.clone();
        let barrier = barrier.clone();
        workers.push(tokio::spawn(async move {
            let mut transaction = pool.begin().await?;
            barrier.wait().await;
            let rows = ensure_and_lock_mmr_rows(&mut transaction, &guild_id, season_id, [key], now)
                .await?;
            transaction.commit().await?;
            Ok::<_, Box<dyn Error + Send + Sync>>(rows)
        }));
    }
    barrier.wait().await;
    for worker in workers {
        let rows = worker.await??;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].mmr, 1500.0);
        assert_eq!(rows[0].revision, 0);
    }

    let row_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM user_lane_mmr
         WHERE guild_id = $1 AND season_id = $2 AND user_id = $3 AND lane = 'MID'",
    )
    .bind(&guild_id)
    .bind(season_id)
    .bind(&key.user_id)
    .fetch_one(pool)
    .await?;
    assert_eq!(row_count, 1);
    Ok(())
}

fn constraint_name(error: &sqlx::Error) -> Option<&str> {
    error
        .as_database_error()
        .and_then(sqlx::error::DatabaseError::constraint)
}

async fn create_database(admin: &PgPool, database_name: &str) -> TestResult {
    assert!(
        database_name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
    );
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "CREATE DATABASE {database_name}"
    )))
    .execute(admin)
    .await?;
    Ok(())
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
            .with_env_var("POSTGRES_DB", "postgres")
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

    fn database_url(&self, database_name: &str) -> String {
        format!(
            "postgresql://postgres:{POSTGRES_PASSWORD}@{}:{}/{database_name}",
            self.host, self.port
        )
    }
}
