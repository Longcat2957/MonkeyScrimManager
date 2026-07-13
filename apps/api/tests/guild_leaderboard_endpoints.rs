use std::{collections::HashMap, error::Error, sync::Arc, time::Duration};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, Response, StatusCode, header},
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
const POSTGRES_PASSWORD: &str = "monkeybot-leaderboard-password";
const DATABASE: &str = "monkeybot_leaderboard";
const VIEWER_ID: &str = "100000000000000001";
const OUTSIDER_ID: &str = "100000000000000002";
const GUILD_ID: &str = "200000000000000001";
const VIEWER_SESSION: &str = "leaderboard-viewer-session";
const OUTSIDER_SESSION: &str = "leaderboard-outsider-session";
const DISCORD_CLIENT_ID: &str = "900000000000000001";
const PUBLIC_ORIGIN: &str = "http://localhost:5173";
const OAUTH_KEY_BASE64URL: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn guild_leaderboard_projects_active_season_players_and_hides_tenants() -> TestResult {
    tokio::time::timeout(Duration::from_secs(120), run_flow())
        .await
        .map_err(|_| "guild leaderboard endpoint test exceeded 120 seconds")??;
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
    let auth_config = config.auth().cloned().expect("test auth configuration");
    let auth_service = Arc::new(AuthService::new(
        auth_config.oauth_encryption_key(),
        CookiePolicy::new(auth_config.secure_cookies(), auth_config.session_ttl()),
    )?);
    let auth_repository = Arc::new(AuthRepository::new(pool.clone()));
    seed_login(&auth_repository, VIEWER_ID, "Viewer", VIEWER_SESSION).await?;
    seed_login(&auth_repository, OUTSIDER_ID, "Outsider", OUTSIDER_SESSION).await?;
    seed_leaderboard(&pool).await?;

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
    let path = format!("/api/guilds/{GUILD_ID}/leaderboard");

    assert_eq!(
        get(&app, &path, None).await?.status(),
        StatusCode::UNAUTHORIZED
    );
    let outsider = get(&app, &path, Some(OUTSIDER_SESSION)).await?;
    assert_eq!(outsider.status(), StatusCode::NOT_FOUND);
    assert_eq!(body_json(outsider).await?["error"]["code"], "not_found");

    let response = get(&app, &path, Some(VIEWER_SESSION)).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await?;
    assert_eq!(
        body["data"]["season"],
        json!({
            "id": "01900000-0000-7000-8000-000000000010",
            "name": "시즌 1"
        })
    );
    let players = body["data"]["players"]
        .as_array()
        .ok_or("players was not an array")?;
    assert_eq!(
        players.len(),
        2,
        "zero-game, inactive, and foreign rows must be omitted"
    );
    assert_eq!(players[0]["id"], "100000000000000003");
    assert_eq!(players[0]["name"], "Longcat");
    assert_eq!(players[0]["tag"], "Longcat#KR1");
    assert_eq!(players[0]["profileIconId"], 7);
    assert_eq!(players[0]["mainRole"], "MID");
    assert_eq!(players[0]["isMe"], false);
    let composite = players[0]["compositeMmr"]
        .as_f64()
        .ok_or("compositeMmr was not a number")?;
    assert!((composite - (4600.0 / 3.0)).abs() < f64::EPSILON);
    assert_eq!(
        players[0]["stats"],
        json!({
            "MID": { "mmr": 1600.0, "games": 2, "wins": 1, "championId": "Ahri" },
            "BOTTOM": { "mmr": 1400.0, "games": 1, "wins": 1, "championId": null }
        })
    );
    assert_eq!(players[1]["id"], VIEWER_ID);
    assert_eq!(players[1]["tag"], Value::Null);
    assert_eq!(players[1]["profileIconId"], Value::Null);
    assert_eq!(players[1]["mainRole"], "TOP");
    assert_eq!(players[1]["isMe"], true);
    assert_private_fields_absent(&body);

    sqlx::query(
        "UPDATE seasons SET status = 'CLOSED', ended_at = clock_timestamp()
         WHERE guild_id = $1 AND status = 'ACTIVE'",
    )
    .bind(GUILD_ID)
    .execute(&pool)
    .await?;
    let without_season = get(&app, &path, Some(VIEWER_SESSION)).await?;
    assert_eq!(without_season.status(), StatusCode::OK);
    assert_eq!(
        body_json(without_season).await?["data"],
        json!({ "season": null, "players": [] })
    );

    pool.close().await;
    drop(server);
    Ok(())
}

async fn seed_leaderboard(pool: &PgPool) -> TestResult {
    sqlx::raw_sql(
        "INSERT INTO users (user_id, display_name, avatar_hash, created_at, updated_at, deleted_at)
         VALUES
           ('100000000000000003', 'Longcat User', NULL, clock_timestamp(), clock_timestamp(), NULL),
           ('100000000000000004', 'Zero User', NULL, clock_timestamp(), clock_timestamp(), NULL),
           ('100000000000000005', 'Inactive User', NULL, clock_timestamp(), clock_timestamp(), NULL),
           ('100000000000000006', 'Foreign User', NULL, clock_timestamp(), clock_timestamp(), NULL);

         INSERT INTO guilds (
             guild_id, discord_name, icon_hash, status, installed_by,
             installed_at, activated_at, left_at, updated_at
         ) VALUES
           ('200000000000000001', 'Guild A', NULL, 'ACTIVE', '100000000000000001',
            clock_timestamp(), clock_timestamp(), NULL, clock_timestamp()),
           ('200000000000000002', 'Guild B', NULL, 'ACTIVE', '100000000000000002',
            clock_timestamp(), clock_timestamp(), NULL, clock_timestamp());

         INSERT INTO guild_members (
             guild_id, user_id, display_name, status, main_lane,
             first_seen_at, registered_at, discord_joined_at, last_active_at, last_synced_at,
             inactivated_at
         ) VALUES
           ('200000000000000001', '100000000000000001', 'Viewer', 'ACTIVE', NULL,
            clock_timestamp(), clock_timestamp(), clock_timestamp(), clock_timestamp(), clock_timestamp(), NULL),
           ('200000000000000001', '100000000000000003', 'Longcat', 'ACTIVE', 'MID',
            clock_timestamp(), clock_timestamp(), clock_timestamp(), clock_timestamp(), clock_timestamp(), NULL),
           ('200000000000000001', '100000000000000004', 'Zero', 'ACTIVE', 'MID',
            clock_timestamp(), clock_timestamp(), clock_timestamp(), clock_timestamp(), clock_timestamp(), NULL),
           ('200000000000000001', '100000000000000005', 'Inactive', 'INACTIVE', 'MID',
            clock_timestamp(), clock_timestamp(), clock_timestamp(), clock_timestamp(), clock_timestamp(), clock_timestamp()),
           ('200000000000000002', '100000000000000002', 'Outsider', 'ACTIVE', NULL,
            clock_timestamp(), clock_timestamp(), clock_timestamp(), clock_timestamp(), clock_timestamp(), NULL),
           ('200000000000000002', '100000000000000006', 'Foreign', 'ACTIVE', 'MID',
            clock_timestamp(), clock_timestamp(), clock_timestamp(), clock_timestamp(), clock_timestamp(), NULL);

         INSERT INTO seasons (guild_id, id, name, status, started_at, ended_at, created_by, created_at)
         VALUES
           ('200000000000000001', '01900000-0000-7000-8000-000000000010', '시즌 1', 'ACTIVE',
            clock_timestamp() - interval '30 days', NULL, '100000000000000001', clock_timestamp()),
           ('200000000000000002', '01900000-0000-7000-8000-000000000020', '다른 시즌', 'ACTIVE',
            clock_timestamp() - interval '30 days', NULL, '100000000000000002', clock_timestamp());

	         INSERT INTO user_privacy_settings (
	             user_id, public_profile_enabled, show_riot_ids, allow_external_record_search, updated_at
	         ) VALUES ('100000000000000003', false, true, false, clock_timestamp());

	         UPDATE user_privacy_settings
	         SET show_riot_ids = false, updated_at = clock_timestamp()
	         WHERE user_id = '100000000000000001';

	         INSERT INTO riot_accounts (
	             puuid, user_id, platform, routing_region, game_name, tag_line,
             normalized_game_name, normalized_tag_line, profile_icon_id, is_main,
             main_position, main_position_updated_at, last_synced_at, created_at, updated_at
	         ) VALUES
	           ('leaderboard-main-puuid', '100000000000000003', 'KR', 'ASIA', 'Longcat', 'KR1',
	            'longcat', 'kr1', 7, true, NULL, NULL, clock_timestamp(), clock_timestamp(), clock_timestamp()),
	           ('leaderboard-alt-puuid', '100000000000000003', 'KR', 'ASIA', 'LongAlt', 'KR2',
	            'longalt', 'kr2', 8, false, NULL, NULL, clock_timestamp(), clock_timestamp(), clock_timestamp()),
	           ('leaderboard-private-puuid', '100000000000000001', 'KR', 'ASIA', 'PrivateViewer', 'KR1',
	            'privateviewer', 'kr1', 9, true, NULL, NULL, clock_timestamp(), clock_timestamp(), clock_timestamp());

         INSERT INTO recruitments (
             guild_id, id, season_id, display_number, title, target_count, status, revision,
             created_by, created_at, updated_by, updated_at, closed_at, converted_at
         ) VALUES (
             '200000000000000001', '01900000-0000-7000-8000-000000000011',
             '01900000-0000-7000-8000-000000000010', 1, '내전', 10, 'CONVERTED', 1,
             '100000000000000001', clock_timestamp(), '100000000000000001', clock_timestamp(),
             clock_timestamp(), clock_timestamp()
         );

         INSERT INTO series (
             guild_id, id, season_id, source_recruitment_id, display_number, title, best_of,
             status, winning_team, terminal_reason, revision, started_at, ended_at, created_by,
             updated_by, updated_at, deleted_at
         ) VALUES (
             '200000000000000001', '01900000-0000-7000-8000-000000000012',
             '01900000-0000-7000-8000-000000000010',
             '01900000-0000-7000-8000-000000000011', 1, '내전', 1,
             'COMPLETED', 'TEAM_1', 'SCORE_REACHED', 1,
             clock_timestamp() - interval '1 day', clock_timestamp() - interval '23 hours',
             '100000000000000001', '100000000000000001', clock_timestamp(), NULL
         );

         INSERT INTO game_inputs (
             guild_id, id, series_id, auction_match_id, set_number, side_preset,
             winning_side, patch_version, status, updated_by, updated_at
         ) VALUES (
             '200000000000000001', '01900000-0000-7000-8000-000000000013',
             '01900000-0000-7000-8000-000000000012', NULL, 1, 'DEFAULT',
             'BLUE', '26.13', 'SUBMITTED', '100000000000000001', clock_timestamp()
         );

         INSERT INTO games (
             guild_id, id, season_id, series_id, auction_match_id, source_input_id,
             game_number, side_preset, winning_side, duration_seconds, patch_version,
             riot_match_id, status, mmr_status, played_at, confirmed_by,
             confirmed_by_type, confirmed_at, voided_by, voided_by_type,
             voided_at, void_reason
         ) VALUES (
             '200000000000000001', '01900000-0000-7000-8000-000000000014',
             '01900000-0000-7000-8000-000000000010',
             '01900000-0000-7000-8000-000000000012', NULL,
             '01900000-0000-7000-8000-000000000013', 1, 'DEFAULT', 'BLUE', 1800, '26.13',
             NULL, 'CONFIRMED', 'LEGACY_NOT_APPLIED', clock_timestamp() - interval '1 day',
             '100000000000000001', 'USER', clock_timestamp(), NULL, NULL, NULL, NULL
         );

         INSERT INTO game_participants (guild_id, game_id, user_id, team, lane, champion_id)
         VALUES (
             '200000000000000001', '01900000-0000-7000-8000-000000000014',
             '100000000000000003', 'TEAM_1', 'MID', 'Ahri'
         );

         INSERT INTO user_lane_mmr (
             guild_id, season_id, user_id, lane, mmr, games_played, wins, revision, updated_at
         ) VALUES
           ('200000000000000001', '01900000-0000-7000-8000-000000000010',
            '100000000000000003', 'MID', 1600, 2, 1, 2, clock_timestamp()),
           ('200000000000000001', '01900000-0000-7000-8000-000000000010',
            '100000000000000003', 'BOTTOM', 1400, 1, 1, 1, clock_timestamp()),
           ('200000000000000001', '01900000-0000-7000-8000-000000000010',
            '100000000000000001', 'TOP', 1510, 3, 2, 3, clock_timestamp()),
           ('200000000000000001', '01900000-0000-7000-8000-000000000010',
            '100000000000000004', 'MID', 2500, 0, 0, 0, clock_timestamp()),
           ('200000000000000001', '01900000-0000-7000-8000-000000000010',
            '100000000000000005', 'MID', 2600, 10, 10, 10, clock_timestamp()),
           ('200000000000000002', '01900000-0000-7000-8000-000000000020',
            '100000000000000006', 'MID', 3000, 10, 10, 10, clock_timestamp());",
    )
    .execute(pool)
    .await?;
    Ok(())
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

async fn get(app: &Router, uri: &str, session: Option<&str>) -> TestResult<Response<Body>> {
    let mut request = Request::get(uri);
    if let Some(session) = session {
        request = request.header(header::COOKIE, format!("monkeybot_session={session}"));
    }
    Ok(app.clone().oneshot(request.body(Body::empty())?).await?)
}

async fn body_json(response: Response<Body>) -> TestResult<Value> {
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn assert_private_fields_absent(body: &Value) {
    let serialized = body.to_string();
    for private in [
        "leaderboard-main-puuid",
        "leaderboard-alt-puuid",
        "routingRegion",
        "userId",
        "discordId",
    ] {
        assert!(
            !serialized.contains(private),
            "leaked private field {private}"
        );
    }
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
        let port = container.get_host_port_ipv4(5432).await?;
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
