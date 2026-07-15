use std::{collections::HashMap, error::Error, sync::Arc, time::Duration};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, Response, StatusCode, header},
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
use sqlx::postgres::PgPoolOptions;
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
const POSTGRES_PASSWORD: &str = "monkeybot-service-status-endpoint-password";
const DATABASE: &str = "monkeybot_service_status_endpoints";
const SERVICE_ADMIN_ID: &str = "100000000000000001";
const MEMBER_ID: &str = "100000000000000002";
const DISCORD_CLIENT_ID: &str = "900000000000000001";
const PUBLIC_ORIGIN: &str = "http://localhost:5173";
const OAUTH_KEY_BASE64URL: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const BOT_SERVICE_TOKEN: &str = "bot-service-token-for-service-status-integration-test";
const ADMIN_SESSION: &str = "service-status-admin-session";
const ADMIN_CSRF: &str = "service-status-admin-csrf";
const MEMBER_SESSION: &str = "service-status-member-session";
const MEMBER_CSRF: &str = "service-status-member-csrf";

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bot_heartbeat_is_durable_and_status_is_service_admin_only() -> TestResult {
    tokio::time::timeout(Duration::from_secs(120), run_flow())
        .await
        .map_err(|_| "service status endpoint test exceeded 120 seconds")??;
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
        SERVICE_ADMIN_ID,
        "Service Admin",
        ADMIN_SESSION,
        ADMIN_CSRF,
    )
    .await?;
    create_login(
        &auth_repository,
        MEMBER_ID,
        "Member",
        MEMBER_SESSION,
        MEMBER_CSRF,
    )
    .await?;

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

    let missing_bot_auth = app
        .clone()
        .oneshot(
            Request::post("/internal/heartbeat")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))?,
        )
        .await?;
    assert_error(
        missing_bot_auth,
        StatusCode::UNAUTHORIZED,
        "unauthorized",
        "인증이 필요합니다.",
    )
    .await?;

    let invalid = bot_post(
        &app,
        "/internal/heartbeat",
        json!({ "instanceId": "invalid instance", "version": "0.1.0", "commitSha": null }),
    )
    .await?;
    assert_error(
        invalid,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_heartbeat",
        "Bot heartbeat 요청 값이 올바르지 않습니다.",
    )
    .await?;

    let first = bot_post(
        &app,
        "/internal/heartbeat",
        json!({ "instanceId": "bot-main-1", "version": "0.1.0", "commitSha": "abc1234" }),
    )
    .await?;
    assert_eq!(first.status(), StatusCode::OK);
    let first = body_json(first).await?;
    assert_eq!(first["data"]["accepted"], true);
    assert!(first["data"]["lastHeartbeatAt"].is_string());

    let (started_at, first_heartbeat_at): (OffsetDateTime, OffsetDateTime) = sqlx::query_as(
        "SELECT started_at, last_heartbeat_at FROM bot_instances WHERE instance_id = $1",
    )
    .bind("bot-main-1")
    .fetch_one(&pool)
    .await?;

    let second = bot_post(
        &app,
        "/internal/heartbeat",
        json!({ "instanceId": "bot-main-1", "version": "0.2.0", "commitSha": "def5678" }),
    )
    .await?;
    assert_eq!(second.status(), StatusCode::OK);
    let (version, commit_sha, persisted_started_at, last_heartbeat_at): (
        String,
        Option<String>,
        OffsetDateTime,
        OffsetDateTime,
    ) = sqlx::query_as(
        "SELECT version, commit_sha, started_at, last_heartbeat_at
         FROM bot_instances WHERE instance_id = $1",
    )
    .bind("bot-main-1")
    .fetch_one(&pool)
    .await?;
    assert_eq!(version, "0.2.0");
    assert_eq!(commit_sha.as_deref(), Some("def5678"));
    assert_eq!(persisted_started_at, started_at);
    assert!(last_heartbeat_at >= first_heartbeat_at);

    let unauthenticated = app
        .clone()
        .oneshot(Request::get("/api/admin/status").body(Body::empty())?)
        .await?;
    assert_error(
        unauthenticated,
        StatusCode::UNAUTHORIZED,
        "unauthorized",
        "인증이 필요합니다.",
    )
    .await?;

    let non_admin = browser_get(&app, MEMBER_SESSION, MEMBER_CSRF).await?;
    assert_error(
        non_admin,
        StatusCode::FORBIDDEN,
        "service_admin_required",
        "서비스 관리자 권한이 필요합니다.",
    )
    .await?;

    let healthy = browser_get(&app, ADMIN_SESSION, ADMIN_CSRF).await?;
    assert_eq!(healthy.status(), StatusCode::OK);
    let healthy = body_json(healthy).await?;
    assert!(healthy["data"]["checkedAt"].is_string());
    let services = healthy["data"]["services"]
        .as_array()
        .ok_or("status response services missing")?;
    assert_eq!(services.len(), 4);
    assert_eq!(service(services, "BOT_HEARTBEAT")["status"], "normal");
    assert!(service(services, "BOT_HEARTBEAT")["issue"].is_null());
    assert!(service(services, "BOT_HEARTBEAT")["lastHealthyAt"].is_string());

    sqlx::query(
        "UPDATE bot_instances
         SET last_heartbeat_at = CURRENT_TIMESTAMP - INTERVAL '91 seconds'
         WHERE instance_id = $1",
    )
    .bind("bot-main-1")
    .execute(&pool)
    .await?;
    let delayed = body_json(browser_get(&app, ADMIN_SESSION, ADMIN_CSRF).await?).await?;
    assert_eq!(
        service_array(&delayed, "BOT_HEARTBEAT")["status"],
        "delayed"
    );
    assert_eq!(
        service_array(&delayed, "BOT_HEARTBEAT")["issue"],
        "마지막 heartbeat가 정상 기준 90초를 초과했습니다."
    );
    assert!(service_array(&delayed, "BOT_HEARTBEAT")["lastHealthyAt"].is_string());

    sqlx::query(
        "UPDATE bot_instances
         SET last_heartbeat_at = CURRENT_TIMESTAMP - INTERVAL '301 seconds'
         WHERE instance_id = $1",
    )
    .bind("bot-main-1")
    .execute(&pool)
    .await?;
    let down = body_json(browser_get(&app, ADMIN_SESSION, ADMIN_CSRF).await?).await?;
    assert_eq!(service_array(&down, "BOT_HEARTBEAT")["status"], "down");
    assert_eq!(
        service_array(&down, "BOT_HEARTBEAT")["issue"],
        "마지막 heartbeat가 5분을 초과했거나 아직 수신되지 않았습니다."
    );

    Ok(())
}

fn service<'a>(services: &'a [Value], key: &str) -> &'a Value {
    services
        .iter()
        .find(|service| service["key"] == key)
        .expect("service row")
}

fn service_array<'a>(body: &'a Value, key: &str) -> &'a Value {
    let services = body["data"]["services"]
        .as_array()
        .expect("status response services");
    service(services, key)
}

async fn bot_post(app: &Router, path: &str, body: Value) -> TestResult<Response<Body>> {
    let request = Request::post(path)
        .header(header::AUTHORIZATION, format!("Bearer {BOT_SERVICE_TOKEN}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body)?))?;
    Ok(app.clone().oneshot(request).await?)
}

async fn browser_get(app: &Router, session: &str, csrf: &str) -> TestResult<Response<Body>> {
    let request = Request::builder()
        .method(Method::GET)
        .uri("/api/admin/status")
        .header(
            header::COOKIE,
            format!("monkeybot_session={session}; monkeybot_csrf={csrf}"),
        )
        .body(Body::empty())?;
    Ok(app.clone().oneshot(request).await?)
}

async fn assert_error(
    response: Response<Body>,
    status: StatusCode,
    code: &str,
    message: &str,
) -> TestResult {
    assert_eq!(response.status(), status);
    let body = body_json(response).await?;
    assert_eq!(body["error"]["code"], code);
    assert_eq!(body["error"]["message"], message);
    assert!(body["error"]["requestId"].is_string());
    Ok(())
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

fn test_config(database_url: String) -> TestResult<Config> {
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
        (
            "MONKEYBOT_BOT_SERVICE_TOKEN".to_owned(),
            BOT_SERVICE_TOKEN.to_owned(),
        ),
        (
            "MONKEYBOT_SERVICE_ADMIN_IDS".to_owned(),
            SERVICE_ADMIN_ID.to_owned(),
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
