use std::{collections::HashMap, error::Error, sync::Arc, time::Duration};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, Response, StatusCode, header},
};
use futures_util::future::BoxFuture;
use monkeybot_api::{
    AppState,
    auth::{AuthService, CookiePolicy},
    build_router,
    config::Config,
    domain::DiscordUserId,
    infrastructure::postgres::{MIGRATOR, PostgresReadinessProbe},
    integrations::discord::{DiscordClient, DiscordClientError, DiscordUser},
    repository::auth::{AuthRepository, NewLogin},
    state::AuthRuntime,
};
use reqwest::Url;
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use testcontainers::{
    ContainerAsync, GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use time::OffsetDateTime;
use tower::ServiceExt;
use uuid::Uuid;

const POSTGRES_IMAGE: &str = "postgres";
const POSTGRES_TAG: &str = "18.4-alpine3.23";
const POSTGRES_PASSWORD: &str = "monkeybot-audit-log-endpoint-password";
const DATABASE: &str = "monkeybot_audit_log_endpoints";
const OPERATOR_ID: &str = "100000000000000001";
const MEMBER_ID: &str = "100000000000000002";
const FALLBACK_USER_ID: &str = "100000000000000003";
const GUILD_ID: &str = "200000000000000001";
const OTHER_GUILD_ID: &str = "200000000000000002";
const OPERATOR_ROLE_ID: &str = "300000000000000001";
const OTHER_OPERATOR_ROLE_ID: &str = "300000000000000002";
const DISCORD_CLIENT_ID: &str = "900000000000000001";
const PUBLIC_ORIGIN: &str = "http://localhost:5173";
const OAUTH_KEY_BASE64URL: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const OPERATOR_SESSION: &str = "audit-log-operator-session";
const OPERATOR_CSRF: &str = "audit-log-operator-csrf";
const MEMBER_SESSION: &str = "audit-log-member-session";
const MEMBER_CSRF: &str = "audit-log-member-csrf";
const SEASON_ID: &str = "01900000-0000-7000-8000-000000000011";
const OTHER_SEASON_ID: &str = "01900000-0000-7000-8000-000000000012";
const RECRUITMENT_ID: &str = "01900000-0000-7000-8000-000000000021";
const CHECKPOINT_ID: &str = "01900000-0000-7000-8000-000000000031";
const AUDIT_GUILD_ID: &str = "01900000-0000-7000-8000-000000000101";
const AUDIT_CROSS_TENANT_ID: &str = "01900000-0000-7000-8000-000000000102";
const AUDIT_RECRUITMENT_ID: &str = "01900000-0000-7000-8000-000000000103";
const AUDIT_SEASON_ID: &str = "01900000-0000-7000-8000-000000000104";
const AUDIT_CONFIG_ID: &str = "01900000-0000-7000-8000-000000000105";
const AUDIT_MEMBER_MMR_ID: &str = "01900000-0000-7000-8000-000000000106";
const OTHER_AUDIT_ID: &str = "01900000-0000-7000-8000-000000000107";

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn guild_audit_logs_authorize_project_and_paginate_without_tenant_leaks() -> TestResult {
    tokio::time::timeout(Duration::from_secs(120), run_flow())
        .await
        .map_err(|_| "guild audit-log endpoint test exceeded 120 seconds")??;
    Ok(())
}

async fn run_flow() -> TestResult {
    let server = TestPostgres::start().await?;
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&server.database_url())
        .await?;
    MIGRATOR.run(&pool).await?;

    let config = test_config(server.database_url())?;
    let auth_config = config.auth().cloned().expect("test auth config");
    let auth_service = Arc::new(AuthService::new(
        auth_config.oauth_encryption_key(),
        CookiePolicy::new(auth_config.secure_cookies(), auth_config.session_ttl()),
    )?);
    let auth_repository = Arc::new(AuthRepository::new(pool.clone()));
    create_login(
        &auth_repository,
        OPERATOR_ID,
        "Operator Account",
        OPERATOR_SESSION,
        OPERATOR_CSRF,
    )
    .await?;
    create_login(
        &auth_repository,
        MEMBER_ID,
        "Member Account",
        MEMBER_SESSION,
        MEMBER_CSRF,
    )
    .await?;
    seed_data(&pool).await?;

    let state = AppState::with_postgres(
        config,
        pool.clone(),
        Arc::new(PostgresReadinessProbe::new(
            pool.clone(),
            Duration::from_secs(1),
        )),
    )
    .with_auth(AuthRuntime::new(
        auth_service,
        auth_repository,
        Arc::new(FakeDiscordClient),
    ));
    let app = build_router(state);
    let path = format!("/api/guilds/{GUILD_ID}/audit-logs");

    let unauthenticated = app
        .clone()
        .oneshot(Request::get(&path).body(Body::empty())?)
        .await?;
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);

    let forbidden = browser_get(&app, &path, MEMBER_SESSION).await?;
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        body_json(forbidden).await?["error"]["code"],
        "operator_required"
    );

    let inaccessible = browser_get(
        &app,
        &format!("/api/guilds/{OTHER_GUILD_ID}/audit-logs"),
        OPERATOR_SESSION,
    )
    .await?;
    assert_eq!(inaccessible.status(), StatusCode::NOT_FOUND);

    let response = browser_get(&app, &path, OPERATOR_SESSION).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let response = body_json(response).await?;
    let entries = response["data"]["entries"]
        .as_array()
        .ok_or("audit entries missing")?;
    assert_eq!(entries.len(), 6);
    assert_eq!(response["data"]["nextCursor"], Value::Null);
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry["id"].as_str())
            .collect::<Vec<_>>(),
        vec![
            Some(AUDIT_MEMBER_MMR_ID),
            Some(AUDIT_CONFIG_ID),
            Some(AUDIT_SEASON_ID),
            Some(AUDIT_RECRUITMENT_ID),
            Some(AUDIT_CROSS_TENANT_ID),
            Some(AUDIT_GUILD_ID),
        ]
    );
    assert!(entries.iter().all(|entry| entry["id"] != OTHER_AUDIT_ID));
    assert_exact_entry_fields(&entries[0])?;

    assert_eq!(entries[0]["principalType"], "USER");
    assert_eq!(entries[0]["principalId"], OPERATOR_ID);
    assert_eq!(entries[0]["principalDisplayName"], "Guild Operator");
    assert_eq!(entries[0]["targetType"], "GUILD_MEMBER_MMR");
    assert_eq!(entries[0]["targetId"], MEMBER_ID);
    assert_eq!(entries[0]["targetDisplayName"], "Guild Member");
    assert_eq!(entries[0]["beforeSummary"], json!({"mmr": 1500}));
    assert_eq!(entries[0]["afterSummary"], json!({"mmr": 1520}));
    assert_eq!(entries[0]["result"], "APPLIED");
    assert_eq!(entries[0]["note"], "수동 조정");

    assert_eq!(entries[1]["principalType"], "BOT");
    assert_eq!(entries[1]["principalDisplayName"], Value::Null);
    assert_eq!(entries[1]["targetDisplayName"], "Audit Guild");
    assert_eq!(entries[2]["principalType"], "SYSTEM");
    assert_eq!(entries[2]["targetDisplayName"], "2026 시즌");
    assert_eq!(entries[2]["checkpointId"], CHECKPOINT_ID);
    assert_eq!(entries[2]["result"], "REVERTED");
    assert_eq!(entries[3]["principalType"], "MIGRATION");
    assert_eq!(entries[3]["targetDisplayName"], "금요일 내전 모집");

    // This USER is only a member of another tenant, so the current guild member join
    // must not leak that guild's display name. The global user fallback remains valid.
    assert_eq!(entries[4]["principalDisplayName"], "Global Fallback");
    assert_eq!(entries[4]["targetDisplayName"], Value::Null);
    assert_eq!(entries[5]["targetDisplayName"], "Audit Guild");

    assert_pagination(&app, &path).await?;
    assert_invalid_queries(&app, &path).await?;

    pool.close().await;
    drop(server);
    Ok(())
}

async fn assert_pagination(app: &Router, path: &str) -> TestResult {
    let first =
        body_json(browser_get(app, &format!("{path}?limit=1"), OPERATOR_SESSION).await?).await?;
    let first_entries = first["data"]["entries"]
        .as_array()
        .ok_or("first page entries missing")?;
    assert_eq!(first_entries.len(), 1);
    assert_eq!(first_entries[0]["id"], AUDIT_MEMBER_MMR_ID);
    let first_cursor = first["data"]["nextCursor"]
        .as_str()
        .ok_or("first page cursor missing")?;

    let second = body_json(
        browser_get(
            app,
            &format!("{path}?limit=1&cursor={first_cursor}"),
            OPERATOR_SESSION,
        )
        .await?,
    )
    .await?;
    let second_entries = second["data"]["entries"]
        .as_array()
        .ok_or("second page entries missing")?;
    assert_eq!(second_entries.len(), 1);
    // These first two rows share a timestamp. UUID ordering must carry the
    // second row onto this page without skipping or duplicating it.
    assert_eq!(second_entries[0]["id"], AUDIT_CONFIG_ID);
    let second_cursor = second["data"]["nextCursor"]
        .as_str()
        .ok_or("second page cursor missing")?;

    let third = body_json(
        browser_get(
            app,
            &format!("{path}?limit=100&cursor={second_cursor}"),
            OPERATOR_SESSION,
        )
        .await?,
    )
    .await?;
    let third_entries = third["data"]["entries"]
        .as_array()
        .ok_or("third page entries missing")?;
    assert_eq!(third_entries.len(), 4);
    assert_eq!(third_entries[0]["id"], AUDIT_SEASON_ID);
    assert_eq!(third_entries[1]["id"], AUDIT_RECRUITMENT_ID);
    assert_eq!(third_entries[2]["id"], AUDIT_CROSS_TENANT_ID);
    assert_eq!(third_entries[3]["id"], AUDIT_GUILD_ID);
    assert_eq!(third["data"]["nextCursor"], Value::Null);
    Ok(())
}

async fn assert_invalid_queries(app: &Router, path: &str) -> TestResult {
    for limit in ["0", "101", "-1", "text", "65536"] {
        let response = browser_get(app, &format!("{path}?limit={limit}"), OPERATOR_SESSION).await?;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body_json(response).await?["error"]["code"], "invalid_limit");
    }
    for cursor in ["not-base64!", "e30", ""] {
        let response =
            browser_get(app, &format!("{path}?cursor={cursor}"), OPERATOR_SESSION).await?;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            body_json(response).await?["error"]["code"],
            "invalid_cursor"
        );
    }
    Ok(())
}

fn assert_exact_entry_fields(entry: &Value) -> TestResult {
    let object = entry.as_object().ok_or("audit entry is not an object")?;
    let mut actual = object.keys().map(String::as_str).collect::<Vec<_>>();
    actual.sort_unstable();
    let mut expected = vec![
        "action",
        "afterSummary",
        "beforeSummary",
        "checkpointId",
        "createdAt",
        "id",
        "note",
        "principalDisplayName",
        "principalId",
        "principalType",
        "requestId",
        "result",
        "targetDisplayName",
        "targetId",
        "targetType",
    ];
    expected.sort_unstable();
    assert_eq!(actual, expected);
    assert!(entry["createdAt"].as_str().is_some());
    Ok(())
}

async fn browser_get(app: &Router, path: &str, session: &str) -> TestResult<Response<Body>> {
    Ok(app
        .clone()
        .oneshot(
            Request::get(path)
                .header(header::COOKIE, format!("monkeybot_session={session}"))
                .body(Body::empty())?,
        )
        .await?)
}

async fn body_json(response: Response<Body>) -> TestResult<Value> {
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

async fn create_login(
    repository: &AuthRepository,
    user_id: &str,
    display_name: &str,
    session_token: &str,
    csrf_token: &str,
) -> TestResult {
    repository
        .complete_login(&NewLogin {
            user_id: DiscordUserId::parse(user_id)?,
            display_name: display_name.to_owned(),
            avatar_hash: None,
            previous_session_token_hash: None,
            session_id: Uuid::now_v7(),
            token_hash: AuthService::hash_secret(session_token),
            csrf_secret_hash: AuthService::hash_secret(csrf_token),
            expires_at: OffsetDateTime::now_utc() + time::Duration::hours(1),
        })
        .await?;
    Ok(())
}

async fn seed_data(pool: &PgPool) -> TestResult {
    let now = OffsetDateTime::now_utc();
    sqlx::query(
        "INSERT INTO users (
             discord_id, display_name, avatar_hash, created_at, updated_at, deleted_at
         ) VALUES ($1, 'Global Fallback', NULL, $2, $2, NULL)",
    )
    .bind(FALLBACK_USER_ID)
    .bind(now)
    .execute(pool)
    .await?;

    for (guild_id, name, installed_by) in [
        (GUILD_ID, "Audit Guild Discord", OPERATOR_ID),
        (OTHER_GUILD_ID, "Other Guild Discord", FALLBACK_USER_ID),
    ] {
        sqlx::query(
            "INSERT INTO guilds (
                 guild_id, discord_name, icon_hash, status, installed_by,
                 installed_at, activated_at, left_at, updated_at
             ) VALUES ($1, $2, NULL, 'ACTIVE', $3, $4, $4, NULL, $4)",
        )
        .bind(guild_id)
        .bind(name)
        .bind(installed_by)
        .bind(now)
        .execute(pool)
        .await?;
    }

    for (guild_id, user_id, display_name) in [
        (GUILD_ID, OPERATOR_ID, "Guild Operator"),
        (GUILD_ID, MEMBER_ID, "Guild Member"),
        (OTHER_GUILD_ID, FALLBACK_USER_ID, "Other Guild Member"),
    ] {
        sqlx::query(
            "INSERT INTO guild_members (
                 guild_id, user_id, display_name, status, main_lane, first_seen_at,
                 registered_at, discord_joined_at, last_active_at, last_synced_at, inactivated_at
             ) VALUES ($1, $2, $3, 'ACTIVE', NULL, $4, $4, $4, $4, $4, NULL)",
        )
        .bind(guild_id)
        .bind(user_id)
        .bind(display_name)
        .bind(now)
        .execute(pool)
        .await?;
    }

    for (guild_id, name, role_id, updated_by) in [
        (GUILD_ID, "Audit Guild", OPERATOR_ROLE_ID, OPERATOR_ID),
        (
            OTHER_GUILD_ID,
            "Other Guild",
            OTHER_OPERATOR_ROLE_ID,
            FALLBACK_USER_ID,
        ),
    ] {
        sqlx::query(
            "INSERT INTO guild_config (
                 guild_id, display_name, operator_role_id, recruitment_channel_id,
                 entry_channel_id, log_channel_id, is_externally_public, revision,
                 updated_by, updated_at
             ) VALUES ($1, $2, $3, NULL, NULL, NULL, false, 0, $4, $5)",
        )
        .bind(guild_id)
        .bind(name)
        .bind(role_id)
        .bind(updated_by)
        .bind(now)
        .execute(pool)
        .await?;
    }

    sqlx::query(
        "INSERT INTO guild_member_discord_roles (guild_id, user_id, role_id, last_synced_at)
         VALUES ($1, $2, $3, $4), ($5, $6, $7, $4)",
    )
    .bind(GUILD_ID)
    .bind(OPERATOR_ID)
    .bind(OPERATOR_ROLE_ID)
    .bind(now)
    .bind(OTHER_GUILD_ID)
    .bind(FALLBACK_USER_ID)
    .bind(OTHER_OPERATOR_ROLE_ID)
    .execute(pool)
    .await?;

    sqlx::query(
        "INSERT INTO seasons (
             guild_id, id, name, status, started_at, ended_at, created_by, created_at
         ) VALUES
           ($1, $2, '2026 시즌', 'ACTIVE', $3, NULL, $4, $3),
           ($5, $6, '다른 길드 시즌', 'ACTIVE', $3, NULL, $7, $3)",
    )
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(SEASON_ID)?)
    .bind(now - time::Duration::days(30))
    .bind(OPERATOR_ID)
    .bind(OTHER_GUILD_ID)
    .bind(Uuid::parse_str(OTHER_SEASON_ID)?)
    .bind(FALLBACK_USER_ID)
    .execute(pool)
    .await?;

    sqlx::query(
        "INSERT INTO recruitments (
             guild_id, id, season_id, display_number, title, target_count, status, revision,
             channel_id, message_id, created_by, created_at, updated_by, updated_at,
             closed_at, converted_at, cancelled_at
         ) VALUES ($1, $2, $3, 1, '금요일 내전 모집', 10, 'OPEN', 0,
                   NULL, NULL, $4, $5, $4, $5, NULL, NULL, NULL)",
    )
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(RECRUITMENT_ID)?)
    .bind(Uuid::parse_str(SEASON_ID)?)
    .bind(OPERATOR_ID)
    .bind(now)
    .execute(pool)
    .await?;

    sqlx::query(
        "INSERT INTO revert_checkpoints (
             guild_id, id, target_type, target_id, target_revision, action, schema_version,
             restore_payload, status, created_by_type, created_by, created_at,
             reverted_by_type, reverted_by, reverted_at
         ) VALUES ($1, $2, 'SEASON', $3, 1, 'TEST_REVERT', 1, $4,
                   'REVERTED', 'SYSTEM', 'system', $5, 'SYSTEM', 'system', $5)",
    )
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(CHECKPOINT_ID)?)
    .bind(SEASON_ID)
    .bind(json!({"seasonId": SEASON_ID}))
    .bind(now)
    .execute(pool)
    .await?;

    let newest = now - time::Duration::minutes(1);
    insert_audit(
        pool,
        GUILD_ID,
        AUDIT_MEMBER_MMR_ID,
        "USER",
        OPERATOR_ID,
        "MEMBER_MMR_ADJUSTED",
        Some("GUILD_MEMBER_MMR"),
        Some(MEMBER_ID),
        None,
        Some("request-mmr"),
        Some(json!({"mmr": 1500})),
        Some(json!({"mmr": 1520})),
        "APPLIED",
        Some("수동 조정"),
        newest,
    )
    .await?;
    insert_audit(
        pool,
        GUILD_ID,
        AUDIT_CONFIG_ID,
        "BOT",
        "monkeybot",
        "GUILD_SETTINGS_SYNCED",
        Some("GUILD_CONFIG"),
        Some(GUILD_ID),
        None,
        None,
        None,
        None,
        "APPLIED",
        None,
        newest,
    )
    .await?;
    insert_audit(
        pool,
        GUILD_ID,
        AUDIT_SEASON_ID,
        "SYSTEM",
        "system",
        "SEASON_REVERTED",
        Some("SEASON"),
        Some(SEASON_ID),
        Some(CHECKPOINT_ID),
        Some("request-revert"),
        None,
        None,
        "REVERTED",
        None,
        newest - time::Duration::seconds(1),
    )
    .await?;
    insert_audit(
        pool,
        GUILD_ID,
        AUDIT_RECRUITMENT_ID,
        "MIGRATION",
        "legacy-import",
        "RECRUITMENT_IMPORTED",
        Some("RECRUITMENT"),
        Some(RECRUITMENT_ID),
        None,
        None,
        None,
        None,
        "APPLIED",
        None,
        newest - time::Duration::seconds(2),
    )
    .await?;
    insert_audit(
        pool,
        GUILD_ID,
        AUDIT_CROSS_TENANT_ID,
        "USER",
        FALLBACK_USER_ID,
        "CROSS_TENANT_PROJECTION_TEST",
        Some("GUILD_MEMBER"),
        Some(FALLBACK_USER_ID),
        None,
        None,
        None,
        None,
        "APPLIED",
        None,
        newest - time::Duration::seconds(3),
    )
    .await?;
    insert_audit(
        pool,
        GUILD_ID,
        AUDIT_GUILD_ID,
        "USER",
        OPERATOR_ID,
        "GUILD_ACTIVATED",
        Some("GUILD"),
        Some(GUILD_ID),
        None,
        None,
        None,
        None,
        "APPLIED",
        None,
        newest - time::Duration::seconds(4),
    )
    .await?;
    insert_audit(
        pool,
        OTHER_GUILD_ID,
        OTHER_AUDIT_ID,
        "USER",
        FALLBACK_USER_ID,
        "OTHER_GUILD_ACTION",
        Some("GUILD"),
        Some(OTHER_GUILD_ID),
        None,
        None,
        None,
        None,
        "APPLIED",
        None,
        newest + time::Duration::hours(1),
    )
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_audit(
    pool: &PgPool,
    guild_id: &str,
    id: &str,
    principal_type: &str,
    principal_id: &str,
    action: &str,
    target_type: Option<&str>,
    target_id: Option<&str>,
    checkpoint_id: Option<&str>,
    request_id: Option<&str>,
    before_summary: Option<Value>,
    after_summary: Option<Value>,
    result: &str,
    note: Option<&str>,
    created_at: OffsetDateTime,
) -> TestResult {
    sqlx::query(
        "INSERT INTO admin_audit_log (
             guild_id, id, principal_type, principal_id, action, target_type, target_id,
             checkpoint_id, request_id, before_summary, after_summary, result, note, created_at
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
    )
    .bind(guild_id)
    .bind(Uuid::parse_str(id)?)
    .bind(principal_type)
    .bind(principal_id)
    .bind(action)
    .bind(target_type)
    .bind(target_id)
    .bind(checkpoint_id.map(Uuid::parse_str).transpose()?)
    .bind(request_id)
    .bind(before_summary)
    .bind(after_summary)
    .bind(result)
    .bind(note)
    .bind(created_at)
    .execute(pool)
    .await?;
    Ok(())
}

fn test_config(database_url: String) -> Result<Config, Box<dyn Error + Send + Sync>> {
    let values = HashMap::from([
        ("MONKEYBOT_ENV".to_owned(), "test".to_owned()),
        (
            "MONKEYBOT_BIND_ADDR".to_owned(),
            "127.0.0.1:30020".to_owned(),
        ),
        (
            "MONKEYBOT_PUBLIC_ORIGIN".to_owned(),
            PUBLIC_ORIGIN.to_owned(),
        ),
        ("MONKEYBOT_DATABASE_URL".to_owned(), database_url),
        (
            "MONKEYBOT_DATABASE_MAX_CONNECTIONS".to_owned(),
            "8".to_owned(),
        ),
        (
            "MONKEYBOT_DATABASE_MIN_CONNECTIONS".to_owned(),
            "1".to_owned(),
        ),
        (
            "MONKEYBOT_DISCORD_CLIENT_ID".to_owned(),
            DISCORD_CLIENT_ID.to_owned(),
        ),
        (
            "MONKEYBOT_DISCORD_CLIENT_SECRET".to_owned(),
            "fake-discord-secret".to_owned(),
        ),
        (
            "MONKEYBOT_OAUTH_ENCRYPTION_KEY".to_owned(),
            OAUTH_KEY_BASE64URL.to_owned(),
        ),
        (
            "MONKEYBOT_SESSION_TTL_SECONDS".to_owned(),
            "3600".to_owned(),
        ),
    ]);
    Ok(Config::from_map(&values)?)
}

#[derive(Debug)]
struct FakeDiscordClient;

impl DiscordClient for FakeDiscordClient {
    fn authorization_url(
        &self,
        _state: &str,
        _pkce_challenge: &str,
    ) -> Result<Url, DiscordClientError> {
        Err(DiscordClientError::InvalidConfiguration)
    }

    fn authenticate<'a>(
        &'a self,
        _code: &'a str,
        _pkce_verifier: &'a SecretString,
    ) -> BoxFuture<'a, Result<DiscordUser, DiscordClientError>> {
        Box::pin(async { Err(DiscordClientError::InvalidConfiguration) })
    }
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
