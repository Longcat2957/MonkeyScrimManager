use std::{collections::HashMap, error::Error, sync::Arc, time::Duration};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, Response, StatusCode, header},
};
use futures_util::future::BoxFuture;
use monkeybot_api::{
    auth::{AuthService, CookiePolicy},
    build_router,
    config::Config,
    domain::DiscordUserId,
    infrastructure::postgres::{MIGRATOR, PostgresReadinessProbe},
    integrations::discord::{DiscordClient, DiscordClientError, DiscordUser},
    repository::auth::{AuthRepository, NewLogin},
    state::{AppState, AuthRuntime},
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
const POSTGRES_PASSWORD: &str = "monkeybot-admin-member-endpoint-password";
const DATABASE: &str = "monkeybot_admin_member_endpoints";
const OPERATOR_ID: &str = "100000000000000001";
const MEMBER_ID: &str = "100000000000000002";
const GUILD_ID: &str = "200000000000000001";
const OPERATOR_ROLE_ID: &str = "300000000000000001";
const DISCORD_CLIENT_ID: &str = "900000000000000001";
const PUBLIC_ORIGIN: &str = "http://localhost:5173";
const OAUTH_KEY_BASE64URL: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const OPERATOR_SESSION: &str = "admin-member-operator-session";
const OPERATOR_CSRF: &str = "admin-member-operator-csrf";
const MEMBER_SESSION: &str = "admin-member-ordinary-session";
const MEMBER_CSRF: &str = "admin-member-ordinary-csrf";
const COMMAND_KEY: &str = "01900000-0000-7000-8000-000000000001";

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn guild_admin_members_are_revisioned_audited_and_idempotent() -> TestResult {
    tokio::time::timeout(Duration::from_secs(120), run_flow())
        .await
        .map_err(|_| "guild admin member endpoint test exceeded 120 seconds")??;
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
        "Operator",
        OPERATOR_SESSION,
        OPERATOR_CSRF,
    )
    .await?;
    create_login(
        &auth_repository,
        MEMBER_ID,
        "NoLane",
        MEMBER_SESSION,
        MEMBER_CSRF,
    )
    .await?;
    seed_guild(&pool).await?;

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
    let list_path = format!("/api/guilds/{GUILD_ID}/members");
    let mmr_path = format!("{list_path}/{MEMBER_ID}/mmr");
    let status_path = format!("{list_path}/{MEMBER_ID}/status");
    let self_status_path = format!("{list_path}/{OPERATOR_ID}/status");

    let unauthenticated = app
        .clone()
        .oneshot(Request::get(&list_path).body(Body::empty())?)
        .await?;
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);

    let forbidden = send(
        &app,
        Method::GET,
        &list_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        None,
        false,
        None,
    )
    .await?;
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        body_json(forbidden).await?["error"]["code"],
        "operator_required"
    );

    let initial = operator_get(&app, &list_path).await?;
    assert_eq!(initial.status(), StatusCode::OK);
    let initial = body_json(initial).await?;
    assert_eq!(initial["data"]["revision"], 0);
    let member = find_member(&initial)?;
    assert_eq!(member["identityKind"], "DISCORD");
    assert_eq!(member["riotId"], "NoLaneMain#KR1");
    assert_eq!(member["role"], "MEMBER");
    assert_eq!(member["mainLane"], Value::Null);
    assert!(member["lastActiveAt"].is_string());
    assert_eq!(
        member["laneMmr"],
        json!({
            "TOP": 1500.0,
            "JUNGLE": 1500.0,
            "MID": 1500.0,
            "BOTTOM": 1500.0,
            "SUPPORT": 1500.0,
        })
    );
    assert_eq!(admin_state_count(&pool).await?, 0);

    let self_deactivation = mutation(
        &app,
        &self_status_path,
        json!({"status":"INACTIVE","expectedRevision":0}),
        None,
    )
    .await?;
    assert_eq!(self_deactivation.status(), StatusCode::CONFLICT);
    assert_eq!(
        body_json(self_deactivation).await?["error"]["code"],
        "self_deactivation_forbidden"
    );
    assert_eq!(admin_state_count(&pool).await?, 0);
    assert_eq!(member_status(&pool, OPERATOR_ID).await?, "ACTIVE");
    assert_eq!(audit_count(&pool).await?, 0);
    assert_eq!(
        operator_get(&app, &list_path).await?.status(),
        StatusCode::OK
    );

    let no_csrf = send(
        &app,
        Method::PATCH,
        &mmr_path,
        OPERATOR_SESSION,
        OPERATOR_CSRF,
        Some(mmr_body(0, 25.5)),
        false,
        Some(COMMAND_KEY),
    )
    .await?;
    assert_eq!(no_csrf.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        body_json(no_csrf).await?["error"]["code"],
        "csrf_validation_failed"
    );

    let missing_key = mutation(&app, &mmr_path, mmr_body(0, 25.5), None).await?;
    assert_eq!(missing_key.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        body_json(missing_key).await?["error"]["code"],
        "invalid_idempotency_key"
    );

    let unknown_field = mutation(
        &app,
        &mmr_path,
        json!({
            "lane": "MID",
            "delta": 25.5,
            "note": "manual correction",
            "expectedRevision": 0,
            "unexpected": true
        }),
        Some(COMMAND_KEY),
    )
    .await?;
    assert_eq!(unknown_field.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let adjusted = mutation(&app, &mmr_path, mmr_body(0, 25.5), Some(COMMAND_KEY)).await?;
    assert_eq!(adjusted.status(), StatusCode::OK);
    let adjusted = body_json(adjusted).await?;
    assert_eq!(adjusted["data"]["revision"], 1);
    assert_eq!(find_member(&adjusted)?["laneMmr"]["MID"], 1525.5);
    assert_eq!(admin_state_count(&pool).await?, 1);
    assert_mmr_storage(&pool, 1525.5, 1).await?;

    let replay = mutation(&app, &mmr_path, mmr_body(0, 25.5), Some(COMMAND_KEY)).await?;
    assert_eq!(replay.status(), StatusCode::OK);
    assert_eq!(body_json(replay).await?["data"], adjusted["data"]);
    assert_mmr_storage(&pool, 1525.5, 1).await?;

    let reused_key = mutation(&app, &mmr_path, mmr_body(0, 10.0), Some(COMMAND_KEY)).await?;
    assert_eq!(reused_key.status(), StatusCode::CONFLICT);
    assert_eq!(
        body_json(reused_key).await?["error"]["code"],
        "idempotency_conflict"
    );

    let stale = mutation(
        &app,
        &status_path,
        json!({"status":"INACTIVE","expectedRevision":0}),
        None,
    )
    .await?;
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    let stale = body_json(stale).await?;
    assert_eq!(stale["error"]["code"], "revision_conflict");
    assert_eq!(stale["error"]["details"]["currentRevision"], 1);

    let status = mutation(
        &app,
        &status_path,
        json!({"status":"INACTIVE","expectedRevision":1}),
        None,
    )
    .await?;
    assert_eq!(status.status(), StatusCode::OK);
    let status = body_json(status).await?;
    assert_eq!(status["data"]["revision"], 2);
    assert_eq!(find_member(&status)?["status"], "INACTIVE");

    let persisted = body_json(operator_get(&app, &list_path).await?).await?;
    assert_eq!(persisted["data"]["revision"], 2);
    assert_eq!(find_member(&persisted)?["laneMmr"]["MID"], 1525.5);
    assert_eq!(find_member(&persisted)?["status"], "INACTIVE");
    assert_database_effects(&pool).await?;

    pool.close().await;
    drop(server);
    Ok(())
}

fn mmr_body(expected_revision: i64, delta: f64) -> Value {
    json!({
        "lane": "MID",
        "delta": delta,
        "note": "manual correction",
        "expectedRevision": expected_revision
    })
}

fn find_member(body: &Value) -> TestResult<&Value> {
    body["data"]["members"]
        .as_array()
        .and_then(|members| members.iter().find(|member| member["id"] == MEMBER_ID))
        .ok_or_else(|| "member missing from response".into())
}

async fn operator_get(app: &Router, path: &str) -> TestResult<Response<Body>> {
    send(
        app,
        Method::GET,
        path,
        OPERATOR_SESSION,
        OPERATOR_CSRF,
        None,
        false,
        None,
    )
    .await
}

async fn mutation(
    app: &Router,
    path: &str,
    body: Value,
    idempotency_key: Option<&str>,
) -> TestResult<Response<Body>> {
    send(
        app,
        Method::PATCH,
        path,
        OPERATOR_SESSION,
        OPERATOR_CSRF,
        Some(body),
        true,
        idempotency_key,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn send(
    app: &Router,
    method: Method,
    path: &str,
    session: &str,
    csrf: &str,
    body: Option<Value>,
    include_csrf: bool,
    idempotency_key: Option<&str>,
) -> TestResult<Response<Body>> {
    let mut request = Request::builder().method(method).uri(path).header(
        header::COOKIE,
        format!("monkeybot_session={session}; monkeybot_csrf={csrf}"),
    );
    let request_body = if let Some(body) = body {
        request = request
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ORIGIN, PUBLIC_ORIGIN);
        if include_csrf {
            request = request.header("x-csrf-token", csrf);
        }
        if let Some(key) = idempotency_key {
            request = request.header("idempotency-key", key);
        }
        Body::from(serde_json::to_vec(&body)?)
    } else {
        Body::empty()
    };
    Ok(app.clone().oneshot(request.body(request_body)?).await?)
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
            discord_user_id: DiscordUserId::parse(user_id)?,
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

async fn seed_guild(pool: &PgPool) -> TestResult {
    let now = OffsetDateTime::now_utc();
    sqlx::query(
        "INSERT INTO guilds (
             guild_id, discord_name, icon_hash, status, installed_by,
             installed_at, activated_at, left_at, updated_at
         ) VALUES ($1, 'Admin Test Guild', NULL, 'ACTIVE', $2, $3, $3, NULL, $3)",
    )
    .bind(GUILD_ID)
    .bind(OPERATOR_ID)
    .bind(now)
    .execute(pool)
    .await?;

    for (user_id, display_name, main_lane) in [
        (OPERATOR_ID, "Operator", Some("MID")),
        (MEMBER_ID, "NoLane", None),
    ] {
        sqlx::query(
            "INSERT INTO guild_members (
                 guild_id, user_id, display_name, status, main_lane, first_seen_at,
                 registered_at, discord_joined_at, last_active_at, last_synced_at, inactivated_at
             ) VALUES ($1, $2, $3, 'ACTIVE', $4, $5, $5, $5, $5, $5, NULL)",
        )
        .bind(GUILD_ID)
        .bind(user_id)
        .bind(display_name)
        .bind(main_lane)
        .bind(now)
        .execute(pool)
        .await?;
    }

    sqlx::query(
        "INSERT INTO guild_config (
             guild_id, display_name, operator_role_id, recruitment_channel_id,
             log_channel_id, is_externally_public, revision, updated_by, updated_at
         ) VALUES ($1, 'Admin Test Guild', $2, NULL, NULL, false, 0, $3, $4)",
    )
    .bind(GUILD_ID)
    .bind(OPERATOR_ROLE_ID)
    .bind(OPERATOR_ID)
    .bind(now)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO guild_member_discord_roles (guild_id, user_id, role_id, last_synced_at)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(GUILD_ID)
    .bind(OPERATOR_ID)
    .bind(OPERATOR_ROLE_ID)
    .bind(now)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO seasons (
             guild_id, id, name, status, started_at, ended_at, created_by, created_at
         ) VALUES ($1, $2, 'Season 1', 'ACTIVE', $3, NULL, $4, $3)",
    )
    .bind(GUILD_ID)
    .bind(Uuid::now_v7())
    .bind(now)
    .bind(OPERATOR_ID)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO riot_accounts (
             puuid, user_id, platform, routing_region, game_name, tag_line,
             normalized_game_name, normalized_tag_line, profile_icon_id, is_main,
             main_position, main_position_updated_at, last_synced_at, created_at, updated_at
         ) VALUES (
             'admin-member-puuid', $1, 'KR', 'ASIA', 'NoLaneMain', 'KR1',
             'nolanemain', 'kr1', 1, true, NULL, NULL, $2, $2, $2
         )",
    )
    .bind(MEMBER_ID)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

async fn admin_state_count(pool: &PgPool) -> TestResult<i64> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM guild_admin_state WHERE guild_id = $1 AND scope = 'MEMBERS'",
    )
    .bind(GUILD_ID)
    .fetch_one(pool)
    .await?)
}

async fn member_status(pool: &PgPool, member_id: &str) -> TestResult<String> {
    Ok(sqlx::query_scalar::<_, String>(
        "SELECT status FROM guild_members WHERE guild_id = $1 AND user_id = $2",
    )
    .bind(GUILD_ID)
    .bind(member_id)
    .fetch_one(pool)
    .await?)
}

async fn audit_count(pool: &PgPool) -> TestResult<i64> {
    Ok(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM admin_audit_log WHERE guild_id = $1")
            .bind(GUILD_ID)
            .fetch_one(pool)
            .await?,
    )
}

async fn assert_mmr_storage(pool: &PgPool, expected_mmr: f64, count: i64) -> TestResult {
    let (mmr, revision) = sqlx::query_as::<_, (f64, i64)>(
        "SELECT mmr, revision
         FROM user_lane_mmr
         WHERE guild_id = $1 AND user_id = $2 AND lane = 'MID'",
    )
    .bind(GUILD_ID)
    .bind(MEMBER_ID)
    .fetch_one(pool)
    .await?;
    assert_eq!((mmr, revision), (expected_mmr, 1));
    let ledger_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*)
         FROM mmr_ledger_entries
         WHERE guild_id = $1 AND user_id = $2 AND source_type = 'MANUAL_ADJUSTMENT'",
    )
    .bind(GUILD_ID)
    .bind(MEMBER_ID)
    .fetch_one(pool)
    .await?;
    assert_eq!(ledger_count, count);
    Ok(())
}

async fn assert_database_effects(pool: &PgPool) -> TestResult {
    let state = sqlx::query_as::<_, (i64, Option<String>)>(
        "SELECT revision, updated_by
         FROM guild_admin_state
         WHERE guild_id = $1 AND scope = 'MEMBERS'",
    )
    .bind(GUILD_ID)
    .fetch_one(pool)
    .await?;
    assert_eq!(state, (2, Some(OPERATOR_ID.to_owned())));

    let member = sqlx::query_as::<_, (String, bool)>(
        "SELECT status, inactivated_at IS NOT NULL
         FROM guild_members WHERE guild_id = $1 AND user_id = $2",
    )
    .bind(GUILD_ID)
    .bind(MEMBER_ID)
    .fetch_one(pool)
    .await?;
    assert_eq!(member, ("INACTIVE".to_owned(), true));

    let actions = sqlx::query_scalar::<_, String>(
        "SELECT action
         FROM admin_audit_log
         WHERE guild_id = $1
         ORDER BY created_at, id",
    )
    .bind(GUILD_ID)
    .fetch_all(pool)
    .await?;
    assert_eq!(
        actions,
        vec![
            "MEMBER_MMR_ADJUSTED".to_owned(),
            "MEMBER_STATUS_CHANGED".to_owned()
        ]
    );
    Ok(())
}

async fn body_json(response: Response<Body>) -> TestResult<Value> {
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    Ok(serde_json::from_slice(&bytes)?)
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
