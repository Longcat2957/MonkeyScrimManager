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
    repository::auth::{AuthRepository, AuthRepositoryError, NewLogin},
    state::{AppState, AuthRuntime},
};
use reqwest::Url;
use secrecy::{ExposeSecret, SecretString};
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
const POSTGRES_PASSWORD: &str = "monkeybot-auth-endpoint-password";
const DATABASE: &str = "monkeybot_auth_endpoints";

const USER_ID: &str = "100000000000000001";
const MEMBER_ID: &str = "100000000000000002";
const OUTSIDER_ID: &str = "100000000000000003";
const COLLIDING_DISCORD_ID: &str = "100000000000000099";
const EXISTING_IDENTITY_OWNER_ID: &str = "100000000000000098";
const GUILD_A: &str = "200000000000000001";
const GUILD_B: &str = "200000000000000002";
const OPERATOR_ROLE_A: &str = "300000000000000001";
const OPERATOR_ROLE_B: &str = "300000000000000002";
const DISCORD_CLIENT_ID: &str = "900000000000000001";
const PUBLIC_ORIGIN: &str = "http://localhost:5173";
const OAUTH_KEY_BASE64URL: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auth_and_guild_context_vertical_flow() -> TestResult {
    tokio::time::timeout(
        Duration::from_secs(120),
        run_auth_and_guild_context_vertical_flow(),
    )
    .await
    .map_err(|_| "auth endpoint integration test exceeded 120 seconds")??;
    Ok(())
}

async fn run_auth_and_guild_context_vertical_flow() -> TestResult {
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
    let state = AppState::with_postgres(
        config,
        pool.clone(),
        Arc::new(PostgresReadinessProbe::new(
            pool.clone(),
            Duration::from_secs(1),
        )),
    )
    .with_auth(AuthRuntime::new(
        auth_service.clone(),
        Arc::new(AuthRepository::new(pool.clone())),
        Arc::new(FakeDiscordClient),
    ));
    let app = build_router(state);

    sqlx::query(
        "INSERT INTO oauth_login_attempts (
            id, state_hash, pkce_verifier_ciphertext, redirect_path,
            created_at, expires_at, consumed_at
         ) VALUES (
            $1, $2, $3, '/',
            CURRENT_TIMESTAMP - INTERVAL '10 minutes',
            CURRENT_TIMESTAMP - INTERVAL '5 minutes', NULL
         )",
    )
    .bind(Uuid::now_v7())
    .bind(vec![9_u8; 32])
    .bind(vec![1_u8; 32])
    .execute(&pool)
    .await?;

    let start = app
        .clone()
        .oneshot(
            Request::get(format!("/oauth/discord/start?next=%2Fg%2F{GUILD_A}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(start.status(), StatusCode::FOUND);
    let authorization_url = Url::parse(start.headers()[header::LOCATION].to_str()?)?;
    assert_eq!(authorization_url.host_str(), Some("discord.example"));
    let authorization_query = authorization_url
        .query_pairs()
        .into_owned()
        .collect::<HashMap<_, _>>();
    let state_secret = authorization_query
        .get("state")
        .cloned()
        .ok_or("authorization URL did not contain state")?;
    let challenge = authorization_query
        .get("code_challenge")
        .ok_or("authorization URL did not contain the PKCE challenge")?;
    assert_eq!(challenge.len(), 43);
    assert_eq!(
        authorization_query.get("code_challenge_method"),
        Some(&"S256".to_owned())
    );

    let start_cookies = set_cookie_values(&start)?;
    assert_eq!(start_cookies.len(), 1);
    assert_eq!(
        cookie_value(&start_cookies, "monkeybot_oauth_state").as_deref(),
        Some(state_secret.as_str())
    );
    assert!(start_cookies[0].contains("Path=/oauth/callback"));
    assert!(start_cookies[0].contains("HttpOnly"));

    let attempt = sqlx::query_as::<_, (Uuid, Vec<u8>, Vec<u8>, String, Option<OffsetDateTime>)>(
        "SELECT id, state_hash, pkce_verifier_ciphertext, redirect_path, consumed_at
         FROM oauth_login_attempts",
    )
    .fetch_one(&pool)
    .await?;
    let attempt_count = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM oauth_login_attempts")
        .fetch_one(&pool)
        .await?;
    assert_eq!(attempt_count, 1, "expired OAuth attempts were not removed");
    assert_eq!(attempt.1, AuthService::hash_secret(&state_secret));
    assert_ne!(attempt.1, state_secret.as_bytes());
    assert!(
        attempt.2.len() > 32,
        "PKCE verifier was not stored as ciphertext"
    );
    assert_eq!(
        attempt.2.first(),
        Some(&1),
        "ciphertext envelope version missing"
    );
    assert_eq!(attempt.3, format!("/g/{GUILD_A}"));
    assert!(attempt.4.is_none());
    let decrypted_verifier = auth_service.decrypt_pkce(attempt.0, &attempt.2)?;
    assert_eq!(decrypted_verifier.expose_secret().len(), 43);

    let callback = app
        .clone()
        .oneshot(
            Request::get(format!(
                "/oauth/callback?code=fake-code&state={state_secret}"
            ))
            .header(
                header::COOKIE,
                format!("monkeybot_oauth_state={state_secret}"),
            )
            .body(Body::empty())?,
        )
        .await?;
    assert_eq!(callback.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        callback.headers()[header::LOCATION],
        format!("/g/{GUILD_A}")
    );
    let callback_cookies = set_cookie_values(&callback)?;
    assert_eq!(callback_cookies.len(), 3);
    let mut session_token = cookie_value(&callback_cookies, "monkeybot_session")
        .ok_or("callback did not set the session cookie")?;
    let mut csrf_token = cookie_value(&callback_cookies, "monkeybot_csrf")
        .ok_or("callback did not set the CSRF cookie")?;
    let cleared_state = callback_cookies
        .iter()
        .find(|cookie| cookie.starts_with("monkeybot_oauth_state="))
        .ok_or("callback did not clear the OAuth state cookie")?;
    assert!(cleared_state.contains("Path=/oauth/callback"));
    assert!(cleared_state.contains("Max-Age=0"));

    let user = sqlx::query_as::<_, (String, String, Option<String>)>(
        "SELECT identity.discord_id, app_user.display_name, app_user.avatar_hash
         FROM users AS app_user
         JOIN user_discord_identities AS identity ON identity.user_id = app_user.user_id
         WHERE identity.discord_id = $1",
    )
    .bind(USER_ID)
    .fetch_one(&pool)
    .await?;
    assert_eq!(
        user,
        (
            USER_ID.to_owned(),
            "Longcat".to_owned(),
            Some("a_0123abcdef".to_owned())
        )
    );
    let privacy = sqlx::query_as::<_, (bool, bool, bool)>(
        "SELECT public_profile_enabled, show_riot_ids, allow_external_record_search
         FROM user_privacy_settings WHERE user_id = $1",
    )
    .bind(USER_ID)
    .fetch_one(&pool)
    .await?;
    assert_eq!(privacy, (true, true, false));
    let stored_session = sqlx::query_as::<_, (Vec<u8>, Vec<u8>, bool)>(
        "SELECT token_hash, csrf_secret_hash,
                revoked_at IS NULL AND expires_at > CURRENT_TIMESTAMP
         FROM user_sessions WHERE user_id = $1",
    )
    .bind(USER_ID)
    .fetch_one(&pool)
    .await?;
    assert_eq!(stored_session.0, AuthService::hash_secret(&session_token));
    assert_eq!(stored_session.1, AuthService::hash_secret(&csrf_token));
    assert_ne!(stored_session.0, session_token.as_bytes());
    assert_ne!(stored_session.1, csrf_token.as_bytes());
    assert!(stored_session.2);
    let consumed = sqlx::query_scalar::<_, bool>(
        "SELECT consumed_at IS NOT NULL FROM oauth_login_attempts WHERE id = $1",
    )
    .bind(attempt.0)
    .fetch_one(&pool)
    .await?;
    assert!(consumed);

    let mut browser_cookie =
        format!("monkeybot_session={session_token}; monkeybot_csrf={csrf_token}");
    let me = authenticated_get(&app, "/api/me", &browser_cookie).await?;
    assert_eq!(me.status(), StatusCode::OK);
    let me = body_json(me).await?;
    assert_eq!(me["data"]["discordId"], USER_ID);
    assert_eq!(me["data"]["displayName"], "Longcat");
    assert_eq!(
        me["data"]["avatarUrl"],
        format!("https://cdn.discordapp.com/avatars/{USER_ID}/a_0123abcdef.webp?size=128")
    );
    assert!(me["data"]["defaultGuildId"].is_null());
    assert_eq!(me["data"]["serviceAdmin"], true);

    let previous_session_id =
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM user_sessions WHERE token_hash = $1")
            .bind(AuthService::hash_secret(&session_token))
            .fetch_one(&pool)
            .await?;
    let rotation_start = app
        .clone()
        .oneshot(Request::get("/oauth/discord/start?next=%2Fme").body(Body::empty())?)
        .await?;
    assert_eq!(rotation_start.status(), StatusCode::FOUND);
    let rotation_url = Url::parse(rotation_start.headers()[header::LOCATION].to_str()?)?;
    let rotation_state = rotation_url
        .query_pairs()
        .find_map(|(key, value)| (key == "state").then(|| value.into_owned()))
        .ok_or("rotation OAuth URL did not contain state")?;
    let rotation_callback = app
        .clone()
        .oneshot(
            Request::get(format!(
                "/oauth/callback?code=fake-code&state={rotation_state}"
            ))
            .header(
                header::COOKIE,
                format!("{browser_cookie}; monkeybot_oauth_state={rotation_state}"),
            )
            .body(Body::empty())?,
        )
        .await?;
    assert_eq!(rotation_callback.status(), StatusCode::SEE_OTHER);
    let rotation_cookies = set_cookie_values(&rotation_callback)?;
    let rotated_session_token = cookie_value(&rotation_cookies, "monkeybot_session")
        .ok_or("rotation did not set a session cookie")?;
    let rotated_csrf_token = cookie_value(&rotation_cookies, "monkeybot_csrf")
        .ok_or("rotation did not set a CSRF cookie")?;
    let rotated_from = sqlx::query_scalar::<_, Option<Uuid>>(
        "SELECT rotated_from_id FROM user_sessions WHERE token_hash = $1",
    )
    .bind(AuthService::hash_secret(&rotated_session_token))
    .fetch_one(&pool)
    .await?;
    assert_eq!(rotated_from, Some(previous_session_id));
    let previous_revoked = sqlx::query_scalar::<_, bool>(
        "SELECT revoked_at IS NOT NULL FROM user_sessions WHERE id = $1",
    )
    .bind(previous_session_id)
    .fetch_one(&pool)
    .await?;
    assert!(previous_revoked);
    let previous_cookie = browser_cookie.clone();
    assert_eq!(
        authenticated_get(&app, "/api/me", &previous_cookie)
            .await?
            .status(),
        StatusCode::UNAUTHORIZED
    );
    session_token = rotated_session_token;
    csrf_token = rotated_csrf_token;
    browser_cookie = format!("monkeybot_session={session_token}; monkeybot_csrf={csrf_token}");

    let (season_a, _season_b) = seed_guild_context(&pool).await?;

    let member_session = auth_service.new_session()?;
    AuthRepository::new(pool.clone())
        .complete_login(&NewLogin {
            discord_user_id: DiscordUserId::parse(MEMBER_ID)?,
            display_name: "Second Member".to_owned(),
            avatar_hash: None,
            previous_session_token_hash: None,
            session_id: Uuid::now_v7(),
            token_hash: member_session.token_hash,
            csrf_secret_hash: member_session.csrf_secret_hash,
            expires_at: OffsetDateTime::now_utc() + time::Duration::hours(1),
        })
        .await?;
    let member_cookie = format!("monkeybot_session={}", member_session.token.expose_secret());
    let member_me = body_json(authenticated_get(&app, "/api/me", &member_cookie).await?).await?;
    assert_eq!(member_me["data"]["serviceAdmin"], false);
    let member_guilds =
        body_json(authenticated_get(&app, "/api/me/guilds", &member_cookie).await?).await?;
    assert_eq!(
        member_guilds["data"]["guilds"][0]["currentUserRole"],
        "MEMBER"
    );

    let guilds = authenticated_get(&app, "/api/me/guilds", &browser_cookie).await?;
    let guilds_status = guilds.status();
    let guilds = body_json(guilds).await?;
    assert_eq!(guilds_status, StatusCode::OK, "{guilds}");
    assert_eq!(guilds["data"]["guilds"].as_array().map(Vec::len), Some(1));
    assert_eq!(guilds["data"]["guilds"][0]["id"], GUILD_A);
    assert_eq!(guilds["data"]["guilds"][0]["displayName"], "MSM Seoul");
    assert_eq!(guilds["data"]["guilds"][0]["memberCount"], 2);
    assert_eq!(guilds["data"]["guilds"][0]["currentUserRole"], "OPERATOR");

    let guild = authenticated_get(&app, &format!("/api/guilds/{GUILD_A}"), &browser_cookie).await?;
    assert_eq!(guild.status(), StatusCode::OK);
    let guild = body_json(guild).await?;
    assert_eq!(guild["data"]["id"], GUILD_A);
    assert_eq!(guild["data"]["displayName"], "MSM Seoul");
    assert_eq!(guild["data"]["memberCount"], 2);
    assert_eq!(guild["data"]["currentUserRole"], "OPERATOR");
    assert_eq!(guild["data"]["isExternallyPublic"], true);
    assert_eq!(guild["data"]["currentSeason"]["id"], season_a.to_string());
    assert_eq!(guild["data"]["currentSeason"]["name"], "2026 Summer");
    assert_eq!(
        guild["data"]["currentSeason"]["startedAt"],
        "2026-07-01T00:00:00Z"
    );

    let hidden =
        authenticated_get(&app, &format!("/api/guilds/{GUILD_B}"), &browser_cookie).await?;
    assert_eq!(hidden.status(), StatusCode::NOT_FOUND);
    assert_eq!(body_json(hidden).await?["error"]["code"], "not_found");

    let missing_csrf = app
        .clone()
        .oneshot(
            Request::put("/api/me/default-guild")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::ORIGIN, PUBLIC_ORIGIN)
                .header(header::COOKIE, &browser_cookie)
                .body(Body::from(json!({ "guildId": GUILD_A }).to_string()))?,
        )
        .await?;
    assert_eq!(missing_csrf.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        body_json(missing_csrf).await?["error"]["code"],
        "csrf_validation_failed"
    );

    let set_default = app
        .clone()
        .oneshot(
            Request::put("/api/me/default-guild")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::ORIGIN, PUBLIC_ORIGIN)
                .header(header::COOKIE, &browser_cookie)
                .header("x-csrf-token", &csrf_token)
                .body(Body::from(json!({ "guildId": GUILD_A }).to_string()))?,
        )
        .await?;
    assert_eq!(set_default.status(), StatusCode::OK);
    assert_eq!(
        body_json(set_default).await?["data"]["defaultGuildId"],
        GUILD_A
    );
    let me = authenticated_get(&app, "/api/me", &browser_cookie).await?;
    assert_eq!(body_json(me).await?["data"]["defaultGuildId"], GUILD_A);

    assert_expired_session_is_rejected(&app, &pool).await?;

    let logout = app
        .clone()
        .oneshot(
            Request::post("/api/logout")
                .header(header::ORIGIN, PUBLIC_ORIGIN)
                .header(header::COOKIE, &browser_cookie)
                .header("x-csrf-token", &csrf_token)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(logout.status(), StatusCode::OK);
    let logout_cookies = set_cookie_values(&logout)?;
    assert_eq!(logout_cookies.len(), 2);
    for name in ["monkeybot_session", "monkeybot_csrf"] {
        let cookie = logout_cookies
            .iter()
            .find(|cookie| cookie.starts_with(&format!("{name}=")))
            .ok_or("logout did not clear an authentication cookie")?;
        assert!(cookie.contains("Path=/"));
        assert!(cookie.contains("Max-Age=0"));
    }
    assert_eq!(body_json(logout).await?["data"]["loggedOut"], true);
    let revoked = sqlx::query_scalar::<_, bool>(
        "SELECT revoked_at IS NOT NULL FROM user_sessions WHERE token_hash = $1",
    )
    .bind(AuthService::hash_secret(&session_token))
    .fetch_one(&pool)
    .await?;
    assert!(revoked);

    let after_logout = authenticated_get(&app, "/api/me", &browser_cookie).await?;
    assert_eq!(after_logout.status(), StatusCode::UNAUTHORIZED);

    let replay = app
        .clone()
        .oneshot(
            Request::get(format!(
                "/oauth/callback?code=fake-code&state={state_secret}"
            ))
            .header(
                header::COOKIE,
                format!("monkeybot_oauth_state={state_secret}"),
            )
            .body(Body::empty())?,
        )
        .await?;
    assert_eq!(replay.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        body_json(replay).await?["error"]["code"],
        "invalid_oauth_state"
    );

    sqlx::query(
        "INSERT INTO users (
             user_id, identity_kind, display_name, created_at, updated_at
         ) VALUES ($1, 'DISCORD', 'Protected Identity Owner', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
    )
    .bind(EXISTING_IDENTITY_OWNER_ID)
    .execute(&pool)
    .await?;
    sqlx::query(
        "INSERT INTO user_discord_identities (
             user_id, discord_id, created_at, updated_at
         ) VALUES ($1, $2, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
    )
    .bind(EXISTING_IDENTITY_OWNER_ID)
    .bind(COLLIDING_DISCORD_ID)
    .execute(&pool)
    .await?;
    let collision_session = auth_service.new_session()?;
    let collision = AuthRepository::new(pool.clone())
        .complete_login(&NewLogin {
            discord_user_id: DiscordUserId::parse(COLLIDING_DISCORD_ID)?,
            display_name: "Discord Collision".to_owned(),
            avatar_hash: None,
            previous_session_token_hash: None,
            session_id: Uuid::now_v7(),
            token_hash: collision_session.token_hash,
            csrf_secret_hash: collision_session.csrf_secret_hash,
            expires_at: OffsetDateTime::now_utc() + time::Duration::hours(1),
        })
        .await;
    assert!(matches!(
        collision,
        Err(AuthRepositoryError::IdentityConflict)
    ));
    let preserved = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT app_user.identity_kind,
                app_user.display_name,
                (SELECT count(*) FROM user_discord_identities WHERE discord_id = $1)
         FROM users AS app_user
         JOIN user_discord_identities AS identity ON identity.user_id = app_user.user_id
         WHERE identity.discord_id = $1",
    )
    .bind(COLLIDING_DISCORD_ID)
    .fetch_one(&pool)
    .await?;
    assert_eq!(
        preserved,
        (
            "DISCORD".to_owned(),
            "Protected Identity Owner".to_owned(),
            1
        )
    );
    let colliding_user_created =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM users WHERE user_id = $1)")
            .bind(COLLIDING_DISCORD_ID)
            .fetch_one(&pool)
            .await?;
    assert!(
        !colliding_user_created,
        "failed login must roll back the new user"
    );

    pool.close().await;
    drop(server);
    Ok(())
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
        ("MONKEYBOT_SERVICE_ADMIN_IDS".to_owned(), USER_ID.to_owned()),
    ]);
    Ok(Config::from_map(&values)?)
}

async fn seed_guild_context(pool: &PgPool) -> TestResult<(Uuid, Uuid)> {
    sqlx::query(
        "INSERT INTO users (user_id, display_name, created_at, updated_at)
         VALUES
           ($1, 'Second Member', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
           ($2, 'Outsider', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
    )
    .bind(MEMBER_ID)
    .bind(OUTSIDER_ID)
    .execute(pool)
    .await?;

    sqlx::query(
        "INSERT INTO guilds (
            guild_id, discord_name, icon_hash, status, installed_by,
            installed_at, activated_at, updated_at
         ) VALUES
           ($1, 'MSM Discord', 'a_abcdef1234', 'ACTIVE', $2,
            CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
           ($3, 'Hidden Discord', NULL, 'ACTIVE', $4,
            CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
    )
    .bind(GUILD_A)
    .bind(USER_ID)
    .bind(GUILD_B)
    .bind(OUTSIDER_ID)
    .execute(pool)
    .await?;

    sqlx::query(
        "INSERT INTO guild_members (
            guild_id, user_id, display_name, status, first_seen_at, registered_at
         ) VALUES
           ($1, $2, 'Longcat', 'ACTIVE', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
           ($1, $3, 'Second Member', 'ACTIVE', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
           ($4, $5, 'Outsider', 'ACTIVE', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
    )
    .bind(GUILD_A)
    .bind(USER_ID)
    .bind(MEMBER_ID)
    .bind(GUILD_B)
    .bind(OUTSIDER_ID)
    .execute(pool)
    .await?;

    sqlx::query(
        "INSERT INTO guild_config (
            guild_id, display_name, operator_role_id, is_externally_public,
            revision, updated_by, updated_at
         ) VALUES
           ($1, 'MSM Seoul', $2, true, 0, $3, CURRENT_TIMESTAMP),
           ($4, 'Hidden Guild', $5, false, 0, $6, CURRENT_TIMESTAMP)",
    )
    .bind(GUILD_A)
    .bind(OPERATOR_ROLE_A)
    .bind(USER_ID)
    .bind(GUILD_B)
    .bind(OPERATOR_ROLE_B)
    .bind(OUTSIDER_ID)
    .execute(pool)
    .await?;

    sqlx::query(
        "INSERT INTO guild_member_discord_roles (guild_id, user_id, role_id, last_synced_at)
         VALUES ($1, $2, $3, CURRENT_TIMESTAMP)",
    )
    .bind(GUILD_A)
    .bind(USER_ID)
    .bind(OPERATOR_ROLE_A)
    .execute(pool)
    .await?;

    let season_a = Uuid::now_v7();
    let season_b = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO seasons (
            guild_id, id, name, status, started_at, created_by, created_at
         ) VALUES
           ($1, $2, '2026 Summer', 'ACTIVE',
            '2026-07-01T00:00:00Z'::timestamptz, $3, CURRENT_TIMESTAMP),
           ($4, $5, 'Hidden Season', 'ACTIVE',
            '2026-07-02T00:00:00Z'::timestamptz, $6, CURRENT_TIMESTAMP)",
    )
    .bind(GUILD_A)
    .bind(season_a)
    .bind(USER_ID)
    .bind(GUILD_B)
    .bind(season_b)
    .bind(OUTSIDER_ID)
    .execute(pool)
    .await?;

    Ok((season_a, season_b))
}

async fn assert_expired_session_is_rejected(app: &Router, pool: &PgPool) -> TestResult {
    let expired_token = "expired-session-token";
    sqlx::query(
        "INSERT INTO user_sessions (
            id, user_id, token_hash, csrf_secret_hash,
            created_at, last_seen_at, expires_at, revoked_at
         ) VALUES (
            $1, $2, $3, $4,
            CURRENT_TIMESTAMP - INTERVAL '2 hours',
            CURRENT_TIMESTAMP - INTERVAL '2 hours',
            CURRENT_TIMESTAMP - INTERVAL '1 hour', NULL
         )",
    )
    .bind(Uuid::now_v7())
    .bind(USER_ID)
    .bind(AuthService::hash_secret(expired_token))
    .bind(AuthService::hash_secret("expired-csrf-token"))
    .execute(pool)
    .await?;

    let response = authenticated_get(
        app,
        "/api/me",
        &format!("monkeybot_session={expired_token}"),
    )
    .await?;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    Ok(())
}

async fn authenticated_get(app: &Router, uri: &str, cookie: &str) -> TestResult<Response<Body>> {
    Ok(app
        .clone()
        .oneshot(
            Request::get(uri)
                .header(header::COOKIE, cookie)
                .body(Body::empty())?,
        )
        .await?)
}

fn set_cookie_values(response: &Response<Body>) -> TestResult<Vec<String>> {
    response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|value| Ok(value.to_str()?.to_owned()))
        .collect()
}

fn cookie_value(set_cookies: &[String], name: &str) -> Option<String> {
    let prefix = format!("{name}=");
    set_cookies.iter().find_map(|cookie| {
        cookie
            .split(';')
            .next()
            .and_then(|pair| pair.strip_prefix(&prefix))
            .map(ToOwned::to_owned)
    })
}

async fn body_json(response: Response<Body>) -> TestResult<Value> {
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

#[derive(Debug)]
struct FakeDiscordClient;

impl DiscordClient for FakeDiscordClient {
    fn authorization_url(
        &self,
        state: &str,
        pkce_challenge: &str,
    ) -> Result<Url, DiscordClientError> {
        let mut url = Url::parse("https://discord.example/oauth2/authorize")
            .map_err(|_| DiscordClientError::InvalidConfiguration)?;
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", DISCORD_CLIENT_ID)
            .append_pair("state", state)
            .append_pair("code_challenge", pkce_challenge)
            .append_pair("code_challenge_method", "S256");
        Ok(url)
    }

    fn authenticate<'a>(
        &'a self,
        code: &'a str,
        pkce_verifier: &'a SecretString,
    ) -> BoxFuture<'a, Result<DiscordUser, DiscordClientError>> {
        Box::pin(async move {
            if code != "fake-code" || pkce_verifier.expose_secret().len() != 43 {
                return Err(DiscordClientError::InvalidAuthorizationCode);
            }
            Ok(DiscordUser {
                id: DiscordUserId::parse(USER_ID).map_err(|_| DiscordClientError::InvalidUser)?,
                display_name: "Longcat".to_owned(),
                avatar_hash: Some("a_0123abcdef".to_owned()),
            })
        })
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
