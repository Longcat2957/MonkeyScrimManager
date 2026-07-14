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
const POSTGRES_PASSWORD: &str = "monkeybot-dummy-recruitment-password";
const DATABASE: &str = "monkeybot_dummy_recruitment";
const OPERATOR_ID: &str = "100000000000000001";
const MEMBER_ID: &str = "100000000000000002";
const GUILD_ID: &str = "200000000000000001";
const OPERATOR_ROLE_ID: &str = "300000000000000001";
const DISCORD_CLIENT_ID: &str = "900000000000000001";
const PUBLIC_ORIGIN: &str = "http://localhost:5173";
const OAUTH_KEY_BASE64URL: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const OPERATOR_SESSION: &str = "dummy-recruitment-operator-session";
const OPERATOR_CSRF: &str = "dummy-recruitment-operator-csrf";
const MEMBER_SESSION: &str = "dummy-recruitment-member-session";
const MEMBER_CSRF: &str = "dummy-recruitment-member-csrf";
const RECRUITMENT_ID: &str = "01900000-0000-7000-8000-000000000100";
const RECRUITMENT_CHANNEL_ID: &str = "400000000000000001";
const RECRUITMENT_MESSAGE_ID: &str = "500000000000000001";
const DUMMY_A: &str = "dmy_01900000-0000-7000-8000-000000000001";
const DUMMY_B: &str = "dmy_01900000-0000-7000-8000-000000000002";
const DUMMY_C: &str = "dmy_01900000-0000-7000-8000-000000000003";

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dummy_participants_are_previewed_and_applied_atomically() -> TestResult {
    tokio::time::timeout(Duration::from_secs(120), run_flow())
        .await
        .map_err(|_| "dummy recruitment endpoint test exceeded 120 seconds")??;
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
    let auth_config = config.auth().cloned().expect("auth config");
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
    seed(&pool).await?;

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
    let base = format!("/api/dev/guilds/{GUILD_ID}/recruitments/{RECRUITMENT_ID}/participants");

    let unauthenticated = app
        .clone()
        .oneshot(Request::get(&base).body(Body::empty())?)
        .await?;
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);

    let non_operator = send(
        &app,
        Method::GET,
        &base,
        MEMBER_SESSION,
        MEMBER_CSRF,
        None,
        false,
        None,
    )
    .await?;
    assert_error(non_operator, StatusCode::FORBIDDEN, "operator_required").await?;

    let initial = send(
        &app,
        Method::GET,
        &base,
        OPERATOR_SESSION,
        OPERATOR_CSRF,
        None,
        false,
        None,
    )
    .await?;
    assert_eq!(initial.status(), StatusCode::OK);
    let initial = body_json(initial).await?;
    assert_eq!(initial["data"]["recruitment"]["joinedCount"], 8);
    assert_eq!(initial["data"]["recruitment"]["remainingSlots"], 2);
    assert_eq!(
        initial["data"]["candidates"].as_array().map(Vec::len),
        Some(3)
    );

    let selection = json!({ "action": "ADD", "userIds": [DUMMY_A, DUMMY_B] });
    let preview = send(
        &app,
        Method::POST,
        &format!("{base}/preview"),
        OPERATOR_SESSION,
        OPERATOR_CSRF,
        Some(selection),
        false,
        None,
    )
    .await?;
    assert_eq!(preview.status(), StatusCode::OK);
    let preview = body_json(preview).await?;
    assert_eq!(preview["data"]["canApply"], true);
    assert_eq!(preview["data"]["currentRevision"], 0);
    assert_eq!(preview["data"]["projectedJoinedCount"], 10);
    assert_eq!(preview["data"]["remainingSlots"], 0);

    let add_body = json!({ "userIds": [DUMMY_A, DUMMY_B], "expectedRevision": 0 });
    let missing_csrf = send(
        &app,
        Method::POST,
        &base,
        OPERATOR_SESSION,
        OPERATOR_CSRF,
        Some(add_body.clone()),
        false,
        Some("dummy-add"),
    )
    .await?;
    assert_error(
        missing_csrf,
        StatusCode::FORBIDDEN,
        "csrf_validation_failed",
    )
    .await?;

    let added = send(
        &app,
        Method::POST,
        &base,
        OPERATOR_SESSION,
        OPERATOR_CSRF,
        Some(add_body.clone()),
        true,
        Some("dummy-add"),
    )
    .await?;
    assert_eq!(added.status(), StatusCode::OK);
    let added_body = body_json(added).await?;
    assert_eq!(added_body["data"]["action"], "ADD");
    assert_eq!(added_body["data"]["overview"]["recruitment"]["revision"], 1);
    assert_eq!(
        added_body["data"]["overview"]["recruitment"]["joinedCount"],
        10
    );
    assert_eq!(preferred_lanes(&pool, DUMMY_A).await?, vec!["MID"]);
    assert!(preferred_lanes(&pool, DUMMY_B).await?.is_empty());
    assert_eq!(audit_count(&pool).await?, 1);
    assert_eq!(outbox_count(&pool).await?, 1);
    let updates = recruitment_message_updates(&pool).await?;
    assert_eq!(updates.len(), 1);
    let add_update_key = format!("recruitment:{RECRUITMENT_ID}:message:update:1");
    let add_update = updates
        .iter()
        .find(|(key, _)| key == &add_update_key)
        .ok_or("missing dummy recruitment add message update")?;
    assert_eq!(
        add_update.1["snapshot"]["recruitment"]["revision"],
        json!(1)
    );
    assert_eq!(
        add_update.1["snapshot"]["recruitment"]["channelId"],
        json!(RECRUITMENT_CHANNEL_ID)
    );
    assert_eq!(
        add_update.1["snapshot"]["recruitment"]["messageId"],
        json!(RECRUITMENT_MESSAGE_ID)
    );
    let add_participants = add_update.1["snapshot"]["participants"]
        .as_array()
        .ok_or("dummy recruitment add notification did not contain participants")?;
    assert_eq!(add_participants.len(), 10);
    let added_dummy_a = add_participants
        .iter()
        .find(|participant| participant["id"] == DUMMY_A)
        .ok_or("dummy recruitment add notification omitted Dummy A")?;
    assert_eq!(added_dummy_a["preferredLanes"], json!(["MID"]));

    let replay = send(
        &app,
        Method::POST,
        &base,
        OPERATOR_SESSION,
        OPERATOR_CSRF,
        Some(add_body.clone()),
        true,
        Some("dummy-add"),
    )
    .await?;
    assert_eq!(replay.status(), StatusCode::OK);
    assert_eq!(body_json(replay).await?["data"], added_body["data"]);
    assert_eq!(audit_count(&pool).await?, 1);
    assert_eq!(recruitment_message_updates(&pool).await?.len(), 1);

    let conflict = send(
        &app,
        Method::POST,
        &base,
        OPERATOR_SESSION,
        OPERATOR_CSRF,
        Some(json!({ "userIds": [DUMMY_A], "expectedRevision": 1 })),
        true,
        Some("dummy-add"),
    )
    .await?;
    assert_error(conflict, StatusCode::CONFLICT, "idempotency_conflict").await?;

    let duplicate_preview = send(
        &app,
        Method::POST,
        &format!("{base}/preview"),
        OPERATOR_SESSION,
        OPERATOR_CSRF,
        Some(json!({ "action": "ADD", "userIds": [DUMMY_B] })),
        false,
        None,
    )
    .await?;
    let duplicate_preview = body_json(duplicate_preview).await?;
    assert_eq!(duplicate_preview["data"]["canApply"], false);
    assert_eq!(
        duplicate_preview["data"]["issues"][0]["code"],
        "already_participant"
    );

    let removed = send(
        &app,
        Method::DELETE,
        &base,
        OPERATOR_SESSION,
        OPERATOR_CSRF,
        Some(json!({ "userIds": [DUMMY_B], "expectedRevision": 1 })),
        true,
        Some("dummy-remove"),
    )
    .await?;
    assert_eq!(removed.status(), StatusCode::OK);
    let removed = body_json(removed).await?;
    assert_eq!(removed["data"]["action"], "REMOVE");
    assert_eq!(removed["data"]["overview"]["recruitment"]["revision"], 2);
    assert_eq!(removed["data"]["overview"]["recruitment"]["joinedCount"], 9);
    assert_eq!(participant_status(&pool, DUMMY_B).await?, "WITHDRAWN");
    assert!(preferred_lanes(&pool, DUMMY_B).await?.is_empty());
    assert_eq!(audit_count(&pool).await?, 2);
    let updates = recruitment_message_updates(&pool).await?;
    assert_eq!(updates.len(), 2);
    let remove_update_key = format!("recruitment:{RECRUITMENT_ID}:message:update:2");
    let remove_update = updates
        .iter()
        .find(|(key, _)| key == &remove_update_key)
        .ok_or("missing dummy recruitment remove message update")?;
    assert_eq!(
        remove_update.1["snapshot"]["recruitment"]["revision"],
        json!(2)
    );
    let remove_participants = remove_update.1["snapshot"]["participants"]
        .as_array()
        .ok_or("dummy recruitment remove notification did not contain participants")?;
    assert_eq!(remove_participants.len(), 9);
    assert!(
        remove_participants
            .iter()
            .all(|participant| participant["id"] != DUMMY_B)
    );

    let stale = send(
        &app,
        Method::POST,
        &base,
        OPERATOR_SESSION,
        OPERATOR_CSRF,
        Some(json!({ "userIds": [DUMMY_B], "expectedRevision": 1 })),
        true,
        Some("dummy-stale"),
    )
    .await?;
    assert_error(stale, StatusCode::CONFLICT, "revision_conflict").await?;

    let invalid_preview = send(
        &app,
        Method::POST,
        &format!("{base}/preview"),
        OPERATOR_SESSION,
        OPERATOR_CSRF,
        Some(json!({ "action": "ADD", "userIds": [DUMMY_C] })),
        false,
        None,
    )
    .await?;
    let invalid_preview = body_json(invalid_preview).await?;
    assert_eq!(invalid_preview["data"]["canApply"], false);
    assert_eq!(
        invalid_preview["data"]["issues"][0]["code"],
        "dummy_user_archived"
    );

    pool.close().await;
    drop(server);
    Ok(())
}

async fn seed(pool: &PgPool) -> TestResult {
    let now = OffsetDateTime::now_utc();
    sqlx::query(
        "INSERT INTO guilds (
             guild_id, discord_name, status, installed_by,
             installed_at, activated_at, updated_at
         ) VALUES ($1, 'Dummy Recruitment Guild', 'ACTIVE', $2, $3, $3, $3)",
    )
    .bind(GUILD_ID)
    .bind(OPERATOR_ID)
    .bind(now)
    .execute(pool)
    .await?;

    for (user_id, display_name) in [(OPERATOR_ID, "Operator"), (MEMBER_ID, "Member")] {
        insert_member(pool, user_id, display_name, "ACTIVE", None, now).await?;
    }
    sqlx::query(
        "INSERT INTO guild_config (
             guild_id, display_name, operator_role_id, is_externally_public,
             revision, updated_by, updated_at
         ) VALUES ($1, 'Dummy Recruitment Guild', $2, false, 0, $3, $4)",
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
    let season_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO seasons (
             guild_id, id, name, status, started_at, created_by, created_at
         ) VALUES ($1, $2, 'Season 1', 'ACTIVE', $3, $4, $3)",
    )
    .bind(GUILD_ID)
    .bind(season_id)
    .bind(now)
    .bind(OPERATOR_ID)
    .execute(pool)
    .await?;

    for (index, (user_id, display_name, profile_state, member_status, main_lane)) in [
        (DUMMY_A, "Dummy Mid", "ACTIVE", "ACTIVE", Some("MID")),
        (DUMMY_B, "Dummy Flex", "ACTIVE", "ACTIVE", None),
        (
            DUMMY_C,
            "Archived Dummy",
            "ARCHIVED",
            "INACTIVE",
            Some("TOP"),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        sqlx::query(
            "INSERT INTO users (
                 user_id, identity_kind, display_name, created_at, updated_at
             ) VALUES ($1, 'DUMMY', $2, $3, $3)",
        )
        .bind(user_id)
        .bind(display_name)
        .bind(now)
        .execute(pool)
        .await?;
        sqlx::query(
            "INSERT INTO dummy_user_profiles (
                 user_id, stable_key, state, avatar_seed, notes, revision,
                 created_by, updated_by, created_at, updated_at
             ) VALUES ($1, $2, $3, NULL, NULL, 0, $4, $4, $5, $5)",
        )
        .bind(user_id)
        .bind(format!("dummy-{index}"))
        .bind(profile_state)
        .bind(OPERATOR_ID)
        .bind(now)
        .execute(pool)
        .await?;
        insert_member(pool, user_id, display_name, member_status, main_lane, now).await?;
    }

    for index in 0..8 {
        let user_id = format!("1000000000000001{index:02}");
        sqlx::query(
            "INSERT INTO users (user_id, display_name, created_at, updated_at)
             VALUES ($1, $2, $3, $3)",
        )
        .bind(&user_id)
        .bind(format!("Filler {index}"))
        .bind(now)
        .execute(pool)
        .await?;
        insert_member(
            pool,
            &user_id,
            &format!("Filler {index}"),
            "ACTIVE",
            None,
            now,
        )
        .await?;
    }

    sqlx::query(
        "INSERT INTO recruitments (
             guild_id, id, season_id, display_number, title, target_count, status,
             revision, channel_id, message_id, created_by, created_at, updated_by, updated_at
         ) VALUES ($1, $2, $3, 1, 'Dummy Test Recruitment', 10, 'CLOSED',
                   0, $4, $5, $6, $7, $6, $7)",
    )
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(RECRUITMENT_ID)?)
    .bind(season_id)
    .bind(RECRUITMENT_CHANNEL_ID)
    .bind(RECRUITMENT_MESSAGE_ID)
    .bind(OPERATOR_ID)
    .bind(now)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO recruitment_entry_drafts (
             guild_id, recruitment_id, revision, updated_by, updated_at
         ) VALUES ($1, $2, 0, $3, $4)",
    )
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(RECRUITMENT_ID)?)
    .bind(OPERATOR_ID)
    .bind(now)
    .execute(pool)
    .await?;
    for index in 0..8 {
        let user_id = format!("1000000000000001{index:02}");
        sqlx::query(
            "INSERT INTO recruitment_participants (
                 guild_id, recruitment_id, user_id, status, joined_at
             ) VALUES ($1, $2, $3, 'JOINED', $4)",
        )
        .bind(GUILD_ID)
        .bind(Uuid::parse_str(RECRUITMENT_ID)?)
        .bind(user_id)
        .bind(now)
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn insert_member(
    pool: &PgPool,
    user_id: &str,
    display_name: &str,
    status: &str,
    main_lane: Option<&str>,
    now: OffsetDateTime,
) -> TestResult {
    sqlx::query(
        "INSERT INTO guild_members (
             guild_id, user_id, display_name, status, main_lane,
             first_seen_at, registered_at, last_active_at, last_synced_at,
             inactivated_at
         ) VALUES ($1, $2, $3, $4, $5, $6, $6, $6, $6,
                   CASE WHEN $4 = 'INACTIVE' THEN $6 ELSE NULL END)",
    )
    .bind(GUILD_ID)
    .bind(user_id)
    .bind(display_name)
    .bind(status)
    .bind(main_lane)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
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

async fn assert_error(response: Response<Body>, status: StatusCode, code: &str) -> TestResult {
    assert_eq!(response.status(), status);
    let body = body_json(response).await?;
    assert_eq!(body["error"]["code"], code, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .is_some_and(|v| !v.is_empty())
    );
    assert!(
        body["error"]["requestId"]
            .as_str()
            .is_some_and(|v| !v.is_empty())
    );
    Ok(())
}

async fn body_json(response: Response<Body>) -> TestResult<Value> {
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

async fn preferred_lanes(pool: &PgPool, user_id: &str) -> TestResult<Vec<String>> {
    Ok(sqlx::query_scalar::<_, String>(
        "SELECT lane FROM recruitment_preferred_lanes
         WHERE guild_id = $1 AND recruitment_id = $2 AND user_id = $3
         ORDER BY priority",
    )
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(RECRUITMENT_ID)?)
    .bind(user_id)
    .fetch_all(pool)
    .await?)
}

async fn participant_status(pool: &PgPool, user_id: &str) -> TestResult<String> {
    Ok(sqlx::query_scalar(
        "SELECT status FROM recruitment_participants
         WHERE guild_id = $1 AND recruitment_id = $2 AND user_id = $3",
    )
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(RECRUITMENT_ID)?)
    .bind(user_id)
    .fetch_one(pool)
    .await?)
}

async fn audit_count(pool: &PgPool) -> TestResult<i64> {
    Ok(sqlx::query_scalar(
        "SELECT count(*) FROM admin_audit_log
         WHERE guild_id = $1 AND action LIKE 'DUMMY_RECRUITMENT_PARTICIPANTS_%'",
    )
    .bind(GUILD_ID)
    .fetch_one(pool)
    .await?)
}

async fn outbox_count(pool: &PgPool) -> TestResult<i64> {
    Ok(sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events
         WHERE guild_id = $1 AND event_type = 'RECRUITMENT_PARTICIPANTS_CHANGED'",
    )
    .bind(GUILD_ID)
    .fetch_one(pool)
    .await?)
}

async fn recruitment_message_updates(pool: &PgPool) -> TestResult<Vec<(String, Value)>> {
    Ok(sqlx::query_as(
        "SELECT deduplication_key, payload
         FROM bot_notification_outbox
         WHERE guild_id = $1 AND notification_type = 'RECRUITMENT_MESSAGE_UPDATE'",
    )
    .bind(GUILD_ID)
    .fetch_all(pool)
    .await?)
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
        (
            "MONKEYBOT_SERVICE_ADMIN_IDS".to_owned(),
            format!("{OPERATOR_ID},{MEMBER_ID}"),
        ),
        (
            "MONKEYBOT_DUMMY_USERS_ENABLED".to_owned(),
            "true".to_owned(),
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
