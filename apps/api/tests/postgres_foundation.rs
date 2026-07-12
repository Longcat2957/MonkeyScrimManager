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
    assert_fk_indexes_cover_child_columns(&full).await?;
    assert_constraints_and_tenant_isolation(&full).await?;
    assert_idempotency_contract(&full).await?;
    assert_concurrent_idempotency_contract(&full).await?;
    assert_structured_idempotency_lock_key(&full).await?;
    assert_outbox_uses_database_clock(&full).await?;
    assert_outbox_claim_and_stale_ack(&full).await?;
    assert_concurrent_mmr_initialization(&full).await?;

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
    MIGRATOR.run_to(8, &n_minus_one).await?;
    let snapshot = PostgresReadinessProbe::new(n_minus_one.clone(), Duration::from_secs(1))
        .check()
        .await;
    assert_eq!(snapshot.postgres, ComponentStatus::Available);
    assert_eq!(snapshot.migration, ComponentStatus::Unavailable);

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
    assert_eq!(table_count, 51, "unexpected public table count");
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

    let invalid_user = sqlx::query(
        "INSERT INTO users (discord_id, display_name, created_at, updated_at)
         VALUES ('01', 'invalid', clock_timestamp(), clock_timestamp())",
    )
    .execute(pool)
    .await
    .expect_err("users.discord_id must enforce the snowflake constraint");
    assert_eq!(
        constraint_name(&invalid_user),
        Some("users_discord_id_check")
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
        "INSERT INTO users (discord_id, display_name, created_at, updated_at)
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
        user_id,
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
