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
    domain::{DiscordUserId, UserId},
    infrastructure::postgres::{MIGRATOR, PostgresReadinessProbe},
    integrations::discord::{DiscordClient, DiscordClientError, DiscordUser},
    repository::{
        auth::{AuthRepository, NewLogin},
        riot::{self, CanonicalRiotAccount, RiotRepositoryError},
        search_suggestions,
    },
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
const POSTGRES_PASSWORD: &str = "monkeybot-dummy-user-endpoint-password";
const DATABASE: &str = "monkeybot_dummy_user_endpoints";
const ADMIN_ID: &str = "100000000000000001";
const GUILD_ID: &str = "200000000000000001";
const MISSING_GUILD_ID: &str = "200000000000000099";
const OPERATOR_ROLE_ID: &str = "300000000000000001";
const DISCORD_CLIENT_ID: &str = "900000000000000001";
const PUBLIC_ORIGIN: &str = "http://localhost:5173";
const OAUTH_KEY_BASE64URL: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const SESSION_TOKEN: &str = "dummy-user-admin-session";
const CSRF_TOKEN: &str = "dummy-user-admin-csrf";
const MEMBER_ID: &str = "100000000000000002";
const MEMBER_SESSION_TOKEN: &str = "dummy-user-member-session";
const MEMBER_CSRF_TOKEN: &str = "dummy-user-member-csrf";

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn persistent_dummy_users_are_atomic_idempotent_and_isolated() -> TestResult {
    tokio::time::timeout(Duration::from_secs(120), run_flow())
        .await
        .map_err(|_| "dummy user endpoint integration test exceeded 120 seconds")??;
    Ok(())
}

async fn run_flow() -> TestResult {
    let server = TestPostgres::start().await?;
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&server.database_url())
        .await?;
    MIGRATOR.run(&pool).await?;

    let config = test_config(server.database_url(), true)?;
    let auth_config = config.auth().cloned().expect("test auth config");
    let auth_service = Arc::new(AuthService::new(
        auth_config.oauth_encryption_key(),
        CookiePolicy::new(auth_config.secure_cookies(), auth_config.session_ttl()),
    )?);
    let auth_repository = Arc::new(AuthRepository::new(pool.clone()));
    create_login(
        &auth_repository,
        ADMIN_ID,
        "Service Admin",
        SESSION_TOKEN,
        CSRF_TOKEN,
    )
    .await?;
    create_login(
        &auth_repository,
        MEMBER_ID,
        "Ordinary Member",
        MEMBER_SESSION_TOKEN,
        MEMBER_CSRF_TOKEN,
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

    let non_admin = send_with_auth(
        &app,
        Method::GET,
        "/api/dev/dummy-users",
        None,
        None,
        MEMBER_SESSION_TOKEN,
        MEMBER_CSRF_TOKEN,
        None,
    )
    .await?;
    assert_eq!(non_admin.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        body_json(non_admin).await?["error"]["code"],
        "service_admin_required"
    );

    for csrf_header in [None, Some("wrong-dummy-user-csrf")] {
        let rejected = send_with_auth(
            &app,
            Method::POST,
            "/api/dev/dummy-users",
            Some(dummy_body("csrf-rejected", "CSRF Rejected", None)),
            Some("csrf-rejected"),
            SESSION_TOKEN,
            CSRF_TOKEN,
            csrf_header,
        )
        .await?;
        assert_eq!(rejected.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            body_json(rejected).await?["error"]["code"],
            "csrf_validation_failed"
        );
    }
    assert_eq!(stable_key_count(&pool, "csrf-rejected").await?, 0);

    let create_body = dummy_body("top-01", "Top 01", Some(GUILD_ID));
    let created = mutation(
        &app,
        Method::POST,
        "/api/dev/dummy-users",
        create_body.clone(),
        "create-top-01",
    )
    .await?;
    assert_eq!(created.status(), StatusCode::CREATED);
    let created = body_json(created).await?;
    let user_id = created["data"]["userId"]
        .as_str()
        .ok_or("created dummy user ID missing")?
        .to_owned();
    assert!(user_id.starts_with("dmy_"));
    assert_eq!(created["data"]["identityKind"], "DUMMY");
    assert_eq!(created["data"]["privacy"]["publicProfileEnabled"], false);
    assert_eq!(created["data"]["guildMemberships"][0]["mainLane"], "TOP");
    assert_eq!(
        created["data"]["guildMemberships"][0]["laneMmr"]["top"],
        1610.0
    );
    assert_eq!(created["data"]["riotAccount"]["gameName"], "Dummy Top 01");

    let real_riot_link = riot::link_account(
        &pool,
        &UserId::parse(&user_id)?,
        &CanonicalRiotAccount {
            puuid: "must-not-link-to-dummy".to_owned(),
            platform: "KR".to_owned(),
            routing_region: "ASIA".to_owned(),
            game_name: "Real Riot Account".to_owned(),
            tag_line: "KR1".to_owned(),
            profile_icon_id: None,
            last_synced_at: OffsetDateTime::now_utc(),
        },
    )
    .await
    .expect_err("the real Riot repository must reject a DUMMY owner");
    assert!(matches!(real_riot_link, RiotRepositoryError::UserNotFound));
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM riot_accounts WHERE user_id = $1 AND origin = 'RIOT'"
        )
        .bind(&user_id)
        .fetch_one(&pool)
        .await?,
        0
    );

    let replay = mutation(
        &app,
        Method::POST,
        "/api/dev/dummy-users",
        create_body.clone(),
        "create-top-01",
    )
    .await?;
    assert_eq!(replay.status(), StatusCode::CREATED);
    assert_eq!(body_json(replay).await?["data"], created["data"]);
    assert_eq!(dummy_count(&pool).await?, 1);

    let conflict = mutation(
        &app,
        Method::POST,
        "/api/dev/dummy-users",
        dummy_body("top-02", "Top 02", Some(GUILD_ID)),
        "create-top-01",
    )
    .await?;
    assert_eq!(conflict.status(), StatusCode::CONFLICT);
    assert_eq!(
        body_json(conflict).await?["error"]["code"],
        "idempotency_conflict"
    );

    let update_path = format!("/api/dev/dummy-users/{user_id}");
    let updated = mutation(
        &app,
        Method::PATCH,
        &update_path,
        json!({"expectedRevision":0,"displayName":"Top Renamed"}),
        "update-top-01",
    )
    .await?;
    let updated = body_json(updated).await?;
    assert_eq!(updated["data"]["revision"], 1);
    assert_eq!(updated["data"]["displayName"], "Top Renamed");

    let archive_path = format!("{update_path}/archive");
    seed_active_recruitment_participation(&pool, &user_id).await?;
    let members_revision_before_archive = members_revision(&pool).await?;
    let guild_audit_before_archive = member_status_audit_count(&pool, &user_id).await?;
    let blocked_archive = mutation(
        &app,
        Method::POST,
        &archive_path,
        json!({"expectedRevision":1}),
        "archive-top-01-blocked",
    )
    .await?;
    assert_eq!(blocked_archive.status(), StatusCode::CONFLICT);
    let blocked_archive = body_json(blocked_archive).await?;
    assert_eq!(
        blocked_archive["error"]["code"],
        "dummy_user_archive_blocked_by_recruitment"
    );
    assert_eq!(blocked_archive["error"]["details"]["joinedRecruitments"], 1);
    assert_eq!(
        blocked_archive["error"]["details"]["joinedAuctionRecruitments"],
        1
    );
    assert_eq!(dummy_state(&pool, &user_id).await?, "ACTIVE");
    assert_eq!(guild_member_status(&pool, &user_id).await?, "ACTIVE");
    assert!(user_deleted_at(&pool, &user_id).await?.is_none());
    assert_eq!(
        members_revision(&pool).await?,
        members_revision_before_archive
    );
    assert_eq!(
        member_status_audit_count(&pool, &user_id).await?,
        guild_audit_before_archive
    );

    withdraw_active_recruitment_participation(&pool, &user_id).await?;
    let archived = mutation(
        &app,
        Method::POST,
        &archive_path,
        json!({"expectedRevision":1}),
        "archive-top-01",
    )
    .await?;
    let archived = body_json(archived).await?;
    assert_eq!(archived["data"]["state"], "ARCHIVED");
    assert_eq!(
        archived["data"]["guildMemberships"][0]["status"],
        "INACTIVE"
    );
    assert_eq!(guild_member_status(&pool, &user_id).await?, "INACTIVE");
    assert!(user_deleted_at(&pool, &user_id).await?.is_none());
    assert_eq!(
        members_revision(&pool).await?,
        members_revision_before_archive + 1
    );
    assert_eq!(
        member_status_audit_count(&pool, &user_id).await?,
        guild_audit_before_archive + 1
    );

    let restore_path = format!("{update_path}/restore");
    let restored = mutation(
        &app,
        Method::POST,
        &restore_path,
        json!({"expectedRevision":2}),
        "restore-top-01",
    )
    .await?;
    let restored = body_json(restored).await?;
    assert_eq!(restored["data"]["state"], "ACTIVE");
    assert_eq!(
        restored["data"]["guildMemberships"][0]["status"],
        "INACTIVE"
    );
    assert_eq!(guild_member_status(&pool, &user_id).await?, "INACTIVE");
    assert!(user_deleted_at(&pool, &user_id).await?.is_none());
    assert_eq!(
        members_revision(&pool).await?,
        members_revision_before_archive + 1
    );
    assert_eq!(
        member_status_audit_count(&pool, &user_id).await?,
        guild_audit_before_archive + 1
    );

    let exact_update_replay = mutation(
        &app,
        Method::PATCH,
        &update_path,
        json!({"expectedRevision":0,"displayName":"Top Renamed"}),
        "update-top-01",
    )
    .await?;
    assert_eq!(
        body_json(exact_update_replay).await?["data"],
        updated["data"]
    );

    let public_suggestions =
        search_suggestions::public_riot_accounts(&pool, "KR", "dummy", 10).await?;
    assert!(public_suggestions.is_empty());

    let invalid_bulk = mutation(
        &app,
        Method::POST,
        "/api/dev/dummy-users/bulk",
        json!({"users":[
            dummy_body("bulk-rollback-01", "Rollback 01", None),
            dummy_body("bulk-rollback-02", "Rollback 02", Some(MISSING_GUILD_ID))
        ]}),
        "bulk-rollback",
    )
    .await?;
    assert_eq!(invalid_bulk.status(), StatusCode::NOT_FOUND);
    assert_eq!(stable_key_count(&pool, "bulk-rollback-%").await?, 0);

    let bulk = mutation(
        &app,
        Method::POST,
        "/api/dev/dummy-users/bulk",
        json!({"users":[
            dummy_body("bulk-01", "Bulk 01", None),
            dummy_body("bulk-02", "Bulk 02", None)
        ]}),
        "bulk-create",
    )
    .await?;
    assert_eq!(bulk.status(), StatusCode::CREATED);
    let bulk = body_json(bulk).await?;
    assert_eq!(bulk["data"]["total"], 2);
    let bulk_first_id = bulk["data"]["users"][0]["userId"]
        .as_str()
        .ok_or("bulk user ID missing")?;

    let group = mutation(
        &app,
        Method::POST,
        "/api/dev/dummy-groups",
        json!({"name":"Top pool","memberIds":[bulk_first_id]}),
        "group-create",
    )
    .await?;
    assert_eq!(group.status(), StatusCode::CREATED);
    let group = body_json(group).await?;
    assert_eq!(group["data"]["memberCount"], 1);
    let group_replay = mutation(
        &app,
        Method::POST,
        "/api/dev/dummy-groups",
        json!({"name":"Top pool","memberIds":[bulk_first_id]}),
        "group-create",
    )
    .await?;
    assert_eq!(body_json(group_replay).await?["data"], group["data"]);

    let impact = authenticated_get(&app, &format!("{update_path}/deletion-impact")).await?;
    assert_eq!(impact.status(), StatusCode::OK);
    assert_eq!(body_json(impact).await?["data"]["canDelete"], false);
    let blocked_delete = mutation(
        &app,
        Method::DELETE,
        &update_path,
        json!({}),
        "delete-blocked",
    )
    .await?;
    assert_eq!(blocked_delete.status(), StatusCode::CONFLICT);
    assert_eq!(
        body_json(blocked_delete).await?["error"]["code"],
        "dummy_user_deletion_blocked"
    );

    let delete_path = format!("/api/dev/dummy-users/{bulk_first_id}");
    let deleted = mutation(
        &app,
        Method::DELETE,
        &delete_path,
        json!({}),
        "delete-unattached",
    )
    .await?;
    assert_eq!(deleted.status(), StatusCode::OK);
    let deleted = body_json(deleted).await?;
    assert_eq!(deleted["data"]["deleted"], true);
    let delete_replay = mutation(
        &app,
        Method::DELETE,
        &delete_path,
        json!({}),
        "delete-unattached",
    )
    .await?;
    assert_eq!(body_json(delete_replay).await?["data"], deleted["data"]);

    assert_eq!(dummy_user_audit_count(&pool, &user_id).await?, 4);

    seed_dummy_user_list_page(&pool).await?;
    let list_response = tokio::time::timeout(
        Duration::from_secs(2),
        authenticated_get(
            &app,
            "/api/dev/dummy-users?query=perf-list-&limit=100&offset=0",
        ),
    )
    .await
    .map_err(|_| "listing a 100-user page exceeded 2 seconds")??;
    assert_eq!(list_response.status(), StatusCode::OK);
    let list_data = body_json(list_response).await?;
    assert_eq!(list_data["data"]["total"], 100);
    let listed_users = list_data["data"]["users"]
        .as_array()
        .ok_or("dummy user list response is not an array")?;
    assert_eq!(listed_users.len(), 100);
    assert!(listed_users.iter().all(|user| {
        user["stableKey"]
            .as_str()
            .is_some_and(|stable_key| stable_key.starts_with("perf-list-"))
            && user["identityKind"] == "DUMMY"
    }));
    let related_user = listed_users
        .iter()
        .find(|user| user["stableKey"] == "perf-list-000")
        .ok_or("related performance-list user missing")?;
    assert_eq!(related_user["privacy"]["publicProfileEnabled"], true);
    assert_eq!(related_user["riotAccount"]["gameName"], "Performance Riot");
    assert_eq!(
        related_user["championPreferences"]["TOP"],
        json!(["Aatrox", "Ahri"])
    );
    assert_eq!(related_user["guildMemberships"][0]["guildId"], GUILD_ID);
    assert_eq!(
        related_user["guildMemberships"][0]["laneMmr"]["top"],
        1610.0
    );
    assert_eq!(related_user["groupIds"].as_array().map(Vec::len), Some(1));
    Ok(())
}

fn dummy_body(stable_key: &str, display_name: &str, guild_id: Option<&str>) -> Value {
    let guild_memberships = guild_id.map_or_else(Vec::new, |guild_id| {
        vec![json!({
            "guildId": guild_id,
            "displayName": display_name,
            "status": "ACTIVE",
            "mainLane": "TOP",
            "laneMmr": {
                "top": 1610.0,
                "jungle": 1500.0,
                "mid": 1500.0,
                "bottom": 1500.0,
                "support": 1500.0
            }
        })]
    });
    json!({
        "stableKey": stable_key,
        "displayName": display_name,
        "privacy": {
            "publicProfileEnabled": false,
            "showRiotIds": true,
            "allowExternalRecordSearch": false
        },
        "guildMemberships": guild_memberships,
        "riotAccount": {
            "platform": "KR",
            "routingRegion": "ASIA",
            "gameName": format!("Dummy {display_name}"),
            "tagLine": "TEST",
            "profileIconId": 29,
            "mainPosition": "TOP"
        }
    })
}

async fn seed_active_recruitment_participation(pool: &PgPool, user_id: &str) -> TestResult {
    let season_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM seasons WHERE guild_id = $1 AND status = 'ACTIVE'",
    )
    .bind(GUILD_ID)
    .fetch_one(pool)
    .await?;
    let recruitment_id = Uuid::now_v7();
    let auction_recruitment_id = Uuid::now_v7();
    let now = OffsetDateTime::now_utc();

    sqlx::query(
        "INSERT INTO recruitments (
             guild_id, id, season_id, display_number, title, target_count, status, revision,
             channel_id, message_id, created_by, created_at, updated_by, updated_at,
             closed_at, converted_at, cancelled_at
         ) VALUES ($1, $2, $3, 1, 'Archive blocker', 10, 'OPEN', 0,
                   NULL, NULL, $4, $5, $4, $5, NULL, NULL, NULL)",
    )
    .bind(GUILD_ID)
    .bind(recruitment_id)
    .bind(season_id)
    .bind(ADMIN_ID)
    .bind(now)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO recruitment_participants (
             guild_id, recruitment_id, user_id, status, joined_at, withdrawn_at
         ) VALUES ($1, $2, $3, 'JOINED', $4, NULL)",
    )
    .bind(GUILD_ID)
    .bind(recruitment_id)
    .bind(user_id)
    .bind(now)
    .execute(pool)
    .await?;

    sqlx::query(
        "INSERT INTO auction_recruitments (
             guild_id, id, season_id, display_number, title, format, target_count, status,
             revision, channel_id, message_id, created_by, created_at, updated_by, updated_at,
             closed_at, converted_at, cancelled_at
         ) VALUES ($1, $2, $3, 1, 'Auction archive blocker', 'TEN_PERSON', 10, 'CLOSED',
                   0, NULL, NULL, $4, $5, $4, $5, $5, NULL, NULL)",
    )
    .bind(GUILD_ID)
    .bind(auction_recruitment_id)
    .bind(season_id)
    .bind(ADMIN_ID)
    .bind(now)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO auction_recruitment_participants (
             guild_id, recruitment_id, user_id, status, declared_lane, mmr_at_join,
             snapshot_origin, joined_at, withdrawn_at
         ) VALUES ($1, $2, $3, 'JOINED', NULL, NULL, 'UNAVAILABLE', $4, NULL)",
    )
    .bind(GUILD_ID)
    .bind(auction_recruitment_id)
    .bind(user_id)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

async fn withdraw_active_recruitment_participation(pool: &PgPool, user_id: &str) -> TestResult {
    let now = OffsetDateTime::now_utc();
    sqlx::query(
        "UPDATE recruitment_participants
         SET status = 'WITHDRAWN', withdrawn_at = $1
         WHERE guild_id = $2 AND user_id = $3 AND status = 'JOINED'",
    )
    .bind(now)
    .bind(GUILD_ID)
    .bind(user_id)
    .execute(pool)
    .await?;
    sqlx::query(
        "UPDATE auction_recruitment_participants
         SET status = 'WITHDRAWN', withdrawn_at = $1
         WHERE guild_id = $2 AND user_id = $3 AND status = 'JOINED'",
    )
    .bind(now)
    .bind(GUILD_ID)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(())
}

async fn dummy_state(pool: &PgPool, user_id: &str) -> TestResult<String> {
    Ok(
        sqlx::query_scalar("SELECT state FROM dummy_user_profiles WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(pool)
            .await?,
    )
}

async fn guild_member_status(pool: &PgPool, user_id: &str) -> TestResult<String> {
    Ok(
        sqlx::query_scalar("SELECT status FROM guild_members WHERE guild_id = $1 AND user_id = $2")
            .bind(GUILD_ID)
            .bind(user_id)
            .fetch_one(pool)
            .await?,
    )
}

async fn user_deleted_at(pool: &PgPool, user_id: &str) -> TestResult<Option<OffsetDateTime>> {
    Ok(
        sqlx::query_scalar("SELECT deleted_at FROM users WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(pool)
            .await?,
    )
}

async fn members_revision(pool: &PgPool) -> TestResult<i64> {
    Ok(sqlx::query_scalar(
        "SELECT revision FROM guild_admin_state WHERE guild_id = $1 AND scope = 'MEMBERS'",
    )
    .bind(GUILD_ID)
    .fetch_one(pool)
    .await?)
}

async fn member_status_audit_count(pool: &PgPool, user_id: &str) -> TestResult<i64> {
    Ok(sqlx::query_scalar(
        "SELECT count(*) FROM admin_audit_log
         WHERE guild_id = $1 AND action = 'MEMBER_STATUS_CHANGED'
           AND target_type = 'GUILD_MEMBER' AND target_id = $2",
    )
    .bind(GUILD_ID)
    .bind(user_id)
    .fetch_one(pool)
    .await?)
}

async fn create_login(
    repository: &AuthRepository,
    discord_user_id: &str,
    display_name: &str,
    session_token: &str,
    csrf_token: &str,
) -> TestResult {
    repository
        .complete_login(&NewLogin {
            discord_user_id: DiscordUserId::parse(discord_user_id)?,
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
         ) VALUES ($1, 'Dummy Test Guild', NULL, 'ACTIVE', $2, $3, $3, NULL, $3)",
    )
    .bind(GUILD_ID)
    .bind(ADMIN_ID)
    .bind(now)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO guild_members (
             guild_id, user_id, display_name, status, main_lane, first_seen_at,
             registered_at, discord_joined_at, last_active_at, last_synced_at, inactivated_at
         ) VALUES ($1, $2, 'Service Admin', 'ACTIVE', 'MID', $3, $3, $3, $3, $3, NULL)",
    )
    .bind(GUILD_ID)
    .bind(ADMIN_ID)
    .bind(now)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO guild_config (
             guild_id, display_name, operator_role_id, recruitment_channel_id,
             log_channel_id, is_externally_public, revision, updated_by, updated_at
         ) VALUES ($1, 'Dummy Test Guild', $2, NULL, NULL, false, 0, $3, $4)",
    )
    .bind(GUILD_ID)
    .bind(OPERATOR_ROLE_ID)
    .bind(ADMIN_ID)
    .bind(now)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO seasons (
             guild_id, id, name, status, started_at, ended_at, created_by, created_at
         ) VALUES ($1, $2, 'Season 1', 'ACTIVE', $3, NULL, $4, $3)",
    )
    .bind(GUILD_ID)
    .bind(Uuid::now_v7())
    .bind(now)
    .bind(ADMIN_ID)
    .execute(pool)
    .await?;
    Ok(())
}

async fn mutation(
    app: &Router,
    method: Method,
    path: &str,
    body: Value,
    idempotency_key: &str,
) -> TestResult<Response<Body>> {
    send(app, method, path, Some(body), Some(idempotency_key)).await
}

async fn authenticated_get(app: &Router, path: &str) -> TestResult<Response<Body>> {
    send(app, Method::GET, path, None, None).await
}

async fn send(
    app: &Router,
    method: Method,
    path: &str,
    body: Option<Value>,
    idempotency_key: Option<&str>,
) -> TestResult<Response<Body>> {
    let csrf_header = body.as_ref().map(|_| CSRF_TOKEN);
    send_with_auth(
        app,
        method,
        path,
        body,
        idempotency_key,
        SESSION_TOKEN,
        CSRF_TOKEN,
        csrf_header,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn send_with_auth(
    app: &Router,
    method: Method,
    path: &str,
    body: Option<Value>,
    idempotency_key: Option<&str>,
    session_token: &str,
    csrf_cookie: &str,
    csrf_header: Option<&str>,
) -> TestResult<Response<Body>> {
    let mut request = Request::builder().method(method).uri(path).header(
        header::COOKIE,
        format!("monkeybot_session={session_token}; monkeybot_csrf={csrf_cookie}"),
    );
    let request_body = if let Some(body) = body {
        request = request
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ORIGIN, PUBLIC_ORIGIN);
        if let Some(csrf_header) = csrf_header {
            request = request.header("x-csrf-token", csrf_header);
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

async fn body_json(response: Response<Body>) -> TestResult<Value> {
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

async fn dummy_count(pool: &PgPool) -> TestResult<i64> {
    Ok(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM users WHERE identity_kind = 'DUMMY'")
            .fetch_one(pool)
            .await?,
    )
}

async fn stable_key_count(pool: &PgPool, pattern: &str) -> TestResult<i64> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM dummy_user_profiles WHERE stable_key LIKE $1",
    )
    .bind(pattern)
    .fetch_one(pool)
    .await?)
}

async fn dummy_user_audit_count(pool: &PgPool, user_id: &str) -> TestResult<i64> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM dummy_user_audit_log
         WHERE target_type = 'DUMMY_USER' AND target_id = $1",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?)
}

async fn seed_dummy_user_list_page(pool: &PgPool) -> TestResult {
    let now = OffsetDateTime::now_utc();
    let user_ids = (0..100)
        .map(|_| format!("dmy_{}", Uuid::now_v7()))
        .collect::<Vec<_>>();
    let stable_keys = (0..100)
        .map(|index| format!("perf-list-{index:03}"))
        .collect::<Vec<_>>();
    let display_names = (0..100)
        .map(|index| format!("Performance List {index:03}"))
        .collect::<Vec<_>>();
    let mut transaction = pool.begin().await?;
    sqlx::query(
        "INSERT INTO users (
             user_id, display_name, avatar_hash, created_at, updated_at,
             deleted_at, identity_kind
         )
         SELECT input.user_id, input.display_name, NULL, $3, $3, NULL, 'DUMMY'
         FROM UNNEST($1::text[], $2::text[]) AS input(user_id, display_name)",
    )
    .bind(&user_ids)
    .bind(&display_names)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "INSERT INTO dummy_user_profiles (
             user_id, stable_key, state, avatar_seed, notes, revision,
             created_by, updated_by, created_at, updated_at
         )
         SELECT input.user_id, input.stable_key, 'ACTIVE', NULL, NULL, 0,
                $3, $3, $4, $4
         FROM UNNEST($1::text[], $2::text[]) AS input(user_id, stable_key)",
    )
    .bind(&user_ids)
    .bind(&stable_keys)
    .bind(ADMIN_ID)
    .bind(now)
    .execute(&mut *transaction)
    .await?;

    let related_user_id = &user_ids[0];
    sqlx::query(
        "INSERT INTO user_privacy_settings (
             user_id, public_profile_enabled, show_riot_ids,
             allow_external_record_search, updated_at
         ) VALUES ($1, true, true, false, $2)",
    )
    .bind(related_user_id)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "INSERT INTO riot_accounts (
             puuid, user_id, platform, routing_region, game_name, tag_line,
             normalized_game_name, normalized_tag_line, profile_icon_id, is_main,
             main_position, main_position_updated_at, last_synced_at,
             created_at, updated_at, origin
         ) VALUES (
             'dummy:perf-list-000', $1, 'KR', 'ASIA', 'Performance Riot', 'PERF',
             'performance riot', 'perf', 29, true, 'TOP', $2, NULL, $2, $2, 'DUMMY'
         )",
    )
    .bind(related_user_id)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "INSERT INTO user_champion_preferences (user_id, lane, position, champion_id)
         VALUES ($1, 'TOP', 1, 'Aatrox'), ($1, 'TOP', 2, 'Ahri')",
    )
    .bind(related_user_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "INSERT INTO guild_members (
             guild_id, user_id, display_name, status, main_lane, first_seen_at,
             registered_at, discord_joined_at, last_active_at, last_synced_at,
             inactivated_at
         ) VALUES ($1, $2, 'Performance List 000', 'ACTIVE', 'TOP', $3,
                   $3, NULL, NULL, NULL, NULL)",
    )
    .bind(GUILD_ID)
    .bind(related_user_id)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    let season_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM seasons WHERE guild_id = $1 AND status = 'ACTIVE'",
    )
    .bind(GUILD_ID)
    .fetch_one(&mut *transaction)
    .await?;
    sqlx::query(
        "INSERT INTO user_lane_mmr (
             guild_id, season_id, user_id, lane, mmr, games_played, wins,
             revision, updated_at
         )
         SELECT $1, $2, $3, input.lane, input.mmr, 0, 0, 0, $4
         FROM (VALUES
             ('TOP', 1610.0::double precision),
             ('JUNGLE', 1500.0::double precision),
             ('MID', 1500.0::double precision),
             ('BOTTOM', 1500.0::double precision),
             ('SUPPORT', 1500.0::double precision)
         ) AS input(lane, mmr)",
    )
    .bind(GUILD_ID)
    .bind(season_id)
    .bind(related_user_id)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    let group_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO dummy_user_groups (
             id, name, description, revision, created_by, updated_by,
             created_at, updated_at
         ) VALUES ($1, 'Performance List Group', NULL, 0, $2, $2, $3, $3)",
    )
    .bind(group_id)
    .bind(ADMIN_ID)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "INSERT INTO dummy_user_group_members (group_id, user_id, position, added_at)
         VALUES ($1, $2, 1, $3)",
    )
    .bind(group_id)
    .bind(related_user_id)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(())
}

fn test_config(database_url: String, enabled: bool) -> TestResult<Config> {
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
            ADMIN_ID.to_owned(),
        ),
        (
            "MONKEYBOT_DUMMY_USERS_ENABLED".to_owned(),
            enabled.to_string(),
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
