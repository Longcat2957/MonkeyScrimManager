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
const POSTGRES_PASSWORD: &str = "monkeybot-season-endpoint-password";
const DATABASE: &str = "monkeybot_season_endpoints";
const OPERATOR_ID: &str = "100000000000000001";
const MEMBER_ID: &str = "100000000000000002";
const GUILD_ID: &str = "200000000000000001";
const OPERATOR_ROLE_ID: &str = "300000000000000001";
const RECRUITMENT_CHANNEL_ID: &str = "400000000000000001";
const INITIAL_SEASON_ID: &str = "01900000-0000-7000-8000-000000000001";
const RECRUITMENT_ID: &str = "01900000-0000-7000-8000-000000000002";
const SERIES_ID: &str = "01900000-0000-7000-8000-000000000003";
const DISCORD_CLIENT_ID: &str = "900000000000000001";
const PUBLIC_ORIGIN: &str = "http://localhost:5173";
const OAUTH_KEY_BASE64URL: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const OPERATOR_SESSION: &str = "season-operator-session";
const OPERATOR_CSRF: &str = "season-operator-csrf";
const MEMBER_SESSION: &str = "season-member-session";
const MEMBER_CSRF: &str = "season-member-csrf";

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn guild_seasons_are_aggregated_rotated_audited_and_idempotent() -> TestResult {
    tokio::time::timeout(Duration::from_secs(120), run_flow())
        .await
        .map_err(|_| "guild season endpoint test exceeded 120 seconds")??;
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
        "Member",
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
    let path = format!("/api/guilds/{GUILD_ID}/seasons");

    let unauthenticated = app
        .clone()
        .oneshot(Request::get(&path).body(Body::empty())?)
        .await?;
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);

    let forbidden = browser_request(
        &app,
        Method::GET,
        &path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        None,
        None,
        false,
    )
    .await?;
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        body_json(forbidden).await?["error"]["code"],
        "operator_required"
    );

    let initial = operator_get(&app, &path).await?;
    assert_eq!(initial.status(), StatusCode::OK);
    let initial = body_json(initial).await?;
    assert_eq!(initial["data"]["current"]["id"], INITIAL_SEASON_ID);
    assert_eq!(initial["data"]["current"]["name"], "시즌 1");
    assert_eq!(initial["data"]["current"]["status"], "ACTIVE");
    assert_eq!(initial["data"]["current"]["endedAt"], Value::Null);
    assert_eq!(initial["data"]["current"]["createdBy"], "Operator");
    assert_eq!(initial["data"]["current"]["participantCount"], 0);
    assert_eq!(initial["data"]["current"]["generalSeriesCount"], 1);
    assert_eq!(initial["data"]["current"]["auctionTournamentCount"], 0);
    assert_eq!(initial["data"]["current"]["confirmedGameCount"], 0);
    assert_eq!(initial["data"]["completed"], json!([]));

    let command = json!({
        "name": "  시즌 2  ",
        "expectedCurrentSeasonId": INITIAL_SEASON_ID
    });
    let no_csrf = browser_request(
        &app,
        Method::POST,
        &path,
        OPERATOR_SESSION,
        OPERATOR_CSRF,
        Some(command.clone()),
        Some(Uuid::now_v7().to_string()),
        false,
    )
    .await?;
    assert_eq!(no_csrf.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        body_json(no_csrf).await?["error"]["code"],
        "csrf_validation_failed"
    );

    let unknown_field = browser_mutation(
        &app,
        &path,
        json!({
            "name": "시즌 2",
            "expectedCurrentSeasonId": INITIAL_SEASON_ID,
            "unexpected": true
        }),
        Uuid::now_v7().to_string(),
    )
    .await?;
    assert_eq!(unknown_field.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let in_progress_key = Uuid::now_v7().to_string();
    let mut in_progress_blocker = pool.begin().await?;
    sqlx::query(
        "SELECT true
         FROM guilds
         WHERE guild_id = $1
         FOR SHARE",
    )
    .bind(GUILD_ID)
    .execute(&mut *in_progress_blocker)
    .await?;
    sqlx::query(
        "SELECT pg_advisory_xact_lock(
             hashtextextended(
                 jsonb_build_array(
                     'monkeybot:idempotency:v1',
                     $1::text,
                     'USER',
                     $2::text,
                     'POST',
                     '/api/guilds/{guildId}/seasons',
                     $3::text
                 )::text,
                 0
             )
         )",
    )
    .bind(GUILD_ID)
    .bind(OPERATOR_ID)
    .bind(&in_progress_key)
    .execute(&mut *in_progress_blocker)
    .await?;

    let in_progress = tokio::time::timeout(
        Duration::from_secs(2),
        browser_mutation(&app, &path, command.clone(), in_progress_key),
    )
    .await
    .map_err(|_| "same-key request waited behind the guild row lock")??;
    assert_eq!(in_progress.status(), StatusCode::CONFLICT);
    assert_eq!(
        body_json(in_progress).await?["error"]["code"],
        "idempotency_in_progress"
    );
    in_progress_blocker.rollback().await?;
    assert_eq!(mutation_counts(&pool).await?, (1, 0, 0));

    let idempotency_key = Uuid::now_v7().to_string();
    let rotated = browser_mutation(&app, &path, command.clone(), idempotency_key.clone()).await?;
    assert_eq!(rotated.status(), StatusCode::OK);
    let rotated = body_json(rotated).await?;
    let snapshot = rotated["data"].clone();
    let current_id = snapshot["current"]["id"]
        .as_str()
        .ok_or("rotated current season id missing")?
        .to_owned();
    assert_ne!(current_id, INITIAL_SEASON_ID);
    assert_eq!(snapshot["current"]["name"], "시즌 2");
    assert_eq!(snapshot["current"]["status"], "ACTIVE");
    assert_eq!(snapshot["current"]["createdBy"], "Operator");
    assert_eq!(snapshot["completed"].as_array().map(Vec::len), Some(1));
    assert_eq!(snapshot["completed"][0]["id"], INITIAL_SEASON_ID);
    assert_eq!(snapshot["completed"][0]["status"], "CLOSED");
    assert!(snapshot["completed"][0]["endedAt"].is_string());
    assert_eq!(snapshot["completed"][0]["generalSeriesCount"], 1);

    let replay = browser_mutation(&app, &path, command.clone(), idempotency_key.clone()).await?;
    assert_eq!(replay.status(), StatusCode::OK);
    assert_eq!(body_json(replay).await?["data"], snapshot);
    assert_eq!(mutation_counts(&pool).await?, (2, 1, 1));

    let changed_payload = browser_mutation(
        &app,
        &path,
        json!({
            "name": "다른 시즌",
            "expectedCurrentSeasonId": INITIAL_SEASON_ID
        }),
        idempotency_key,
    )
    .await?;
    assert_eq!(changed_payload.status(), StatusCode::CONFLICT);
    assert_eq!(
        body_json(changed_payload).await?["error"]["code"],
        "idempotency_conflict"
    );

    let stale = browser_mutation(
        &app,
        &path,
        json!({
            "name": "시즌 3",
            "expectedCurrentSeasonId": INITIAL_SEASON_ID
        }),
        Uuid::now_v7().to_string(),
    )
    .await?;
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    let stale = body_json(stale).await?;
    assert_eq!(stale["error"]["code"], "current_season_conflict");
    assert_eq!(stale["error"]["details"]["currentSeasonId"], current_id);
    assert_eq!(mutation_counts(&pool).await?, (2, 1, 1));
    assert_preserved_mmr(&pool).await?;

    let concurrent_body = json!({
        "name": "시즌 3",
        "expectedCurrentSeasonId": current_id
    });
    let (first, second) = tokio::join!(
        browser_mutation(
            &app,
            &path,
            concurrent_body.clone(),
            Uuid::now_v7().to_string()
        ),
        browser_mutation(&app, &path, concurrent_body, Uuid::now_v7().to_string())
    );
    let first = first?;
    let second = second?;
    let (success, conflict) = match (first.status(), second.status()) {
        (StatusCode::OK, StatusCode::CONFLICT) => (first, second),
        (StatusCode::CONFLICT, StatusCode::OK) => (second, first),
        statuses => panic!("concurrent rotations returned {statuses:?}"),
    };
    assert_eq!(
        body_json(success).await?["data"]["current"]["name"],
        "시즌 3"
    );
    assert_eq!(
        body_json(conflict).await?["error"]["code"],
        "current_season_conflict"
    );
    assert_eq!(mutation_counts(&pool).await?, (3, 2, 2));

    pool.close().await;
    drop(server);
    Ok(())
}

async fn operator_get(app: &Router, path: &str) -> TestResult<Response<Body>> {
    browser_request(
        app,
        Method::GET,
        path,
        OPERATOR_SESSION,
        OPERATOR_CSRF,
        None,
        None,
        false,
    )
    .await
}

async fn browser_mutation(
    app: &Router,
    path: &str,
    body: Value,
    idempotency_key: String,
) -> TestResult<Response<Body>> {
    browser_request(
        app,
        Method::POST,
        path,
        OPERATOR_SESSION,
        OPERATOR_CSRF,
        Some(body),
        Some(idempotency_key),
        true,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn browser_request(
    app: &Router,
    method: Method,
    path: &str,
    session: &str,
    csrf: &str,
    body: Option<Value>,
    idempotency_key: Option<String>,
    include_csrf: bool,
) -> TestResult<Response<Body>> {
    let mut request = Request::builder().method(method).uri(path).header(
        header::COOKIE,
        format!("monkeybot_session={session}; monkeybot_csrf={csrf}"),
    );
    if let Some(idempotency_key) = idempotency_key {
        request = request.header("idempotency-key", idempotency_key);
    }
    let request_body = if let Some(body) = body {
        request = request
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ORIGIN, PUBLIC_ORIGIN);
        if include_csrf {
            request = request.header("x-csrf-token", csrf);
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

async fn seed_guild(pool: &PgPool) -> TestResult {
    let now = OffsetDateTime::now_utc();
    sqlx::query(
        "INSERT INTO guilds (
             guild_id, discord_name, icon_hash, status, installed_by,
             installed_at, activated_at, left_at, updated_at
         ) VALUES ($1, 'Season Guild', NULL, 'ACTIVE', $2, $3, $3, NULL, $3)",
    )
    .bind(GUILD_ID)
    .bind(OPERATOR_ID)
    .bind(now)
    .execute(pool)
    .await?;
    for (user_id, display_name) in [(OPERATOR_ID, "Operator"), (MEMBER_ID, "Member")] {
        sqlx::query(
            "INSERT INTO guild_members (
                 guild_id, user_id, display_name, status, main_lane, first_seen_at,
                 registered_at, discord_joined_at, last_active_at, last_synced_at, inactivated_at
             ) VALUES ($1, $2, $3, 'ACTIVE', NULL, $4, $4, $4, $4, $4, NULL)",
        )
        .bind(GUILD_ID)
        .bind(user_id)
        .bind(display_name)
        .bind(now)
        .execute(pool)
        .await?;
    }
    sqlx::query(
        "INSERT INTO guild_config (
             guild_id, display_name, operator_role_id, recruitment_channel_id,
             entry_channel_id, log_channel_id, is_externally_public, revision,
             updated_by, updated_at
         ) VALUES ($1, 'Season Guild', $2, $3, NULL, NULL, false, 0, $4, $5)",
    )
    .bind(GUILD_ID)
    .bind(OPERATOR_ROLE_ID)
    .bind(RECRUITMENT_CHANNEL_ID)
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
         ) VALUES ($1, $2, '시즌 1', 'ACTIVE', $3, NULL, $4, $3)",
    )
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(INITIAL_SEASON_ID)?)
    .bind(now - time::Duration::days(30))
    .bind(OPERATOR_ID)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO recruitments (
             guild_id, id, season_id, display_number, title, target_count, status, revision,
             channel_id, message_id, created_by, created_at, updated_by, updated_at,
             closed_at, converted_at, cancelled_at
         ) VALUES ($1, $2, $3, 1, '완료 모집', 10, 'CONVERTED', 1,
                   NULL, NULL, $4, $5, $4, $5, $5, $5, NULL)",
    )
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(RECRUITMENT_ID)?)
    .bind(Uuid::parse_str(INITIAL_SEASON_ID)?)
    .bind(OPERATOR_ID)
    .bind(now - time::Duration::days(2))
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO series (
             guild_id, id, season_id, source_recruitment_id, display_number, title,
             best_of, status, winning_team, terminal_reason, revision, started_at, ended_at,
             created_by, end_card_channel_id, end_card_message_id, updated_by, updated_at, deleted_at
         ) VALUES ($1, $2, $3, $4, 1, '완료 시리즈', 3, 'COMPLETED', 'TEAM_1',
                   'SCORE_REACHED', 1, $5, $6, $7, NULL, NULL, $7, $6, NULL)",
    )
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(SERIES_ID)?)
    .bind(Uuid::parse_str(INITIAL_SEASON_ID)?)
    .bind(Uuid::parse_str(RECRUITMENT_ID)?)
    .bind(now - time::Duration::days(2))
    .bind(now - time::Duration::days(1))
    .bind(OPERATOR_ID)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO user_lane_mmr (
             guild_id, season_id, user_id, lane, mmr, games_played, wins, revision, updated_at
         ) VALUES ($1, $2, $3, 'TOP', 1575, 4, 3, 0, $4)",
    )
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(INITIAL_SEASON_ID)?)
    .bind(MEMBER_ID)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

async fn mutation_counts(pool: &PgPool) -> TestResult<(i64, i64, i64)> {
    Ok(sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT
           (SELECT count(*) FROM seasons WHERE guild_id = $1),
           (SELECT count(*) FROM admin_audit_log
              WHERE guild_id = $1 AND action = 'SEASON_ROTATED'),
           (SELECT count(*) FROM idempotency_records
              WHERE guild_id = $1
                AND route_template = '/api/guilds/{guildId}/seasons')",
    )
    .bind(GUILD_ID)
    .fetch_one(pool)
    .await?)
}

async fn assert_preserved_mmr(pool: &PgPool) -> TestResult {
    let row = sqlx::query_as::<_, (f64, i32, i32)>(
        "SELECT mmr, games_played, wins
         FROM user_lane_mmr
         WHERE guild_id = $1 AND season_id = $2 AND user_id = $3 AND lane = 'TOP'",
    )
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(INITIAL_SEASON_ID)?)
    .bind(MEMBER_ID)
    .fetch_one(pool)
    .await?;
    assert_eq!(row, (1575.0, 4, 3));
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
