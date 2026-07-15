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
const POSTGRES_PASSWORD: &str = "monkeybot-data-transfer-endpoint-password";
const DATABASE: &str = "monkeybot_data_transfer_endpoints";
const OPERATOR_ID: &str = "100000000000000001";
const MEMBER_ID: &str = "100000000000000002";
const GUILD_ID: &str = "200000000000000001";
const OPERATOR_ROLE_ID: &str = "300000000000000001";
const DISCORD_CLIENT_ID: &str = "900000000000000001";
const PUBLIC_ORIGIN: &str = "http://localhost:5173";
const OAUTH_KEY_BASE64URL: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const OPERATOR_SESSION: &str = "data-transfer-operator-session";
const OPERATOR_CSRF: &str = "data-transfer-operator-csrf";
const SEASON_ID: &str = "01900000-0000-7000-8000-000000000011";
const CONFLICT_IDEMPOTENCY_KEY: &str = "data-transfer-conflict-preview";
const STALE_IDEMPOTENCY_KEY: &str = "data-transfer-stale-mmr";
const APPLY_IDEMPOTENCY_KEY: &str = "data-transfer-apply";

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn guild_data_transfer_exports_previews_and_applies_mmr_seed_idempotently() -> TestResult {
    tokio::time::timeout(Duration::from_secs(120), run_flow())
        .await
        .map_err(|_| "guild data-transfer endpoint test exceeded 120 seconds")??;
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
        "data-transfer-member-session",
        "data-transfer-member-csrf",
    )
    .await?;
    seed_guild(&pool).await?;
    seed_member_mmr(&pool, 1600.0).await?;

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
    let export_path = format!("/api/guilds/{GUILD_ID}/data-export");
    let preview_path = format!("/api/guilds/{GUILD_ID}/data-imports/preview");

    let exported = operator_post(&app, &export_path, None, None).await?;
    assert_eq!(exported.status(), StatusCode::OK);
    let exported = body_json(exported).await?;
    let archive = exported["data"]["archive"].clone();
    assert_eq!(archive["format"], "MONKEYBOT_DATA_TRANSFER");
    assert_eq!(archive["kind"], "MEMBER_SEASON_MMR_SEED");
    assert_eq!(archive["schemaVersion"], 1);
    assert_eq!(archive["source"]["season"]["id"], SEASON_ID);
    assert!(archive.get("signature").is_none());
    assert_exported_member(&archive)?;

    set_member_mmr(&pool, 1500.0).await?;
    let mut edited_archive = archive.clone();
    set_archive_member_mmr(&mut edited_archive, MEMBER_ID, 1650.0)?;

    set_member_status(&pool, "INACTIVE").await?;
    let conflict_preview = preview_archive(&app, &preview_path, edited_archive.clone()).await?;
    assert_eq!(conflict_preview["summary"]["matchedMembers"], 1);
    assert_eq!(
        conflict_preview["conflicts"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        conflict_preview["conflicts"][0]["reason"],
        "member_not_active"
    );
    let conflict_apply = apply_preview(&app, &conflict_preview, CONFLICT_IDEMPOTENCY_KEY).await?;
    assert_error(
        conflict_apply,
        StatusCode::CONFLICT,
        "preview_conflicts",
        "대상 길드 멤버 상태가 검토 결과와 달라져 가져올 수 없습니다.",
    )
    .await?;

    set_member_status(&pool, "ACTIVE").await?;
    let stale_preview = preview_archive(&app, &preview_path, edited_archive.clone()).await?;
    assert_eq!(stale_preview["summary"]["changes"], 5);
    set_member_lane_mmr(&pool, "MID", 1499.0).await?;
    let stale_apply = apply_preview(&app, &stale_preview, STALE_IDEMPOTENCY_KEY).await?;
    assert_error(
        stale_apply,
        StatusCode::CONFLICT,
        "mmr_state_changed",
        "대상 길드의 MMR이 변경되었습니다. 파일을 다시 검증해 주세요.",
    )
    .await?;

    set_member_lane_mmr(&pool, "MID", 1500.0).await?;
    let successful_preview = preview_archive(&app, &preview_path, edited_archive).await?;
    assert_eq!(successful_preview["summary"]["changes"], 5);
    assert_eq!(successful_preview["conflicts"], json!([]));
    let applied = apply_preview(&app, &successful_preview, APPLY_IDEMPOTENCY_KEY).await?;
    assert_eq!(applied.status(), StatusCode::OK);
    let applied = body_json(applied).await?;
    assert_eq!(applied["data"]["status"], "APPLIED");
    assert_eq!(applied["data"]["summary"]["changes"], 5);
    assert_eq!(applied["data"]["targetRevision"], 1);
    assert_applied_effects(&pool, &successful_preview, 1650.0).await?;

    let replay = apply_preview(&app, &successful_preview, APPLY_IDEMPOTENCY_KEY).await?;
    assert_eq!(replay.status(), StatusCode::OK);
    let replay = body_json(replay).await?;
    assert_eq!(replay["data"], applied["data"]);
    assert_eq!(data_import_ledger_count(&pool).await?, 5);

    pool.close().await;
    drop(server);
    Ok(())
}

fn assert_exported_member(archive: &Value) -> TestResult {
    let members = archive["members"]
        .as_array()
        .ok_or("export archive members are missing")?;
    let member = members
        .iter()
        .find(|member| member["discordId"] == MEMBER_ID)
        .ok_or("export archive member is missing")?;
    assert_eq!(member["displayName"], "Member");
    assert_eq!(member["lanes"].as_array().map(Vec::len), Some(5));
    assert!(
        member["lanes"]
            .as_array()
            .is_some_and(|lanes| lanes.iter().all(|lane| lane["mmr"] == 1600.0))
    );
    Ok(())
}

fn set_archive_member_mmr(archive: &mut Value, discord_id: &str, mmr: f64) -> TestResult {
    let members = archive["members"]
        .as_array_mut()
        .ok_or("export archive members are missing")?;
    let member = members
        .iter_mut()
        .find(|member| member["discordId"] == discord_id)
        .ok_or("export archive member is missing")?;
    let lanes = member["lanes"]
        .as_array_mut()
        .ok_or("export archive member lanes are missing")?;
    if lanes.len() != 5 {
        return Err("export archive member lane count is invalid".into());
    }
    for lane in lanes {
        lane["mmr"] = json!(mmr);
    }
    Ok(())
}

async fn preview_archive(app: &Router, path: &str, archive: Value) -> TestResult<Value> {
    let preview = operator_post(app, path, Some(json!({"archive": archive})), None).await?;
    assert_eq!(preview.status(), StatusCode::OK);
    Ok(body_json(preview).await?["data"].clone())
}

async fn apply_preview(
    app: &Router,
    preview: &Value,
    idempotency_key: &str,
) -> TestResult<Response<Body>> {
    let import_id = preview["importId"]
        .as_str()
        .ok_or("import preview id is missing")?;
    let path = format!("/api/guilds/{GUILD_ID}/data-imports/{import_id}/apply");
    operator_post(
        app,
        &path,
        Some(json!({
            "expectedDigest": preview["digest"],
            "expectedTargetRevision": preview["targetRevision"],
        })),
        Some(idempotency_key),
    )
    .await
}

async fn operator_post(
    app: &Router,
    path: &str,
    body: Option<Value>,
    idempotency_key: Option<&str>,
) -> TestResult<Response<Body>> {
    let mut request = Request::builder()
        .method(Method::POST)
        .uri(path)
        .header(
            header::COOKIE,
            format!("monkeybot_session={OPERATOR_SESSION}; monkeybot_csrf={OPERATOR_CSRF}"),
        )
        .header(header::ORIGIN, PUBLIC_ORIGIN)
        .header("x-csrf-token", OPERATOR_CSRF);
    if let Some(key) = idempotency_key {
        request = request.header("idempotency-key", key);
    }
    let body = if let Some(body) = body {
        request = request.header(header::CONTENT_TYPE, "application/json");
        Body::from(serde_json::to_vec(&body)?)
    } else {
        Body::empty()
    };
    Ok(app.clone().oneshot(request.body(body)?).await?)
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

async fn seed_guild(pool: &PgPool) -> TestResult {
    let now = OffsetDateTime::now_utc();
    let season_id = Uuid::parse_str(SEASON_ID)?;
    sqlx::query(
        "INSERT INTO guilds (
             guild_id, discord_name, icon_hash, status, installed_by,
             installed_at, activated_at, left_at, updated_at
         ) VALUES ($1, 'Data Transfer Test Guild', NULL, 'ACTIVE', $2, $3, $3, NULL, $3)",
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
             log_channel_id, is_externally_public, revision, updated_by, updated_at
         ) VALUES ($1, 'Data Transfer Test Guild', $2, NULL, NULL, false, 0, $3, $4)",
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
    .bind(season_id)
    .bind(now)
    .bind(OPERATOR_ID)
    .execute(pool)
    .await?;
    Ok(())
}

async fn seed_member_mmr(pool: &PgPool, mmr: f64) -> TestResult {
    let now = OffsetDateTime::now_utc();
    let season_id = Uuid::parse_str(SEASON_ID)?;
    for lane in ["TOP", "JUNGLE", "MID", "BOTTOM", "SUPPORT"] {
        sqlx::query(
            "INSERT INTO user_lane_mmr (
                 guild_id, season_id, user_id, lane, mmr, games_played, wins, revision, updated_at
             ) VALUES ($1, $2, $3, $4, $5, 0, 0, 0, $6)",
        )
        .bind(GUILD_ID)
        .bind(season_id)
        .bind(MEMBER_ID)
        .bind(lane)
        .bind(mmr)
        .bind(now)
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn set_member_mmr(pool: &PgPool, mmr: f64) -> TestResult {
    sqlx::query(
        "UPDATE user_lane_mmr
         SET mmr = $1, updated_at = CURRENT_TIMESTAMP
         WHERE guild_id = $2 AND season_id = $3 AND user_id = $4",
    )
    .bind(mmr)
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(SEASON_ID)?)
    .bind(MEMBER_ID)
    .execute(pool)
    .await?;
    Ok(())
}

async fn set_member_lane_mmr(pool: &PgPool, lane: &str, mmr: f64) -> TestResult {
    sqlx::query(
        "UPDATE user_lane_mmr
         SET mmr = $1, updated_at = CURRENT_TIMESTAMP
         WHERE guild_id = $2 AND season_id = $3 AND user_id = $4 AND lane = $5",
    )
    .bind(mmr)
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(SEASON_ID)?)
    .bind(MEMBER_ID)
    .bind(lane)
    .execute(pool)
    .await?;
    Ok(())
}

async fn set_member_status(pool: &PgPool, status: &str) -> TestResult {
    sqlx::query(
        "UPDATE guild_members
         SET status = $1,
             inactivated_at = CASE WHEN $1 = 'INACTIVE' THEN CURRENT_TIMESTAMP ELSE NULL END
         WHERE guild_id = $2 AND user_id = $3",
    )
    .bind(status)
    .bind(GUILD_ID)
    .bind(MEMBER_ID)
    .execute(pool)
    .await?;
    Ok(())
}

async fn assert_applied_effects(pool: &PgPool, preview: &Value, expected_mmr: f64) -> TestResult {
    let mmr_rows = sqlx::query_as::<_, (String, f64, i32, i32, i64)>(
        "SELECT lane, mmr, games_played, wins, revision
         FROM user_lane_mmr
         WHERE guild_id = $1 AND season_id = $2 AND user_id = $3
         ORDER BY lane",
    )
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(SEASON_ID)?)
    .bind(MEMBER_ID)
    .fetch_all(pool)
    .await?;
    assert_eq!(mmr_rows.len(), 5);
    assert!(mmr_rows.iter().all(|(_, mmr, games, wins, revision)| (
        *mmr, *games, *wins, *revision
    ) == (expected_mmr, 0, 0, 1)));

    let ledger_rows = sqlx::query_as::<_, (String, i16, i16, f64, f64)>(
        "SELECT source_type, games_played_delta, wins_delta, mmr_before, mmr_after
         FROM mmr_ledger_entries
         WHERE guild_id = $1 AND season_id = $2 AND user_id = $3
         ORDER BY lane",
    )
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(SEASON_ID)?)
    .bind(MEMBER_ID)
    .fetch_all(pool)
    .await?;
    assert_eq!(ledger_rows.len(), 5);
    assert!(
        ledger_rows
            .iter()
            .all(|(source_type, games_delta, wins_delta, before, after)| {
                source_type == "DATA_IMPORT_SEED"
                    && (*games_delta, *wins_delta, *before, *after) == (0, 0, 1500.0, expected_mmr)
            })
    );

    let import_id = Uuid::parse_str(
        preview["importId"]
            .as_str()
            .ok_or("successful import preview id is missing")?,
    )?;
    let import = sqlx::query_as::<_, (String, bool)>(
        "SELECT status, archive_payload IS NULL
         FROM guild_data_imports
         WHERE guild_id = $1 AND id = $2",
    )
    .bind(GUILD_ID)
    .bind(import_id)
    .fetch_one(pool)
    .await?;
    assert_eq!(import, ("APPLIED".to_owned(), true));
    Ok(())
}

async fn data_import_ledger_count(pool: &PgPool) -> TestResult<i64> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT count(*)
         FROM mmr_ledger_entries
         WHERE guild_id = $1 AND source_type = 'DATA_IMPORT_SEED'",
    )
    .bind(GUILD_ID)
    .fetch_one(pool)
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
