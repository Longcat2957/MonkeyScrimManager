use std::{collections::HashMap, error::Error, sync::Arc, time::Duration};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, Response, StatusCode, header},
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
const POSTGRES_PASSWORD: &str = "monkeybot-privacy-endpoint-password";
const DATABASE: &str = "monkeybot_privacy_endpoints";
const USER_ID: &str = "100000000000000001";
const DISCORD_CLIENT_ID: &str = "900000000000000001";
const PUBLIC_ORIGIN: &str = "http://localhost:5173";
const OAUTH_KEY_BASE64URL: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const SESSION_TOKEN: &str = "privacy-endpoint-session-token";
const CSRF_TOKEN: &str = "privacy-endpoint-csrf-token";

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn privacy_settings_are_authenticated_strict_and_persistent() -> TestResult {
    tokio::time::timeout(Duration::from_secs(120), run_privacy_settings_flow())
        .await
        .map_err(|_| "privacy endpoint integration test exceeded 120 seconds")??;
    Ok(())
}

async fn run_privacy_settings_flow() -> TestResult {
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
    auth_repository
        .complete_login(&NewLogin {
            discord_user_id: DiscordUserId::parse(USER_ID)?,
            display_name: "Longcat".to_owned(),
            avatar_hash: None,
            previous_session_token_hash: None,
            session_id: Uuid::now_v7(),
            token_hash: AuthService::hash_secret(SESSION_TOKEN),
            csrf_secret_hash: AuthService::hash_secret(CSRF_TOKEN),
            expires_at: OffsetDateTime::now_utc() + time::Duration::hours(1),
        })
        .await?;

    // Exercise the repository's read defaults rather than the row created during OAuth login.
    sqlx::query("DELETE FROM user_privacy_settings WHERE user_id = $1")
        .bind(USER_ID)
        .execute(&pool)
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

    let unauthenticated = app
        .clone()
        .oneshot(Request::get("/api/me/privacy").body(Body::empty())?)
        .await?;
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);

    let defaults = authenticated_get(&app).await?;
    assert_eq!(defaults.status(), StatusCode::OK);
    assert_eq!(
        body_json(defaults).await?["data"]["settings"],
        json!({
            "publicProfile": true,
            "showRiotIds": true,
            "allowExternalRecordSearch": false
        })
    );
    assert_eq!(privacy_row_count(&pool).await?, 0);

    let no_csrf = authenticated_put(
        &app,
        &json!({
            "publicProfile": false,
            "showRiotIds": false,
            "allowExternalRecordSearch": true
        }),
        false,
    )
    .await?;
    assert_eq!(no_csrf.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        body_json(no_csrf).await?["error"]["code"],
        "csrf_validation_failed"
    );

    let unknown_field = authenticated_put(
        &app,
        &json!({
            "publicProfile": false,
            "showRiotIds": false,
            "allowExternalRecordSearch": true,
            "unexpected": true
        }),
        true,
    )
    .await?;
    assert_eq!(unknown_field.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(privacy_row_count(&pool).await?, 0);

    let updated = authenticated_put(
        &app,
        &json!({
            "publicProfile": false,
            "showRiotIds": false,
            "allowExternalRecordSearch": true
        }),
        true,
    )
    .await?;
    assert_eq!(updated.status(), StatusCode::OK);
    let updated = body_json(updated).await?;
    assert_eq!(
        updated["data"]["settings"],
        json!({
            "publicProfile": false,
            "showRiotIds": false,
            "allowExternalRecordSearch": true
        })
    );

    let stored = sqlx::query_as::<_, (bool, bool, bool)>(
        "SELECT public_profile_enabled, show_riot_ids, allow_external_record_search
         FROM user_privacy_settings
         WHERE user_id = $1",
    )
    .bind(USER_ID)
    .fetch_one(&pool)
    .await?;
    assert_eq!(stored, (false, false, true));

    let persisted = authenticated_get(&app).await?;
    assert_eq!(persisted.status(), StatusCode::OK);
    assert_eq!(
        body_json(persisted).await?["data"]["settings"],
        updated["data"]["settings"]
    );

    pool.close().await;
    drop(server);
    Ok(())
}

async fn authenticated_get(app: &Router) -> TestResult<Response<Body>> {
    let request = Request::get("/api/me/privacy")
        .header(
            header::COOKIE,
            format!("monkeybot_session={SESSION_TOKEN}; monkeybot_csrf={CSRF_TOKEN}"),
        )
        .body(Body::empty())?;
    Ok(app.clone().oneshot(request).await?)
}

async fn authenticated_put(
    app: &Router,
    body: &Value,
    include_csrf_header: bool,
) -> TestResult<Response<Body>> {
    let mut request = Request::put("/api/me/privacy")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, PUBLIC_ORIGIN)
        .header(
            header::COOKIE,
            format!("monkeybot_session={SESSION_TOKEN}; monkeybot_csrf={CSRF_TOKEN}"),
        );
    if include_csrf_header {
        request = request.header("x-csrf-token", CSRF_TOKEN);
    }
    Ok(app
        .clone()
        .oneshot(request.body(Body::from(serde_json::to_vec(body)?))?)
        .await?)
}

async fn privacy_row_count(pool: &sqlx::PgPool) -> TestResult<i64> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM user_privacy_settings WHERE user_id = $1",
    )
    .bind(USER_ID)
    .fetch_one(pool)
    .await?)
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
