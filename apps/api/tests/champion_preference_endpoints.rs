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
const POSTGRES_PASSWORD: &str = "monkeybot-preferences-endpoint-password";
const DATABASE: &str = "monkeybot_preferences_endpoints";
const USER_ID: &str = "100000000000000001";
const DISCORD_CLIENT_ID: &str = "900000000000000001";
const PUBLIC_ORIGIN: &str = "http://localhost:5173";
const OAUTH_KEY_BASE64URL: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const SESSION_TOKEN: &str = "preferences-endpoint-session-token";
const CSRF_TOKEN: &str = "preferences-endpoint-csrf-token";

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn champion_preferences_are_authenticated_ordered_atomic_lane_replacements() -> TestResult {
    tokio::time::timeout(Duration::from_secs(120), run_champion_preferences_flow())
        .await
        .map_err(|_| "champion preference endpoint integration test exceeded 120 seconds")??;
    Ok(())
}

async fn run_champion_preferences_flow() -> TestResult {
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
        .oneshot(Request::get("/api/me/champion-preferences").body(Body::empty())?)
        .await?;
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);

    let initial = authenticated_get(&app).await?;
    assert_eq!(initial.status(), StatusCode::OK);
    assert_eq!(
        body_json(initial).await?["data"]["preferences"],
        empty_preferences()
    );

    let no_csrf = request(
        &app,
        Method::PUT,
        "/api/me/champion-preferences/MID",
        &json!({"championIds":["Ahri"]}),
        false,
    )
    .await?;
    assert_eq!(no_csrf.status(), StatusCode::FORBIDDEN);

    let mid = mutation(
        &app,
        "/api/me/champion-preferences/MID",
        &json!({"championIds":["Ahri","Orianna","Syndra"]}),
    )
    .await?;
    assert_eq!(mid.status(), StatusCode::OK);
    let mid = body_json(mid).await?;
    assert_eq!(
        mid["data"]["preferences"]["MID"],
        json!(["Ahri", "Orianna", "Syndra"])
    );
    assert_all_lane_keys(&mid["data"]["preferences"]);

    let top = mutation(
        &app,
        "/api/me/champion-preferences/TOP",
        &json!({"championIds":["Gnar","KSante"]}),
    )
    .await?;
    let top = body_json(top).await?;
    assert_eq!(top["data"]["preferences"]["TOP"], json!(["Gnar", "KSante"]));
    assert_eq!(
        top["data"]["preferences"]["MID"],
        json!(["Ahri", "Orianna", "Syndra"])
    );

    let duplicate = mutation(
        &app,
        "/api/me/champion-preferences/MID",
        &json!({"championIds":["Ahri","Ahri"]}),
    )
    .await?;
    assert_eq!(duplicate.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body_json(duplicate).await?["error"]["code"],
        "duplicate_champion_id"
    );

    let too_many = mutation(
        &app,
        "/api/me/champion-preferences/JUNGLE",
        &json!({
            "championIds": [
                "Champion0", "Champion1", "Champion2", "Champion3", "Champion4",
                "Champion5", "Champion6", "Champion7", "Champion8", "Champion9",
                "Champion10"
            ]
        }),
    )
    .await?;
    assert_eq!(too_many.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body_json(too_many).await?["error"]["code"],
        "too_many_champion_preferences"
    );

    let invalid_lane = mutation(
        &app,
        "/api/me/champion-preferences/FLEX",
        &json!({"championIds":[]}),
    )
    .await?;
    assert_eq!(invalid_lane.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        body_json(invalid_lane).await?["error"]["code"],
        "invalid_lane"
    );

    let replaced = mutation(
        &app,
        "/api/me/champion-preferences/MID",
        &json!({"championIds":["Syndra","Ahri"]}),
    )
    .await?;
    let replaced = body_json(replaced).await?;
    assert_eq!(
        replaced["data"]["preferences"]["MID"],
        json!(["Syndra", "Ahri"])
    );
    assert_eq!(
        replaced["data"]["preferences"]["TOP"],
        json!(["Gnar", "KSante"])
    );

    let stored = sqlx::query_as::<_, (i16, String)>(
        "SELECT position, champion_id
         FROM user_champion_preferences
         WHERE user_id = $1 AND lane = 'MID'
         ORDER BY position",
    )
    .bind(USER_ID)
    .fetch_all(&pool)
    .await?;
    assert_eq!(
        stored,
        vec![(1, "Syndra".to_owned()), (2, "Ahri".to_owned())]
    );

    let final_snapshot = authenticated_get(&app).await?;
    let final_snapshot = body_json(final_snapshot).await?;
    assert_eq!(
        final_snapshot["data"]["preferences"],
        replaced["data"]["preferences"]
    );

    pool.close().await;
    drop(server);
    Ok(())
}

async fn authenticated_get(app: &Router) -> TestResult<Response<Body>> {
    let request = Request::get("/api/me/champion-preferences")
        .header(
            header::COOKIE,
            format!("monkeybot_session={SESSION_TOKEN}; monkeybot_csrf={CSRF_TOKEN}"),
        )
        .body(Body::empty())?;
    Ok(app.clone().oneshot(request).await?)
}

async fn mutation(app: &Router, uri: &str, body: &Value) -> TestResult<Response<Body>> {
    request(app, Method::PUT, uri, body, true).await
}

async fn request(
    app: &Router,
    method: Method,
    uri: &str,
    body: &Value,
    csrf: bool,
) -> TestResult<Response<Body>> {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .header(
            header::COOKIE,
            format!("monkeybot_session={SESSION_TOKEN}; monkeybot_csrf={CSRF_TOKEN}"),
        );
    if csrf {
        request = request
            .header(header::ORIGIN, PUBLIC_ORIGIN)
            .header("x-csrf-token", CSRF_TOKEN);
    }
    Ok(app
        .clone()
        .oneshot(request.body(Body::from(serde_json::to_vec(body)?))?)
        .await?)
}

async fn body_json(response: Response<Body>) -> TestResult<Value> {
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn empty_preferences() -> Value {
    json!({"TOP":[],"JUNGLE":[],"MID":[],"BOTTOM":[],"SUPPORT":[]})
}

fn assert_all_lane_keys(preferences: &Value) {
    let object = preferences
        .as_object()
        .expect("preferences must be an object");
    assert_eq!(object.len(), 5);
    for lane in ["TOP", "JUNGLE", "MID", "BOTTOM", "SUPPORT"] {
        assert!(object.contains_key(lane), "missing lane {lane}");
    }
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
