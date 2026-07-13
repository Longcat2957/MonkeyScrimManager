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
const POSTGRES_PASSWORD: &str = "monkeybot-search-suggestions-password";
const DATABASE: &str = "monkeybot_search_suggestions";

const VIEWER_ID: &str = "100000000000000001";
const OUTSIDER_ID: &str = "100000000000000002";
const ALPHA_ID: &str = "100000000000000003";
const ALPINE_ID: &str = "100000000000000004";
const ALBATROSS_ID: &str = "100000000000000005";
const INACTIVE_ID: &str = "100000000000000006";
const OTHER_GUILD_ID: &str = "100000000000000007";
const GUILD_A: &str = "200000000000000001";
const GUILD_B: &str = "200000000000000002";
const DISCORD_CLIENT_ID: &str = "900000000000000001";
const PUBLIC_ORIGIN: &str = "http://localhost:5173";
const OAUTH_KEY_BASE64URL: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const VIEWER_SESSION_TOKEN: &str = "suggestions-viewer-session-token";
const OUTSIDER_SESSION_TOKEN: &str = "suggestions-outsider-session-token";

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn suggestions_union_public_riot_ids_and_gate_guild_linkage() -> TestResult {
    tokio::time::timeout(Duration::from_secs(120), run_suggestion_flow())
        .await
        .map_err(|_| "search suggestion endpoint integration test exceeded 120 seconds")??;
    Ok(())
}

async fn run_suggestion_flow() -> TestResult {
    let server = TestPostgres::start().await?;
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&server.database_url())
        .await?;
    MIGRATOR.run(&pool).await?;

    let config = test_config(server.database_url())?;
    let auth_config = config.auth().cloned().expect("test auth configuration");
    let auth_service = Arc::new(AuthService::new(
        auth_config.oauth_encryption_key(),
        CookiePolicy::new(auth_config.secure_cookies(), auth_config.session_ttl()),
    )?);
    let auth_repository = Arc::new(AuthRepository::new(pool.clone()));
    seed_login(&auth_repository, VIEWER_ID, "Viewer", VIEWER_SESSION_TOKEN).await?;
    seed_login(
        &auth_repository,
        OUTSIDER_ID,
        "Outsider",
        OUTSIDER_SESSION_TOKEN,
    )
    .await?;
    seed_search_data(&pool).await?;

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
        Arc::new(UnusedDiscordClient),
    ));
    let app = build_router(state);

    assert_public_riot_suggestions(&app).await?;
    assert_guild_member_suggestions(&app).await?;

    pool.close().await;
    drop(server);
    Ok(())
}

async fn assert_public_riot_suggestions(app: &Router) -> TestResult {
    let too_short = get(app, "/api/riot/records/suggestions?q=l", None).await?;
    assert_eq!(too_short.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        body_json(too_short).await?["error"]["code"],
        "invalid_search_query"
    );

    let too_many = get(
        app,
        "/api/riot/records/suggestions?q=lo&platform=KR&limit=9",
        None,
    )
    .await?;
    assert_eq!(too_many.status(), StatusCode::BAD_REQUEST);

    let response = get(
        app,
        "/api/riot/records/suggestions?q=lo&platform=KR&limit=8",
        None,
    )
    .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await?;
    assert_eq!(
        body["data"]["suggestions"],
        json!([
            {
                "kind": "riotAccount",
                "value": "LoneWolf#KR5",
                "label": "LoneWolf#KR5",
                "description": "KR",
                "profileIconId": 12
            },
            {
                "kind": "riotAccount",
                "value": "Longcat#KR1",
                "label": "Longcat#KR1",
                "description": "KR",
                "profileIconId": 77
            },
            {
                "kind": "riotAccount",
                "value": "LongHidden#KR3",
                "label": "LongHidden#KR3",
                "description": "KR",
                "profileIconId": 9
            },
            {
                "kind": "riotAccount",
                "value": "LongRoad#KR2",
                "label": "LongRoad#KR2",
                "description": "KR",
                "profileIconId": 8
            }
        ])
    );
    assert_private_identifiers_absent(&body);

    let full_id = get(
        app,
        "/api/riot/records/suggestions?q=longcat%23kr&platform=KR",
        None,
    )
    .await?;
    assert_eq!(full_id.status(), StatusCode::OK);
    assert_eq!(
        body_json(full_id).await?["data"]["suggestions"][0]["value"],
        "Longcat#KR1"
    );
    Ok(())
}

async fn assert_guild_member_suggestions(app: &Router) -> TestResult {
    let uri = format!("/api/guilds/{GUILD_A}/records/suggestions?q=al&limit=8");
    let unauthenticated = get(app, &uri, None).await?;
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);

    let outsider_cookie = format!("monkeybot_session={OUTSIDER_SESSION_TOKEN}");
    let outsider = get(app, &uri, Some(&outsider_cookie)).await?;
    assert_eq!(outsider.status(), StatusCode::NOT_FOUND);

    let viewer_cookie = format!("monkeybot_session={VIEWER_SESSION_TOKEN}");
    let response = get(app, &uri, Some(&viewer_cookie)).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await?;
    assert_eq!(
        body["data"]["suggestions"],
        json!([
            {
                "kind": "guildMember",
                "value": "Albatross",
                "label": "Albatross",
                "description": null,
                "profileIconId": null
            },
            {
                "kind": "guildMember",
                "value": "Alpha",
                "label": "Alpha",
                "description": "Longcat#KR1",
                "profileIconId": 7
            },
            {
                "kind": "guildMember",
                "value": "Alpine",
                "label": "Alpine",
                "description": "LongRoad#KR2",
                "profileIconId": 8
            }
        ])
    );
    assert_private_identifiers_absent(&body);
    Ok(())
}

fn assert_private_identifiers_absent(body: &Value) {
    let serialized = body.to_string();
    for private in [
        "puuid-visible-longcat",
        "puuid-private-external",
        "snapshot-only-puuid",
        "snapshot-duplicate-puuid",
        VIEWER_ID,
        ALPHA_ID,
    ] {
        assert!(
            !serialized.contains(private),
            "leaked private value {private}"
        );
    }
    assert!(!serialized.contains("puuid"));
    assert!(!serialized.contains("userId"));
}

async fn seed_login(
    repository: &AuthRepository,
    user_id: &str,
    display_name: &str,
    session_token: &str,
) -> TestResult {
    repository
        .complete_login(&NewLogin {
            discord_user_id: DiscordUserId::parse(user_id)?,
            display_name: display_name.to_owned(),
            avatar_hash: None,
            previous_session_token_hash: None,
            session_id: Uuid::now_v7(),
            token_hash: AuthService::hash_secret(session_token),
            csrf_secret_hash: AuthService::hash_secret("unused-csrf-token"),
            expires_at: OffsetDateTime::now_utc() + time::Duration::hours(1),
        })
        .await?;
    Ok(())
}

async fn seed_search_data(pool: &PgPool) -> TestResult {
    for (user_id, display_name) in [
        (ALPHA_ID, "Alpha User"),
        (ALPINE_ID, "Alpine User"),
        (ALBATROSS_ID, "Albatross User"),
        (INACTIVE_ID, "Inactive User"),
        (OTHER_GUILD_ID, "Other User"),
    ] {
        sqlx::query(
            "INSERT INTO users (
                 user_id, display_name, avatar_hash, created_at, updated_at, deleted_at
             ) VALUES ($1, $2, NULL, clock_timestamp(), clock_timestamp(), NULL)",
        )
        .bind(user_id)
        .bind(display_name)
        .execute(pool)
        .await?;
    }

    for guild_id in [GUILD_A, GUILD_B] {
        sqlx::query(
            "INSERT INTO guilds (
                 guild_id, discord_name, icon_hash, status, installed_by,
                 installed_at, activated_at, left_at, updated_at
             ) VALUES (
                 $1, $2, NULL, 'ACTIVE', $3,
                 clock_timestamp(), clock_timestamp(), NULL, clock_timestamp()
             )",
        )
        .bind(guild_id)
        .bind(format!("Guild {guild_id}"))
        .bind(VIEWER_ID)
        .execute(pool)
        .await?;
    }

    for (guild_id, user_id, display_name, status) in [
        (GUILD_A, VIEWER_ID, "Viewer", "ACTIVE"),
        (GUILD_A, ALPHA_ID, "Alpha", "ACTIVE"),
        (GUILD_A, ALPINE_ID, "Alpine", "ACTIVE"),
        (GUILD_A, ALBATROSS_ID, "Albatross", "ACTIVE"),
        (GUILD_A, INACTIVE_ID, "Alchemy", "INACTIVE"),
        (GUILD_B, OUTSIDER_ID, "Outsider", "ACTIVE"),
        (GUILD_B, OTHER_GUILD_ID, "Alone", "ACTIVE"),
    ] {
        sqlx::query(
            "INSERT INTO guild_members (
                 guild_id, user_id, display_name, status, main_lane,
                 first_seen_at, registered_at, discord_joined_at,
                 last_active_at, last_synced_at, inactivated_at
             ) VALUES (
                 $1, $2, $3, $4, NULL,
                 clock_timestamp(), clock_timestamp(), clock_timestamp(),
                 clock_timestamp(), clock_timestamp(),
                 CASE WHEN $4 = 'INACTIVE' THEN clock_timestamp() ELSE NULL END
             )",
        )
        .bind(guild_id)
        .bind(user_id)
        .bind(display_name)
        .bind(status)
        .execute(pool)
        .await?;
    }

    for (user_id, show_riot_ids, allow_external_record_search) in [
        (ALPHA_ID, true, true),
        (ALPINE_ID, true, false),
        (ALBATROSS_ID, false, true),
        (OTHER_GUILD_ID, true, true),
    ] {
        sqlx::query(
            "INSERT INTO user_privacy_settings (
                 user_id, public_profile_enabled, show_riot_ids,
                 allow_external_record_search, updated_at
             ) VALUES ($1, true, $2, $3, clock_timestamp())",
        )
        .bind(user_id)
        .bind(show_riot_ids)
        .bind(allow_external_record_search)
        .execute(pool)
        .await?;
    }

    for (
        puuid,
        user_id,
        platform,
        routing_region,
        game_name,
        tag_line,
        normalized_game_name,
        normalized_tag_line,
        profile_icon_id,
    ) in [
        (
            "puuid-visible-longcat",
            ALPHA_ID,
            "KR",
            "ASIA",
            "Longcat",
            "KR1",
            "longcat",
            "kr1",
            7,
        ),
        (
            "puuid-private-external",
            ALPINE_ID,
            "KR",
            "ASIA",
            "LongRoad",
            "KR2",
            "longroad",
            "kr2",
            8,
        ),
        (
            "puuid-private-riot-id",
            ALBATROSS_ID,
            "KR",
            "ASIA",
            "LongHidden",
            "KR3",
            "longhidden",
            "kr3",
            9,
        ),
        (
            "puuid-other-platform",
            OTHER_GUILD_ID,
            "JP1",
            "ASIA",
            "Lollipop",
            "JP1",
            "lollipop",
            "jp1",
            10,
        ),
    ] {
        sqlx::query(
            "INSERT INTO riot_accounts (
                 puuid, user_id, platform, routing_region, game_name, tag_line,
                 normalized_game_name, normalized_tag_line, profile_icon_id,
                 is_main, main_position, main_position_updated_at,
                 last_synced_at, created_at, updated_at
             ) VALUES (
                 $1, $2, $3, $4, $5, $6,
                 $7, $8, $9,
                 true, NULL, NULL,
                 clock_timestamp(), clock_timestamp(), clock_timestamp()
             )",
        )
        .bind(puuid)
        .bind(user_id)
        .bind(platform)
        .bind(routing_region)
        .bind(game_name)
        .bind(tag_line)
        .bind(normalized_game_name)
        .bind(normalized_tag_line)
        .bind(profile_icon_id)
        .execute(pool)
        .await?;
    }

    for (puuid, game_name, tag_line, normalized_game_name, normalized_tag_line, profile_icon_id) in [
        (
            "snapshot-only-puuid",
            "LoneWolf",
            "KR5",
            "lonewolf",
            "kr5",
            12,
        ),
        (
            "snapshot-duplicate-puuid",
            "Longcat",
            "KR1",
            "longcat",
            "kr1",
            77,
        ),
    ] {
        sqlx::query(
            "INSERT INTO riot_records_snapshots (
                 platform, puuid, routing_region, game_name, tag_line,
                 normalized_game_name, normalized_tag_line, schema_version, snapshot,
                 refreshed_at, created_at, updated_at
             ) VALUES (
                 'KR', $1, 'ASIA', $2, $3,
                 $4, $5, 1, jsonb_build_object('profileIconId', $6::integer),
                 clock_timestamp(), clock_timestamp(), clock_timestamp() + interval '1 minute'
             )",
        )
        .bind(puuid)
        .bind(game_name)
        .bind(tag_line)
        .bind(normalized_game_name)
        .bind(normalized_tag_line)
        .bind(profile_icon_id)
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn get(app: &Router, uri: &str, cookie: Option<&str>) -> TestResult<Response<Body>> {
    let mut request = Request::get(uri);
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    Ok(app.clone().oneshot(request.body(Body::empty())?).await?)
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
struct UnusedDiscordClient;

impl DiscordClient for UnusedDiscordClient {
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
        Box::pin(async { Err(DiscordClientError::InvalidAuthorizationCode) })
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
