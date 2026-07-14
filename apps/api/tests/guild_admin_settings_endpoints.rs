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
const POSTGRES_PASSWORD: &str = "monkeybot-admin-settings-endpoint-password";
const DATABASE: &str = "monkeybot_admin_settings_endpoints";
const OPERATOR_ID: &str = "100000000000000001";
const MEMBER_ID: &str = "100000000000000002";
const GUILD_ID: &str = "200000000000000001";
const OLD_OPERATOR_ROLE_ID: &str = "300000000000000001";
const NEW_OPERATOR_ROLE_ID: &str = "300000000000000002";
const MANAGED_ROLE_ID: &str = "300000000000000003";
const RECRUITMENT_CHANNEL_ID: &str = "400000000000000001";
const ENTRY_CHANNEL_ID: &str = "400000000000000002";
const LOG_CHANNEL_ID: &str = "400000000000000003";
const REPLACEMENT_CHANNEL_ID: &str = "400000000000000004";
const RESULT_CHANNEL_ID: &str = "400000000000000005";
const STALE_CHANNEL_ID: &str = "400000000000000099";
const UNKNOWN_CHANNEL_ID: &str = "499999999999999999";
const DISCORD_CLIENT_ID: &str = "900000000000000001";
const PUBLIC_ORIGIN: &str = "http://localhost:5173";
const OAUTH_KEY_BASE64URL: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const BOT_SERVICE_TOKEN: &str = "bot-service-token-for-admin-settings-integration-test";
const OPERATOR_SESSION: &str = "admin-settings-operator-session";
const OPERATOR_CSRF: &str = "admin-settings-operator-csrf";
const MEMBER_SESSION: &str = "admin-settings-member-session";
const MEMBER_CSRF: &str = "admin-settings-member-csrf";

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn guild_settings_use_authoritative_discord_catalog_revision_and_audit() -> TestResult {
    tokio::time::timeout(Duration::from_secs(120), run_flow())
        .await
        .map_err(|_| "guild admin settings endpoint test exceeded 120 seconds")??;
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
    let settings_path = format!("/api/guilds/{GUILD_ID}/settings");
    let catalog_path = format!("/internal/guilds/{GUILD_ID}/discord-catalog");

    let unauthenticated = app
        .clone()
        .oneshot(Request::get(&settings_path).body(Body::empty())?)
        .await?;
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);

    let forbidden = browser_request(
        &app,
        Method::GET,
        &settings_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        None,
        false,
    )
    .await?;
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        body_json(forbidden).await?["error"]["code"],
        "operator_required"
    );

    let missing_bot_auth = app
        .clone()
        .oneshot(
            Request::put(&catalog_path)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&catalog_body())?))?,
        )
        .await?;
    assert_eq!(missing_bot_auth.status(), StatusCode::UNAUTHORIZED);

    let unknown_guild = bot_put(
        &app,
        "/internal/guilds/299999999999999999/discord-catalog",
        catalog_body(),
    )
    .await?;
    assert_eq!(unknown_guild.status(), StatusCode::NOT_FOUND);

    let everyone_catalog = bot_put(
        &app,
        &catalog_path,
        json!({
            "roles": [{
                "id": GUILD_ID,
                "name": "@everyone",
                "position": 0,
                "managed": false
            }],
            "channels": []
        }),
    )
    .await?;
    assert_eq!(everyone_catalog.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body_json(everyone_catalog).await?["error"]["code"],
        "invalid_discord_catalog"
    );

    let duplicate_catalog = bot_put(
        &app,
        &catalog_path,
        json!({
            "roles": [
                {"id": OLD_OPERATOR_ROLE_ID, "name": "Operators", "position": 10, "managed": false},
                {"id": OLD_OPERATOR_ROLE_ID, "name": "Duplicate", "position": 9, "managed": false}
            ],
            "channels": []
        }),
    )
    .await?;
    assert_eq!(duplicate_catalog.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(catalog_row_count(&pool).await?, (0, 0, 0));

    let synced = bot_put(&app, &catalog_path, catalog_body()).await?;
    assert_eq!(synced.status(), StatusCode::OK);
    let synced = body_json(synced).await?;
    let first_synced_at = synced["data"]["syncedAt"]
        .as_str()
        .ok_or("catalog sync timestamp missing")?;
    assert!(!first_synced_at.is_empty());

    let initial = operator_get(&app, &settings_path).await?;
    assert_eq!(initial.status(), StatusCode::OK);
    let initial = body_json(initial).await?;
    assert_eq!(initial["data"]["revision"], 0);
    assert_eq!(initial["data"]["catalogUpdatedAt"], first_synced_at);
    assert_eq!(
        initial["data"]["settings"],
        json!({
            "displayName": "Stored Guild",
            "operatorRoleId": OLD_OPERATOR_ROLE_ID,
            "recruitmentChannelId": STALE_CHANNEL_ID,
            "entryChannelId": null,
            "resultChannelId": null,
            "logChannelId": null,
            "isExternallyPublic": false
        })
    );
    assert_eq!(
        initial["data"]["roles"],
        json!([
            {"id": OLD_OPERATOR_ROLE_ID, "name": "내전 운영진"},
            {"id": NEW_OPERATOR_ROLE_ID, "name": "서버 관리자"}
        ])
    );
    assert!(
        initial["data"]["roles"]
            .as_array()
            .is_some_and(|roles| roles.iter().all(|role| role["id"] != MANAGED_ROLE_ID)),
        "managed roles must not be selectable"
    );
    assert_eq!(
        initial["data"]["channels"],
        json!([
            {"id": RECRUITMENT_CHANNEL_ID, "name": "내전-모집", "parentName": "MonkeyBot"},
            {"id": ENTRY_CHANNEL_ID, "name": "내전-엔트리", "parentName": "MonkeyBot"},
            {"id": LOG_CHANNEL_ID, "name": "내전-운영-로그", "parentName": null},
            {"id": RESULT_CHANNEL_ID, "name": "내전-결과", "parentName": "MonkeyBot"}
        ])
    );

    sqlx::query(
        "UPDATE guild_config
         SET recruitment_channel_id = NULL
         WHERE guild_id = $1",
    )
    .bind(GUILD_ID)
    .execute(&pool)
    .await?;
    let legacy_null_recruitment = operator_get(&app, &settings_path).await?;
    assert_eq!(legacy_null_recruitment.status(), StatusCode::OK);
    assert!(
        body_json(legacy_null_recruitment).await?["data"]["settings"]["recruitmentChannelId"]
            .is_null()
    );

    let update = update_body(0);
    let no_csrf = browser_request(
        &app,
        Method::PUT,
        &settings_path,
        OPERATOR_SESSION,
        OPERATOR_CSRF,
        Some(update.clone()),
        false,
    )
    .await?;
    assert_eq!(no_csrf.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        body_json(no_csrf).await?["error"]["code"],
        "csrf_validation_failed"
    );

    let season_field = browser_mutation(
        &app,
        &settings_path,
        json!({
            "displayName": "Updated Guild",
            "operatorRoleId": NEW_OPERATOR_ROLE_ID,
            "recruitmentChannelId": RECRUITMENT_CHANNEL_ID,
            "entryChannelId": ENTRY_CHANNEL_ID,
            "resultChannelId": RESULT_CHANNEL_ID,
            "logChannelId": LOG_CHANNEL_ID,
            "isExternallyPublic": true,
            "expectedRevision": 0,
            "seasonId": "01900000-0000-7000-8000-000000000001"
        }),
    )
    .await?;
    assert_eq!(season_field.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let mut missing_recruitment_channel_body = update.clone();
    missing_recruitment_channel_body["recruitmentChannelId"] = Value::Null;
    let missing_recruitment_channel =
        browser_mutation(&app, &settings_path, missing_recruitment_channel_body).await?;
    assert_eq!(
        missing_recruitment_channel.status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let missing_recruitment_channel = body_json(missing_recruitment_channel).await?;
    assert_eq!(
        missing_recruitment_channel["error"]["code"],
        "recruitment_channel_required"
    );
    assert_eq!(
        missing_recruitment_channel["error"]["message"],
        "모집 채널은 반드시 설정해야 합니다."
    );
    assert_eq!(
        missing_recruitment_channel["error"]["details"],
        json!({ "field": "recruitmentChannelId" })
    );
    assert_eq!(audit_count(&pool).await?, 0);

    let mut invalid_channel_body = update.clone();
    invalid_channel_body["resultChannelId"] = json!(UNKNOWN_CHANNEL_ID);
    let invalid_channel = browser_mutation(&app, &settings_path, invalid_channel_body).await?;
    assert_eq!(invalid_channel.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let invalid_channel = body_json(invalid_channel).await?;
    assert_eq!(
        invalid_channel["error"]["code"],
        "invalid_settings_selection"
    );
    assert_eq!(
        invalid_channel["error"]["details"]["field"],
        "resultChannelId"
    );

    let mut managed_role_body = update.clone();
    managed_role_body["operatorRoleId"] = json!(MANAGED_ROLE_ID);
    let managed_role = browser_mutation(&app, &settings_path, managed_role_body).await?;
    assert_eq!(managed_role.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body_json(managed_role).await?["error"]["details"]["field"],
        "operatorRoleId"
    );

    let updated = browser_mutation(&app, &settings_path, update.clone()).await?;
    assert_eq!(updated.status(), StatusCode::OK);
    let updated = body_json(updated).await?;
    assert_eq!(updated["data"]["revision"], 1);
    assert_eq!(updated["data"]["updatedBy"], OPERATOR_ID);
    assert_eq!(updated["data"]["settings"]["displayName"], "Updated Guild");
    assert_eq!(
        updated["data"]["settings"]["entryChannelId"],
        ENTRY_CHANNEL_ID
    );
    assert_eq!(
        updated["data"]["settings"]["resultChannelId"],
        RESULT_CHANNEL_ID
    );

    let stale = browser_mutation(&app, &settings_path, update).await?;
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    let stale = body_json(stale).await?;
    assert_eq!(stale["error"]["code"], "revision_conflict");
    assert_eq!(stale["error"]["details"]["currentRevision"], 1);
    assert_eq!(audit_count(&pool).await?, 1);
    assert_persisted_settings_and_audit(&pool).await?;

    let replaced = bot_put(
        &app,
        &catalog_path,
        json!({
            "roles": [{
                "id": OLD_OPERATOR_ROLE_ID,
                "name": "이전 운영진",
                "position": 1,
                "managed": false
            }],
            "channels": [{
                "id": REPLACEMENT_CHANNEL_ID,
                "name": "새-채널",
                "parentName": null,
                "position": 0,
                "kind": "TEXT"
            }]
        }),
    )
    .await?;
    assert_eq!(replaced.status(), StatusCode::OK);
    assert_eq!(catalog_row_count(&pool).await?, (1, 1, 1));

    let unchanged_missing_selections = browser_mutation(
        &app,
        &settings_path,
        json!({
            "displayName": "Renamed Only",
            "operatorRoleId": NEW_OPERATOR_ROLE_ID,
            "recruitmentChannelId": RECRUITMENT_CHANNEL_ID,
            "entryChannelId": ENTRY_CHANNEL_ID,
            "resultChannelId": RESULT_CHANNEL_ID,
            "logChannelId": LOG_CHANNEL_ID,
            "isExternallyPublic": true,
            "expectedRevision": 1
        }),
    )
    .await?;
    assert_eq!(unchanged_missing_selections.status(), StatusCode::OK);
    let unchanged_missing_selections = body_json(unchanged_missing_selections).await?;
    assert_eq!(unchanged_missing_selections["data"]["revision"], 2);
    assert_eq!(
        unchanged_missing_selections["data"]["roles"],
        json!([{"id": OLD_OPERATOR_ROLE_ID, "name": "이전 운영진"}])
    );
    assert_eq!(audit_count(&pool).await?, 2);

    pool.close().await;
    drop(server);
    Ok(())
}

fn catalog_body() -> Value {
    json!({
        "roles": [
            {"id": OLD_OPERATOR_ROLE_ID, "name": "내전 운영진", "position": 10, "managed": false},
            {"id": NEW_OPERATOR_ROLE_ID, "name": "서버 관리자", "position": 9, "managed": false},
            {"id": MANAGED_ROLE_ID, "name": "MonkeyBot", "position": 20, "managed": true}
        ],
        "channels": [
            {
                "id": RECRUITMENT_CHANNEL_ID,
                "name": "내전-모집",
                "parentName": "MonkeyBot",
                "position": 0,
                "kind": "TEXT"
            },
            {
                "id": ENTRY_CHANNEL_ID,
                "name": "내전-엔트리",
                "parentName": "MonkeyBot",
                "position": 1,
                "kind": "TEXT"
            },
            {
                "id": LOG_CHANNEL_ID,
                "name": "내전-운영-로그",
                "parentName": null,
                "position": 2,
                "kind": "ANNOUNCEMENT"
            },
            {
                "id": RESULT_CHANNEL_ID,
                "name": "내전-결과",
                "parentName": "MonkeyBot",
                "position": 3,
                "kind": "TEXT"
            }
        ]
    })
}

fn update_body(expected_revision: i64) -> Value {
    json!({
        "displayName": "  Updated Guild  ",
        "operatorRoleId": NEW_OPERATOR_ROLE_ID,
        "recruitmentChannelId": RECRUITMENT_CHANNEL_ID,
        "entryChannelId": ENTRY_CHANNEL_ID,
        "resultChannelId": RESULT_CHANNEL_ID,
        "logChannelId": LOG_CHANNEL_ID,
        "isExternallyPublic": true,
        "expectedRevision": expected_revision
    })
}

async fn operator_get(app: &Router, path: &str) -> TestResult<Response<Body>> {
    browser_request(
        app,
        Method::GET,
        path,
        OPERATOR_SESSION,
        OPERATOR_CSRF,
        None,
        false,
    )
    .await
}

async fn browser_mutation(app: &Router, path: &str, body: Value) -> TestResult<Response<Body>> {
    browser_request(
        app,
        Method::PUT,
        path,
        OPERATOR_SESSION,
        OPERATOR_CSRF,
        Some(body),
        true,
    )
    .await
}

async fn browser_request(
    app: &Router,
    method: Method,
    path: &str,
    session: &str,
    csrf: &str,
    body: Option<Value>,
    include_csrf: bool,
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
        Body::from(serde_json::to_vec(&body)?)
    } else {
        Body::empty()
    };
    Ok(app.clone().oneshot(request.body(request_body)?).await?)
}

async fn bot_put(app: &Router, path: &str, body: Value) -> TestResult<Response<Body>> {
    let request = Request::put(path)
        .header(header::AUTHORIZATION, format!("Bearer {BOT_SERVICE_TOKEN}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body)?))?;
    Ok(app.clone().oneshot(request).await?)
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
         ) VALUES ($1, 'Admin Settings Guild', NULL, 'ACTIVE', $2, $3, $3, NULL, $3)",
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
             entry_channel_id, result_channel_id, log_channel_id, is_externally_public, revision,
             updated_by, updated_at
         ) VALUES ($1, 'Stored Guild', $2, $3, NULL, NULL, NULL, false, 0, $4, $5)",
    )
    .bind(GUILD_ID)
    .bind(OLD_OPERATOR_ROLE_ID)
    .bind(STALE_CHANNEL_ID)
    .bind(OPERATOR_ID)
    .bind(now)
    .execute(pool)
    .await?;
    for role_id in [OLD_OPERATOR_ROLE_ID, NEW_OPERATOR_ROLE_ID] {
        sqlx::query(
            "INSERT INTO guild_member_discord_roles (guild_id, user_id, role_id, last_synced_at)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(GUILD_ID)
        .bind(OPERATOR_ID)
        .bind(role_id)
        .bind(now)
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn catalog_row_count(pool: &PgPool) -> TestResult<(i64, i64, i64)> {
    Ok(sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT
           (SELECT count(*) FROM guild_discord_roles WHERE guild_id = $1),
           (SELECT count(*) FROM guild_discord_channels WHERE guild_id = $1),
           (SELECT count(*) FROM guild_discord_catalog_state WHERE guild_id = $1)",
    )
    .bind(GUILD_ID)
    .fetch_one(pool)
    .await?)
}

async fn audit_count(pool: &PgPool) -> TestResult<i64> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM admin_audit_log
         WHERE guild_id = $1 AND action = 'GUILD_SETTINGS_UPDATED'",
    )
    .bind(GUILD_ID)
    .fetch_one(pool)
    .await?)
}

async fn assert_persisted_settings_and_audit(pool: &PgPool) -> TestResult {
    let row = sqlx::query_as::<
        _,
        (
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            bool,
            i64,
            Option<String>,
        ),
    >(
        "SELECT display_name, operator_role_id, recruitment_channel_id, entry_channel_id,
                result_channel_id, log_channel_id, is_externally_public, revision, updated_by
         FROM guild_config WHERE guild_id = $1",
    )
    .bind(GUILD_ID)
    .fetch_one(pool)
    .await?;
    assert_eq!(
        row,
        (
            "Updated Guild".to_owned(),
            NEW_OPERATOR_ROLE_ID.to_owned(),
            Some(RECRUITMENT_CHANNEL_ID.to_owned()),
            Some(ENTRY_CHANNEL_ID.to_owned()),
            Some(RESULT_CHANNEL_ID.to_owned()),
            Some(LOG_CHANNEL_ID.to_owned()),
            true,
            1,
            Some(OPERATOR_ID.to_owned()),
        )
    );

    let (action, target_type, target_id, before, after) =
        sqlx::query_as::<_, (String, Option<String>, Option<String>, Value, Value)>(
            "SELECT action, target_type, target_id, before_summary, after_summary
             FROM admin_audit_log
             WHERE guild_id = $1 AND action = 'GUILD_SETTINGS_UPDATED'",
        )
        .bind(GUILD_ID)
        .fetch_one(pool)
        .await?;
    assert_eq!(action, "GUILD_SETTINGS_UPDATED");
    assert_eq!(target_type.as_deref(), Some("GUILD_CONFIG"));
    assert_eq!(target_id.as_deref(), Some(GUILD_ID));
    assert_eq!(before["displayName"], "Stored Guild");
    assert!(before["recruitmentChannelId"].is_null());
    assert_eq!(after["displayName"], "Updated Guild");
    assert_eq!(after["entryChannelId"], ENTRY_CHANNEL_ID);
    assert_eq!(after["resultChannelId"], RESULT_CHANNEL_ID);
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
        (
            "MONKEYBOT_BOT_SERVICE_TOKEN".to_owned(),
            BOT_SERVICE_TOKEN.to_owned(),
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
