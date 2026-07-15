use std::{collections::HashMap, error::Error, sync::Arc, time::Duration};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, Response, StatusCode},
};
use monkeybot_api::{
    AppState, build_router,
    config::Config,
    infrastructure::postgres::{MIGRATOR, PostgresReadinessProbe},
};
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
const POSTGRES_PASSWORD: &str = "monkeybot-user-profile-endpoints-password";
const DATABASE: &str = "monkeybot_user_profile_endpoints";
const TARGET_USER_ID: &str = "1525847677542207548";
const PRIVATE_USER_ID: &str = "1525847677542207549";
const UNKNOWN_USER_ID: &str = "1525847677542207550";
const ACTIVE_GUILD_A: &str = "200000000000000001";
const ACTIVE_GUILD_B: &str = "200000000000000002";
const INACTIVE_MEMBER_GUILD: &str = "200000000000000003";
const DISABLED_GUILD: &str = "200000000000000004";
const DISCORD_CLIENT_ID: &str = "900000000000000001";
const PUBLIC_ORIGIN: &str = "http://localhost:5173";
const OAUTH_KEY_BASE64URL: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn public_user_profile_projects_only_public_global_counts() -> TestResult {
    tokio::time::timeout(Duration::from_secs(120), run_flow())
        .await
        .map_err(|_| "user profile endpoint test exceeded 120 seconds")??;
    Ok(())
}

async fn run_flow() -> TestResult {
    let server = TestPostgres::start().await?;
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&server.database_url())
        .await?;
    MIGRATOR.run(&pool).await?;
    seed_data(&pool).await?;

    let config = test_config(server.database_url())?;
    let state = AppState::with_postgres(
        config,
        pool.clone(),
        Arc::new(PostgresReadinessProbe::new(
            pool.clone(),
            Duration::from_secs(1),
        )),
    );
    let app = build_router(state);

    let path = format!("/api/users/{TARGET_USER_ID}/profile");
    let response = get(&app, &path).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await?;
    assert_eq!(
        body["data"],
        json!({
            "discordId": TARGET_USER_ID,
            "displayName": "Longcat",
            "activeGuildCount": 2,
            "seasonCount": 3
        })
    );
    assert_eq!(body["meta"]["requestId"].as_str().is_some(), true);
    assert_profile_fields_are_minimal(&body)?;

    let invalid = get(&app, "/api/users/not-a-discord-id/profile").await?;
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    let invalid = body_json(invalid).await?;
    assert_eq!(invalid["error"]["code"], "invalid_discord_id");
    assert_eq!(
        invalid["error"]["message"],
        "Discord 사용자 ID 형식이 올바르지 않습니다."
    );

    let private = get(&app, &format!("/api/users/{PRIVATE_USER_ID}/profile")).await?;
    let unknown = get(&app, &format!("/api/users/{UNKNOWN_USER_ID}/profile")).await?;
    assert_hidden(private).await?;
    assert_hidden(unknown).await?;

    pool.close().await;
    drop(server);
    Ok(())
}

async fn seed_data(pool: &PgPool) -> TestResult {
    let now = OffsetDateTime::now_utc();
    insert_user(pool, TARGET_USER_ID, "Longcat", None, now).await?;
    insert_user(pool, PRIVATE_USER_ID, "Private Player", Some(false), now).await?;

    for (guild_id, status) in [
        (ACTIVE_GUILD_A, "ACTIVE"),
        (ACTIVE_GUILD_B, "ACTIVE"),
        (INACTIVE_MEMBER_GUILD, "ACTIVE"),
        (DISABLED_GUILD, "DISABLED"),
    ] {
        sqlx::query(
            "INSERT INTO guilds (
                 guild_id, discord_name, icon_hash, status, installed_by,
                 installed_at, activated_at, left_at, updated_at
             ) VALUES ($1, $2, NULL, $3, $4, $5, $5, NULL, $5)",
        )
        .bind(guild_id)
        .bind(format!("Guild {guild_id}"))
        .bind(status)
        .bind(TARGET_USER_ID)
        .bind(now)
        .execute(pool)
        .await?;
    }

    for (guild_id, user_id, status) in [
        (ACTIVE_GUILD_A, TARGET_USER_ID, "ACTIVE"),
        (ACTIVE_GUILD_B, TARGET_USER_ID, "ACTIVE"),
        (INACTIVE_MEMBER_GUILD, TARGET_USER_ID, "INACTIVE"),
        (DISABLED_GUILD, TARGET_USER_ID, "ACTIVE"),
        (ACTIVE_GUILD_A, PRIVATE_USER_ID, "ACTIVE"),
    ] {
        sqlx::query(
            "INSERT INTO guild_members (
                 guild_id, user_id, display_name, status, main_lane,
                 first_seen_at, registered_at, discord_joined_at,
                 last_active_at, last_synced_at, inactivated_at
             ) VALUES ($1, $2, $3, $4, NULL, $5, $5, $5, $5, $5,
                       CASE WHEN $4 = 'INACTIVE' THEN $5 ELSE NULL END)",
        )
        .bind(guild_id)
        .bind(user_id)
        .bind(if user_id == TARGET_USER_ID {
            "Longcat"
        } else {
            "Private Player"
        })
        .bind(status)
        .bind(now)
        .execute(pool)
        .await?;
    }

    for (guild_id, id, status) in [
        (
            ACTIVE_GUILD_A,
            "01900000-0000-7000-8000-000000000001",
            "ACTIVE",
        ),
        (
            ACTIVE_GUILD_A,
            "01900000-0000-7000-8000-000000000002",
            "CLOSED",
        ),
        (
            ACTIVE_GUILD_B,
            "01900000-0000-7000-8000-000000000003",
            "ACTIVE",
        ),
        (
            INACTIVE_MEMBER_GUILD,
            "01900000-0000-7000-8000-000000000004",
            "ACTIVE",
        ),
        (
            DISABLED_GUILD,
            "01900000-0000-7000-8000-000000000005",
            "ACTIVE",
        ),
    ] {
        let ended_at = (status == "CLOSED").then_some(now);
        sqlx::query(
            "INSERT INTO seasons (
                 guild_id, id, name, status, started_at, ended_at, created_by, created_at
             ) VALUES ($1, $2, '시즌', $3, $4, $5, NULL, $4)",
        )
        .bind(guild_id)
        .bind(Uuid::parse_str(id)?)
        .bind(status)
        .bind(now - time::Duration::days(1))
        .bind(ended_at)
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn insert_user(
    pool: &PgPool,
    user_id: &str,
    display_name: &str,
    public_profile_enabled: Option<bool>,
    now: OffsetDateTime,
) -> TestResult {
    sqlx::query(
        "INSERT INTO users (
             user_id, identity_kind, display_name, avatar_hash, created_at, updated_at, deleted_at
         ) VALUES ($1, 'DISCORD', $2, NULL, $3, $3, NULL)",
    )
    .bind(user_id)
    .bind(display_name)
    .bind(now)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO user_discord_identities (
             user_id, discord_id, identity_kind, created_at, updated_at
         ) VALUES ($1, $1, 'DISCORD', $2, $2)",
    )
    .bind(user_id)
    .bind(now)
    .execute(pool)
    .await?;
    if let Some(public_profile_enabled) = public_profile_enabled {
        sqlx::query(
            "INSERT INTO user_privacy_settings (
                 user_id, public_profile_enabled, show_riot_ids,
                 allow_external_record_search, updated_at
             ) VALUES ($1, $2, true, false, $3)",
        )
        .bind(user_id)
        .bind(public_profile_enabled)
        .bind(now)
        .execute(pool)
        .await?;
    }
    Ok(())
}

fn assert_profile_fields_are_minimal(body: &Value) -> TestResult {
    let object = body["data"]
        .as_object()
        .ok_or("profile data was not an object")?;
    let mut actual = object.keys().map(String::as_str).collect::<Vec<_>>();
    actual.sort_unstable();
    assert_eq!(
        actual,
        vec![
            "activeGuildCount",
            "discordId",
            "displayName",
            "seasonCount"
        ]
    );
    let serialized = body.to_string();
    for private_value in [ACTIVE_GUILD_A, ACTIVE_GUILD_B, "userId", "riot", "puuid"] {
        assert!(
            !serialized.contains(private_value),
            "public profile leaked {private_value}"
        );
    }
    Ok(())
}

async fn assert_hidden(response: Response<Body>) -> TestResult {
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = body_json(response).await?;
    assert_eq!(body["error"]["code"], "not_found");
    assert_eq!(
        body["error"]["message"],
        "요청한 사용자 프로필을 찾을 수 없습니다."
    );
    Ok(())
}

async fn get(app: &Router, path: &str) -> TestResult<Response<Body>> {
    Ok(app
        .clone()
        .oneshot(Request::get(path).body(Body::empty())?)
        .await?)
}

async fn body_json(response: Response<Body>) -> TestResult<Value> {
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    Ok(serde_json::from_slice(&bytes)?)
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
    ]);
    Ok(Config::from_map(&values)?)
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
