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
    domain::{DiscordUserId, UserId},
    infrastructure::postgres::{MIGRATOR, PostgresReadinessProbe},
    integrations::discord::{DiscordClient, DiscordClientError, DiscordUser},
    realtime::{RealtimeRuntime, RealtimeTopic, TopicAuthorizationError},
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
const POSTGRES_PASSWORD: &str = "monkeybot-web-recruitment-password";
const DATABASE: &str = "monkeybot_web_recruitment";
const BOT_TOKEN: &str = "bot-service-token-that-is-at-least-32-characters";
const GUILD_ID: &str = "200000000000000001";
const MEMBER_ID: &str = "100000000000000001";
const OUTSIDER_ID: &str = "100000000000000002";
const CANDIDATE_ID: &str = "100000000000000003";
const FULL_ROSTER_MEMBER_IDS: [&str; 10] = [
    MEMBER_ID,
    CANDIDATE_ID,
    "100000000000000004",
    "100000000000000005",
    "100000000000000006",
    "100000000000000007",
    "100000000000000008",
    "100000000000000009",
    "100000000000000010",
    "100000000000000011",
];
const OPERATOR_ROLE_ID: &str = "300000000000000001";
const RECRUITMENT_CHANNEL_ID: &str = "400000000000000001";
const ENTRY_CHANNEL_ID: &str = "400000000000000002";
const RESULT_CHANNEL_ID: &str = "400000000000000003";
const MESSAGE_ID: &str = "500000000000000001";
const BOT_INSTANCE_ID: &str = "web-recruitment-test-bot";
const SEASON_ID: &str = "01900000-0000-7000-8000-000000000001";
const MEMBER_SESSION: &str = "web-recruitment-member-session";
const MEMBER_CSRF: &str = "web-recruitment-member-csrf";
const OUTSIDER_SESSION: &str = "web-recruitment-outsider-session";
const OUTSIDER_CSRF: &str = "web-recruitment-outsider-csrf";
const CANDIDATE_SESSION: &str = "web-recruitment-candidate-session";
const CANDIDATE_CSRF: &str = "web-recruitment-candidate-csrf";
const DISCORD_CLIENT_ID: &str = "900000000000000001";
const PUBLIC_ORIGIN: &str = "http://localhost:5173";
const OAUTH_KEY_BASE64URL: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn active_member_can_create_a_real_recruitment_from_the_web() -> TestResult {
    tokio::time::timeout(Duration::from_secs(120), run_flow())
        .await
        .map_err(|_| "web recruitment create endpoint test exceeded 120 seconds")??;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn active_operator_can_convert_a_complete_entry_draft_into_a_series() -> TestResult {
    tokio::time::timeout(Duration::from_secs(120), run_series_conversion_flow())
        .await
        .map_err(|_| "web series conversion endpoint test exceeded 120 seconds")??;
    Ok(())
}

async fn run_series_conversion_flow() -> TestResult {
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
        MEMBER_ID,
        "Active Member",
        MEMBER_SESSION,
        MEMBER_CSRF,
    )
    .await?;
    create_login(
        &auth_repository,
        CANDIDATE_ID,
        "Eligible Candidate",
        CANDIDATE_SESSION,
        CANDIDATE_CSRF,
    )
    .await?;
    for (index, user_id) in FULL_ROSTER_MEMBER_IDS.iter().skip(2).enumerate() {
        let session = format!("web-series-full-roster-{index}-session");
        let csrf = format!("web-series-full-roster-{index}-csrf");
        create_login(
            &auth_repository,
            user_id,
            &format!("Full Roster Member {}", index + 3),
            &session,
            &csrf,
        )
        .await?;
    }
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
    let recruitment_id = seed_closed_entry_recruitment(&pool, 3, true).await?;
    let series_path = format!("/api/guilds/{GUILD_ID}/series");
    let request_body = || {
        json!({
            "sourceRecruitmentId": recruitment_id,
            "expectedRevision": 7,
        })
    };

    let unauthenticated = app
        .clone()
        .oneshot(
            Request::post(&series_path)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&request_body())?))?,
        )
        .await?;
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);

    let missing_csrf = browser_mutation(
        &app,
        Method::POST,
        &series_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-series-missing-csrf",
        false,
        request_body(),
        None,
    )
    .await?;
    assert_eq!(missing_csrf.status(), StatusCode::FORBIDDEN);
    let missing_csrf = body_json(missing_csrf).await?;
    assert_eq!(missing_csrf["error"]["code"], "csrf_validation_failed");
    assert_eq!(
        missing_csrf["error"]["message"],
        "요청 출처를 확인할 수 없습니다."
    );

    let non_operator = browser_mutation(
        &app,
        Method::POST,
        &series_path,
        CANDIDATE_SESSION,
        CANDIDATE_CSRF,
        "web-series-non-operator",
        true,
        request_body(),
        None,
    )
    .await?;
    assert_eq!(non_operator.status(), StatusCode::FORBIDDEN);
    let non_operator = body_json(non_operator).await?;
    assert_eq!(non_operator["error"]["code"], "operator_required");
    assert_eq!(
        non_operator["error"]["message"],
        "길드 운영자 권한이 필요합니다."
    );

    let incomplete = browser_mutation(
        &app,
        Method::POST,
        &series_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-series-incomplete-entry",
        true,
        request_body(),
        None,
    )
    .await?;
    assert_eq!(incomplete.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let incomplete = body_json(incomplete).await?;
    assert_eq!(incomplete["error"]["code"], "entry_draft_incomplete");
    assert_eq!(
        incomplete["error"]["message"],
        "참가자 10명을 양 팀의 모든 라인에 배치한 뒤 시리즈를 시작할 수 있습니다."
    );
    assert_eq!(incomplete["error"]["details"]["joinedCount"], 10);
    assert_eq!(incomplete["error"]["details"]["assignedCount"], 0);
    assert_eq!(incomplete["error"]["details"]["targetCount"], 10);

    let entry_path = format!("/api/guilds/{GUILD_ID}/recruitments/{recruitment_id}/entry-draft");
    let before_assignments = browser_get(&app, &entry_path, MEMBER_SESSION).await?;
    assert_eq!(before_assignments.status(), StatusCode::OK);
    assert_eq!(
        body_json(before_assignments).await?["data"]["capabilities"]["canCreateSeries"],
        false
    );

    seed_complete_entry_assignments(&pool, &recruitment_id).await?;
    let ready_entry = browser_get(&app, &entry_path, MEMBER_SESSION).await?;
    assert_eq!(ready_entry.status(), StatusCode::OK);
    let ready_entry = body_json(ready_entry).await?;
    assert_eq!(ready_entry["data"]["capabilities"]["canCreateSeries"], true);
    assert_eq!(ready_entry["data"]["recruitment"]["bestOf"], 3);
    assert_eq!(ready_entry["data"]["recruitment"]["hardFearless"], true);

    let created = browser_mutation(
        &app,
        Method::POST,
        &series_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-series-create-1",
        true,
        request_body(),
        None,
    )
    .await?;
    assert_eq!(created.status(), StatusCode::CREATED);
    let created = body_json(created).await?;
    let series_id = created["data"]["series"]["id"]
        .as_str()
        .ok_or("series creation response did not contain an ID")?
        .to_owned();
    assert_eq!(
        created["data"]["series"]["href"],
        format!("/g/{GUILD_ID}/series/{series_id}/pick-ban")
    );

    let pick_ban_path = format!("/api/guilds/{GUILD_ID}/series/{series_id}");
    let pick_ban_snapshot = browser_get(&app, &pick_ban_path, MEMBER_SESSION).await?;
    assert_eq!(pick_ban_snapshot.status(), StatusCode::OK);
    let pick_ban_snapshot = body_json(pick_ban_snapshot).await?;
    assert_eq!(pick_ban_snapshot["data"]["series"]["id"], series_id);
    assert_eq!(pick_ban_snapshot["data"]["series"]["status"], "IN_PROGRESS");
    assert_eq!(pick_ban_snapshot["data"]["series"]["revision"], 0);
    assert_eq!(pick_ban_snapshot["data"]["series"]["hardFearless"], true);
    assert_eq!(
        pick_ban_snapshot["data"]["teams"].as_array().map(Vec::len),
        Some(2)
    );
    assert!(
        pick_ban_snapshot["data"]["teams"]
            .as_array()
            .is_some_and(|teams| teams.iter().all(|team| {
                team["players"]
                    .as_array()
                    .is_some_and(|players| players.len() == 5)
            }))
    );
    assert_eq!(pick_ban_snapshot["data"]["currentGame"]["setNumber"], 1);
    assert_eq!(
        pick_ban_snapshot["data"]["currentGame"]["sidePreset"],
        "DEFAULT"
    );
    assert_eq!(
        pick_ban_snapshot["data"]["currentGame"]["picks"]
            .as_object()
            .map(|picks| picks.len()),
        Some(10)
    );
    assert!(
        pick_ban_snapshot["data"]["currentGame"]["picks"]
            .as_object()
            .is_some_and(|picks| picks.values().all(Value::is_null))
    );
    assert_eq!(
        pick_ban_snapshot["data"]["currentGame"]["bans"]["BLUE"]
            .as_array()
            .map(Vec::len),
        Some(5)
    );
    assert_eq!(
        pick_ban_snapshot["data"]["currentGame"]["bans"]["RED"]
            .as_array()
            .map(Vec::len),
        Some(5)
    );
    assert_eq!(pick_ban_snapshot["data"]["capabilities"]["canRevert"], true);
    assert_eq!(pick_ban_snapshot["data"]["capabilities"]["canDelete"], true);
    assert_eq!(
        pick_ban_snapshot["data"]["capabilities"]["canEarlyComplete"],
        false
    );

    let realtime = RealtimeRuntime::from_config(&test_config(server.database_url())?)?
        .ok_or("test configuration must enable realtime")?;
    let series_topic = RealtimeTopic::parse(&format!("guild:{GUILD_ID}:series:{series_id}"))?;
    let normal_member = UserId::parse(CANDIDATE_ID)?;
    assert!(matches!(
        realtime
            .authorize_topic(&pool, &normal_member, &series_topic)
            .await,
        Err(TopicAuthorizationError::Forbidden)
    ));
    let operator = UserId::parse(MEMBER_ID)?;
    realtime
        .authorize_topic(&pool, &operator, &series_topic)
        .await
        .expect("active operator may subscribe to active-series invalidations");
    let disabled = sqlx::query("UPDATE guilds SET status = 'DISABLED' WHERE guild_id = $1")
        .bind(GUILD_ID)
        .execute(&pool)
        .await?;
    assert_eq!(disabled.rows_affected(), 1);
    assert!(matches!(
        realtime
            .authorize_topic(&pool, &operator, &series_topic)
            .await,
        Err(TopicAuthorizationError::Forbidden)
    ));
    let restored = sqlx::query("UPDATE guilds SET status = 'ACTIVE' WHERE guild_id = $1")
        .bind(GUILD_ID)
        .execute(&pool)
        .await?;
    assert_eq!(restored.rows_affected(), 1);

    let non_operator_pick_ban = browser_mutation(
        &app,
        Method::PATCH,
        &format!("{pick_ban_path}/pickban"),
        CANDIDATE_SESSION,
        CANDIDATE_CSRF,
        "web-series-pickban-non-operator",
        true,
        json!({
            "expectedRevision": 0,
            "operation": { "type": "SET_SIDE", "sidePreset": "SWAPPED" }
        }),
        None,
    )
    .await?;
    assert_eq!(non_operator_pick_ban.status(), StatusCode::FORBIDDEN);
    let non_operator_pick_ban = body_json(non_operator_pick_ban).await?;
    assert_eq!(non_operator_pick_ban["error"]["code"], "operator_required");

    let stale_pick_ban = browser_mutation(
        &app,
        Method::PATCH,
        &format!("{pick_ban_path}/pickban"),
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-series-pickban-stale",
        true,
        json!({
            "expectedRevision": 9,
            "operation": { "type": "SET_SIDE", "sidePreset": "SWAPPED" }
        }),
        None,
    )
    .await?;
    assert_eq!(stale_pick_ban.status(), StatusCode::CONFLICT);
    let stale_pick_ban = body_json(stale_pick_ban).await?;
    assert_eq!(stale_pick_ban["error"]["code"], "series_revision_conflict");
    assert_eq!(stale_pick_ban["error"]["details"]["expectedRevision"], 9);
    assert_eq!(stale_pick_ban["error"]["details"]["actualRevision"], 0);

    let replay = browser_mutation(
        &app,
        Method::POST,
        &series_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-series-create-1",
        true,
        request_body(),
        None,
    )
    .await?;
    assert_eq!(replay.status(), StatusCode::CREATED);
    assert_eq!(body_json(replay).await?["data"], created["data"]);

    let stale_revision = browser_mutation(
        &app,
        Method::POST,
        &series_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-series-stale-revision",
        true,
        request_body(),
        None,
    )
    .await?;
    assert_eq!(stale_revision.status(), StatusCode::CONFLICT);
    let stale_revision = body_json(stale_revision).await?;
    assert_eq!(
        stale_revision["error"]["code"],
        "recruitment_revision_conflict"
    );
    assert_eq!(stale_revision["error"]["details"]["expectedRevision"], 7);
    assert_eq!(stale_revision["error"]["details"]["actualRevision"], 8);

    let series_id = Uuid::parse_str(&series_id)?;
    let series = sqlx::query_as::<_, (Uuid, Uuid, i64, String, i16, bool, String, i64, String)>(
        "SELECT id, source_recruitment_id, display_number, title, best_of, hard_fearless, status, revision, created_by
         FROM series
         WHERE guild_id = $1 AND id = $2",
    )
    .bind(GUILD_ID)
    .bind(series_id)
    .fetch_one(&pool)
    .await?;
    assert_eq!(series.0, series_id);
    assert_eq!(series.1, Uuid::parse_str(&recruitment_id)?);
    assert_eq!(series.2, 1);
    assert_eq!(series.3, "일반 내전 #1");
    assert_eq!(series.4, 3);
    assert!(series.5);
    assert_eq!(series.6, "IN_PROGRESS");
    assert_eq!(series.7, 0);
    assert_eq!(series.8, MEMBER_ID);

    let participant_snapshot = sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT
             count(*),
             count(*) FILTER (
                 WHERE mmr_snapshot_origin = 'CAPTURED' AND mmr_at_lock = 1500
             ),
             count(*) FILTER (WHERE mmr_at_lock IS NULL)
         FROM series_participants
         WHERE guild_id = $1 AND series_id = $2",
    )
    .bind(GUILD_ID)
    .bind(series_id)
    .fetch_one(&pool)
    .await?;
    assert_eq!(participant_snapshot, (10, 10, 0));
    let initialized_mmr_rows = sqlx::query_scalar::<_, i64>(
        "SELECT count(*)
         FROM user_lane_mmr
         WHERE guild_id = $1 AND season_id = $2 AND mmr = 1500",
    )
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(SEASON_ID)?)
    .fetch_one(&pool)
    .await?;
    assert_eq!(initialized_mmr_rows, 10);

    let recruitment_state = sqlx::query_as::<_, (String, i64, bool)>(
        "SELECT status, revision, converted_at IS NOT NULL
         FROM recruitments
         WHERE guild_id = $1 AND id = $2",
    )
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(&recruitment_id)?)
    .fetch_one(&pool)
    .await?;
    assert_eq!(recruitment_state, ("CONVERTED".to_owned(), 8, true));

    let entry_notification = sqlx::query_as::<_, (String, String, String, Value)>(
        "SELECT notification_type, channel_id, deduplication_key, payload
         FROM bot_notification_outbox
         WHERE guild_id = $1 AND notification_type = 'ENTRY_CONFIRMATION_CREATE'",
    )
    .bind(GUILD_ID)
    .fetch_one(&pool)
    .await?;
    assert_eq!(entry_notification.0, "ENTRY_CONFIRMATION_CREATE");
    assert_eq!(entry_notification.1, ENTRY_CHANNEL_ID);
    assert_eq!(
        entry_notification.2,
        format!("series:{series_id}:entry-confirmation")
    );
    assert_eq!(entry_notification.3["schemaVersion"], 1);
    assert_eq!(entry_notification.3["seriesId"], series_id.to_string());
    assert_eq!(
        entry_notification.3["snapshot"]["series"]["id"],
        series_id.to_string()
    );
    assert_eq!(
        entry_notification.3["snapshot"]["series"]["bestOf"],
        json!(3)
    );
    assert_eq!(
        entry_notification.3["snapshot"]["series"]["hardFearless"],
        json!(true)
    );
    assert_eq!(
        entry_notification.3["snapshot"]["teams"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    assert!(
        entry_notification.3["snapshot"]["teams"]
            .as_array()
            .is_some_and(|teams| teams.iter().all(|team| {
                team["players"]
                    .as_array()
                    .is_some_and(|players| players.len() == 5)
            }))
    );
    assert_eq!(
        entry_notification.3["snapshot"]["links"]["pickBan"],
        format!("/g/{GUILD_ID}/series/{series_id}/pick-ban")
    );

    let converted_recruitment_notification = sqlx::query_scalar::<_, Value>(
        "SELECT payload
         FROM bot_notification_outbox
         WHERE guild_id = $1
           AND notification_type = 'RECRUITMENT_MESSAGE_UPDATE'
           AND payload ->> 'recruitmentId' = $2",
    )
    .bind(GUILD_ID)
    .bind(&recruitment_id)
    .fetch_one(&pool)
    .await?;
    assert_eq!(
        converted_recruitment_notification["snapshot"]["recruitment"]["status"],
        "CONVERTED"
    );

    let side_effects = sqlx::query_as::<_, (i64, i64, i64, i64)>(
        "SELECT
             (SELECT count(*) FROM admin_audit_log
                WHERE guild_id = $1 AND action = 'SERIES_CREATED'),
             (SELECT count(*) FROM outbox_events
                WHERE guild_id = $1 AND event_type = 'RECRUITMENT_CONVERTED'),
             (SELECT count(*) FROM outbox_events
                WHERE guild_id = $1 AND event_type = 'SERIES_CREATED'),
             (SELECT count(*) FROM idempotency_records
                WHERE guild_id = $1
                  AND principal_type = 'USER'
                  AND principal_id = $2
                  AND method = 'POST'
                  AND route_template = '/api/guilds/{guildId}/series')",
    )
    .bind(GUILD_ID)
    .bind(MEMBER_ID)
    .fetch_one(&pool)
    .await?;
    assert_eq!(side_effects, (1, 1, 1, 1));

    let pick_ban_update_path = format!("{pick_ban_path}/pickban");
    let draft_updated = browser_mutation(
        &app,
        Method::PATCH,
        &pick_ban_update_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-series-pickban-update-1",
        true,
        json!({
            "expectedRevision": 0,
            "operation": {
                "type": "SET_PICK",
                "playerId": MEMBER_ID,
                "championId": "Ahri"
            }
        }),
        None,
    )
    .await?;
    assert_eq!(draft_updated.status(), StatusCode::OK);
    let draft_updated = body_json(draft_updated).await?;
    assert_eq!(draft_updated["data"]["series"]["revision"], 1);
    assert_eq!(
        draft_updated["data"]["currentGame"]["picks"][MEMBER_ID],
        "Ahri"
    );

    let game_path = format!("{pick_ban_path}/games");
    let game_command = |set_number: i16, expected_revision: i64| {
        json!({
            "setNumber": set_number,
            "expectedRevision": expected_revision,
            "sidePreset": "DEFAULT",
            "winner": "BLUE",
            "patchVersion": "15.14"
        })
    };
    let first_game = browser_mutation(
        &app,
        Method::POST,
        &game_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-series-game-1",
        true,
        game_command(1, 1),
        None,
    )
    .await?;
    assert_eq!(first_game.status(), StatusCode::CREATED);
    let first_game = body_json(first_game).await?;
    assert_eq!(first_game["data"]["series"]["status"], "IN_PROGRESS");
    assert_eq!(first_game["data"]["series"]["revision"], 2);
    assert_eq!(first_game["data"]["confirmedGameCount"], 1);
    assert_eq!(
        first_game["data"]["nextHref"],
        format!("/g/{GUILD_ID}/series/{series_id}/pick-ban")
    );

    let repeated_hard_fearless_pick = browser_mutation(
        &app,
        Method::PATCH,
        &pick_ban_update_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-series-hard-fearless-repeated-pick",
        true,
        json!({
            "expectedRevision": 2,
            "operation": {
                "type": "SET_PICK",
                "playerId": MEMBER_ID,
                "championId": "Ahri"
            }
        }),
        None,
    )
    .await?;
    assert_eq!(
        repeated_hard_fearless_pick.status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let repeated_hard_fearless_pick = body_json(repeated_hard_fearless_pick).await?;
    assert_eq!(
        repeated_hard_fearless_pick["error"]["code"],
        "hard_fearless_champion_used"
    );
    assert_eq!(
        repeated_hard_fearless_pick["error"]["message"],
        "하드 피어리스에서는 이전 세트에서 픽한 챔피언을 다시 픽할 수 없습니다."
    );
    assert_eq!(
        repeated_hard_fearless_pick["error"]["details"]["championId"],
        "Ahri"
    );

    let duplicate_bulk_side_effects = series_pick_ban_draft_side_effects(&pool, series_id).await?;
    assert_eq!(duplicate_bulk_side_effects, (1, 1));

    let duplicate_bulk = browser_mutation(
        &app,
        Method::PATCH,
        &pick_ban_update_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-series-pickban-bulk-duplicate",
        true,
        json!({
            "expectedRevision": 2,
            "operation": {
                "type": "APPLY_BULK",
                "items": [
                    { "type": "SET_PICK", "playerId": CANDIDATE_ID, "championId": "Lux" },
                    { "type": "SET_BAN", "side": "BLUE", "slot": 1, "championId": "Lux" }
                ]
            }
        }),
        None,
    )
    .await?;
    assert_eq!(duplicate_bulk.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let duplicate_bulk = body_json(duplicate_bulk).await?;
    assert_eq!(
        duplicate_bulk["error"]["code"],
        "duplicate_series_game_champion"
    );
    assert_eq!(
        duplicate_bulk["error"]["message"],
        "같은 세트에서 픽과 밴은 같은 챔피언을 중복할 수 없습니다."
    );
    assert_eq!(duplicate_bulk["error"]["details"]["championId"], "Lux");

    let after_duplicate_bulk = browser_get(&app, &pick_ban_path, MEMBER_SESSION).await?;
    assert_eq!(after_duplicate_bulk.status(), StatusCode::OK);
    let after_duplicate_bulk = body_json(after_duplicate_bulk).await?;
    assert_eq!(after_duplicate_bulk["data"]["series"]["revision"], 2);
    assert!(after_duplicate_bulk["data"]["currentGame"]["picks"][CANDIDATE_ID].is_null());
    assert!(after_duplicate_bulk["data"]["currentGame"]["bans"]["BLUE"][0].is_null());
    assert_eq!(
        series_pick_ban_draft_side_effects(&pool, series_id).await?,
        duplicate_bulk_side_effects
    );

    let mut repeated_submit_picks = serde_json::Map::new();
    for user_id in FULL_ROSTER_MEMBER_IDS {
        repeated_submit_picks.insert(user_id.to_owned(), Value::Null);
    }
    repeated_submit_picks.insert(MEMBER_ID.to_owned(), json!("Ahri"));
    let repeated_hard_fearless_submit = browser_mutation(
        &app,
        Method::POST,
        &game_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-series-hard-fearless-repeated-submit",
        true,
        json!({
            "setNumber": 2,
            "expectedRevision": 2,
            "sidePreset": "DEFAULT",
            "winner": "BLUE",
            "patchVersion": "15.14",
            "picks": repeated_submit_picks
        }),
        None,
    )
    .await?;
    assert_eq!(
        repeated_hard_fearless_submit.status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let repeated_hard_fearless_submit = body_json(repeated_hard_fearless_submit).await?;
    assert_eq!(
        repeated_hard_fearless_submit["error"]["code"],
        "hard_fearless_champion_used"
    );

    let bulk_applied = browser_mutation(
        &app,
        Method::PATCH,
        &pick_ban_update_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-series-pickban-bulk-applied",
        true,
        json!({
            "expectedRevision": 2,
            "operation": {
                "type": "APPLY_BULK",
                "items": [
                    { "type": "SET_PICK", "playerId": CANDIDATE_ID, "championId": "Lux" },
                    { "type": "SET_BAN", "side": "BLUE", "slot": 1, "championId": "Yone" }
                ]
            }
        }),
        None,
    )
    .await?;
    assert_eq!(bulk_applied.status(), StatusCode::OK);
    let bulk_applied = body_json(bulk_applied).await?;
    assert_eq!(bulk_applied["data"]["series"]["revision"], 3);
    assert_eq!(bulk_applied["data"]["series"]["hardFearless"], true);
    assert_eq!(
        bulk_applied["data"]["currentGame"]["picks"][CANDIDATE_ID],
        "Lux"
    );
    assert_eq!(
        bulk_applied["data"]["currentGame"]["bans"]["BLUE"][0],
        "Yone"
    );

    set_result_channel(&pool, Some(RESULT_CHANNEL_ID)).await?;

    let second_game = browser_mutation(
        &app,
        Method::POST,
        &game_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-series-game-2",
        true,
        game_command(2, 3),
        None,
    )
    .await?;
    assert_eq!(second_game.status(), StatusCode::CREATED);
    let second_game = body_json(second_game).await?;
    assert_eq!(second_game["data"]["series"]["status"], "COMPLETED");
    assert_eq!(second_game["data"]["series"]["revision"], 4);
    assert_eq!(second_game["data"]["confirmedGameCount"], 2);
    assert_eq!(
        second_game["data"]["nextHref"],
        format!("/g/{GUILD_ID}/series/{series_id}/result")
    );
    let normal_result_notifications =
        series_result_notifications(&pool, &series_id.to_string()).await?;
    assert_eq!(normal_result_notifications.len(), 1);
    let (channel_id, deduplication_key, payload) = &normal_result_notifications[0];
    assert_eq!(channel_id, RESULT_CHANNEL_ID);
    assert_eq!(deduplication_key, &format!("series:{series_id}:result:4"));
    assert_eq!(payload["schemaVersion"], 2);
    assert_eq!(payload["seriesId"], series_id.to_string());
    assert_eq!(
        payload["snapshot"]["series"],
        json!({
            "id": series_id.to_string(),
            "displayNumber": 1,
            "title": "일반 내전 #1",
            "bestOf": 3,
            "hardFearless": true,
            "winningTeam": "TEAM_1",
            "terminalReason": "SCORE_REACHED",
        })
    );
    assert_eq!(
        payload["snapshot"]["score"],
        json!({ "team1": 2, "team2": 0 })
    );
    let notification_teams = payload["snapshot"]["teams"]
        .as_array()
        .ok_or("result notification teams are missing")?;
    assert_eq!(notification_teams.len(), 2);
    assert_eq!(notification_teams[0]["team"], "TEAM_1");
    assert_eq!(notification_teams[0]["name"], "1팀");
    assert_eq!(
        notification_teams[0]["players"].as_array().map(Vec::len),
        Some(5)
    );
    assert_eq!(
        notification_teams[0]["players"][0],
        json!({ "id": MEMBER_ID, "displayName": "Active Member", "lane": "TOP" })
    );
    assert_eq!(
        notification_teams[0]["players"][1],
        json!({ "id": CANDIDATE_ID, "displayName": "Eligible Candidate", "lane": "JUNGLE" })
    );
    assert_eq!(notification_teams[1]["team"], "TEAM_2");
    assert_eq!(notification_teams[1]["name"], "2팀");
    assert_eq!(
        notification_teams[1]["players"].as_array().map(Vec::len),
        Some(5)
    );
    assert!(
        notification_teams
            .iter()
            .all(|team| team.get("mmr").is_none())
    );

    let notification_games = payload["snapshot"]["games"]
        .as_array()
        .ok_or("result notification games are missing")?;
    assert_eq!(notification_games.len(), 2);
    assert_eq!(notification_games[0]["setNumber"], 1);
    assert_eq!(notification_games[0]["winner"], "TEAM_1");
    assert_eq!(notification_games[0]["sidePreset"], "DEFAULT");
    assert_eq!(notification_games[0]["patchVersion"], "15.14");
    assert_eq!(
        notification_games[0]["team1"]["picks"][0],
        json!({
            "id": MEMBER_ID,
            "displayName": "Active Member",
            "lane": "TOP",
            "championId": "Ahri"
        })
    );
    assert_eq!(
        notification_games[0]["team1"]["picks"][1]["championId"],
        Value::Null
    );
    assert_eq!(
        notification_games[0]["bans"],
        json!({
            "blue": [null, null, null, null, null],
            "red": [null, null, null, null, null]
        })
    );
    assert_eq!(notification_games[1]["setNumber"], 2);
    assert_eq!(notification_games[1]["winner"], "TEAM_1");
    assert_eq!(notification_games[1]["sidePreset"], "DEFAULT");
    assert_eq!(
        notification_games[1]["team1"]["picks"][1],
        json!({
            "id": CANDIDATE_ID,
            "displayName": "Eligible Candidate",
            "lane": "JUNGLE",
            "championId": "Lux"
        })
    );
    assert_eq!(notification_games[1]["bans"]["blue"][0], "Yone");
    assert_eq!(
        notification_games[1]["bans"]["red"],
        json!([null, null, null, null, null])
    );
    assert!(
        notification_games
            .iter()
            .all(|game| game.get("mmrChanges").is_none())
    );
    assert_eq!(
        payload["snapshot"]["links"]["result"],
        format!("/g/{GUILD_ID}/series/{series_id}/result")
    );
    assert!(payload["snapshot"].get("mmr").is_none());

    let result_path = format!("{pick_ban_path}/result");
    let result = browser_get(&app, &result_path, MEMBER_SESSION).await?;
    assert_eq!(result.status(), StatusCode::OK);
    let result = body_json(result).await?;
    assert_eq!(result["data"]["series"]["status"], "COMPLETED");
    assert_eq!(result["data"]["score"], json!({ "team1": 2, "team2": 0 }));
    assert_eq!(result["data"]["games"].as_array().map(Vec::len), Some(2));
    assert!(result["data"]["games"].as_array().is_some_and(|games| {
        games.iter().all(|game| {
            game["mmrChanges"]
                .as_array()
                .is_some_and(|changes| changes.len() == 10)
        })
    }));
    assert_eq!(result["data"]["capabilities"]["canRevertLastGame"], true);

    let revert_last_game_path = format!("{game_path}/last");
    let reverted_last_game = browser_mutation(
        &app,
        Method::DELETE,
        &revert_last_game_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-series-game-2-revert",
        true,
        json!({ "expectedRevision": 4 }),
        None,
    )
    .await?;
    assert_eq!(reverted_last_game.status(), StatusCode::OK);
    let reverted_last_game = body_json(reverted_last_game).await?;
    assert_eq!(
        reverted_last_game["data"]["series"]["status"],
        "IN_PROGRESS"
    );
    assert_eq!(reverted_last_game["data"]["series"]["revision"], 5);
    assert_eq!(reverted_last_game["data"]["confirmedGameCount"], 1);

    set_result_channel(&pool, None).await?;
    set_entry_channel(&pool, None).await?;

    let restored_second_game = browser_mutation(
        &app,
        Method::POST,
        &game_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-series-game-2-restored",
        true,
        game_command(2, 5),
        None,
    )
    .await?;
    assert_eq!(restored_second_game.status(), StatusCode::CREATED);
    assert_eq!(
        body_json(restored_second_game).await?["data"]["series"]["revision"],
        6
    );
    assert_eq!(
        series_result_notifications(&pool, &series_id.to_string())
            .await?
            .len(),
        1
    );

    let revert_path = format!("{pick_ban_path}/revert");
    let reverted = browser_mutation(
        &app,
        Method::POST,
        &revert_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-series-revert-1",
        true,
        json!({ "expectedRevision": 6 }),
        None,
    )
    .await?;
    assert_eq!(reverted.status(), StatusCode::OK);
    let reverted = body_json(reverted).await?;
    assert_eq!(
        reverted["data"]["nextHref"],
        format!("/g/{GUILD_ID}/recruitments/{recruitment_id}/entry")
    );
    let revert_replay = browser_mutation(
        &app,
        Method::POST,
        &revert_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-series-revert-1",
        true,
        json!({ "expectedRevision": 6 }),
        None,
    )
    .await?;
    assert_eq!(revert_replay.status(), StatusCode::OK);
    assert_eq!(body_json(revert_replay).await?["data"], reverted["data"]);

    let reverted_state = sqlx::query_as::<_, (String, i64, String, i64, bool)>(
        "SELECT recruitment.status,
                recruitment.revision,
                series.status,
                series.revision,
                recruitment.converted_at IS NULL
         FROM recruitments AS recruitment
         JOIN series
           ON series.guild_id = recruitment.guild_id
          AND series.source_recruitment_id = recruitment.id
         WHERE recruitment.guild_id = $1 AND recruitment.id = $2 AND series.id = $3",
    )
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(&recruitment_id)?)
    .bind(series_id)
    .fetch_one(&pool)
    .await?;
    assert_eq!(
        reverted_state,
        ("CLOSED".to_owned(), 9, "CANCELLED".to_owned(), 7, true)
    );
    let reverted_mmr = sqlx::query_as::<_, (i64, i64, i64, i64)>(
        "SELECT count(*),
                count(*) FILTER (WHERE mmr = 1500),
                count(*) FILTER (WHERE games_played = 0),
                count(*) FILTER (WHERE wins = 0)
         FROM user_lane_mmr
         WHERE guild_id = $1 AND season_id = $2",
    )
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(SEASON_ID)?)
    .fetch_one(&pool)
    .await?;
    assert_eq!(reverted_mmr, (10, 10, 10, 10));
    let reverted_games = sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT count(*) FILTER (WHERE status = 'CONFIRMED'),
                count(*) FILTER (WHERE status = 'VOIDED'),
                count(*) FILTER (WHERE mmr_status = 'REVERSED')
         FROM games
         WHERE guild_id = $1 AND series_id = $2",
    )
    .bind(GUILD_ID)
    .bind(series_id)
    .fetch_one(&pool)
    .await?;
    assert_eq!(reverted_games, (0, 3, 3));

    sqlx::query(
        "UPDATE recruitments
         SET best_of = 5,
             hard_fearless = false
         WHERE guild_id = $1 AND id = $2",
    )
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(&recruitment_id)?)
    .execute(&pool)
    .await?;

    let recreated = browser_mutation(
        &app,
        Method::POST,
        &series_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-series-create-2",
        true,
        json!({ "sourceRecruitmentId": recruitment_id, "expectedRevision": 9 }),
        None,
    )
    .await?;
    assert_eq!(recreated.status(), StatusCode::CREATED);
    let recreated = body_json(recreated).await?;
    let recreated_series_id = recreated["data"]["series"]["id"]
        .as_str()
        .ok_or("recreated series response did not contain an ID")?
        .to_owned();
    let recreated_pick_ban_path = format!("/api/guilds/{GUILD_ID}/series/{recreated_series_id}");
    let recreated_snapshot = browser_get(&app, &recreated_pick_ban_path, MEMBER_SESSION).await?;
    assert_eq!(recreated_snapshot.status(), StatusCode::OK);
    let recreated_snapshot = body_json(recreated_snapshot).await?;
    assert_eq!(recreated_snapshot["data"]["series"]["bestOf"], 5);
    assert_eq!(recreated_snapshot["data"]["series"]["hardFearless"], false);
    assert_eq!(
        recreated_snapshot["data"]["capabilities"]["canEarlyComplete"],
        false
    );

    let recreated_pick_ban_update_path = format!("{recreated_pick_ban_path}/pickban");
    let recreated_game_path = format!("{recreated_pick_ban_path}/games");
    let recreated_first_pick = browser_mutation(
        &app,
        Method::PATCH,
        &recreated_pick_ban_update_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-series-non-hard-fearless-pick-1",
        true,
        json!({
            "expectedRevision": 0,
            "operation": {
                "type": "SET_PICK",
                "playerId": MEMBER_ID,
                "championId": "Ahri"
            }
        }),
        None,
    )
    .await?;
    assert_eq!(recreated_first_pick.status(), StatusCode::OK);
    let recreated_first_game = browser_mutation(
        &app,
        Method::POST,
        &recreated_game_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-series-non-hard-fearless-game-1",
        true,
        game_command(1, 1),
        None,
    )
    .await?;
    assert_eq!(recreated_first_game.status(), StatusCode::CREATED);
    let recreated_repeated_pick = browser_mutation(
        &app,
        Method::PATCH,
        &recreated_pick_ban_update_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-series-non-hard-fearless-pick-2",
        true,
        json!({
            "expectedRevision": 2,
            "operation": {
                "type": "SET_PICK",
                "playerId": MEMBER_ID,
                "championId": "Ahri"
            }
        }),
        None,
    )
    .await?;
    assert_eq!(recreated_repeated_pick.status(), StatusCode::OK);

    let recreated_after_first_game =
        browser_get(&app, &recreated_pick_ban_path, MEMBER_SESSION).await?;
    assert_eq!(recreated_after_first_game.status(), StatusCode::OK);
    let recreated_after_first_game = body_json(recreated_after_first_game).await?;
    assert_eq!(
        recreated_after_first_game["data"]["capabilities"]["canEarlyComplete"],
        true
    );
    assert!(
        series_result_notifications(&pool, &recreated_series_id)
            .await?
            .is_empty()
    );
    set_entry_channel(&pool, Some(ENTRY_CHANNEL_ID)).await?;
    set_result_channel(&pool, None).await?;

    let early_complete_path = format!("{recreated_pick_ban_path}/complete");
    let early_draw = browser_mutation(
        &app,
        Method::POST,
        &early_complete_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-series-early-draw-invalid",
        true,
        json!({ "expectedRevision": 3, "outcome": "DRAW" }),
        None,
    )
    .await?;
    assert_eq!(early_draw.status(), StatusCode::CONFLICT);
    let early_draw = body_json(early_draw).await?;
    assert_eq!(
        early_draw["error"]["code"],
        "early_draw_requires_tied_score"
    );
    assert_eq!(
        early_draw["error"]["details"],
        json!({ "team1Score": 1, "team2Score": 0 })
    );

    let early_complete_body = json!({ "expectedRevision": 3, "outcome": "TEAM_1" });
    let early_complete = browser_mutation(
        &app,
        Method::POST,
        &early_complete_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-series-early-complete-1",
        true,
        early_complete_body.clone(),
        None,
    )
    .await?;
    assert_eq!(early_complete.status(), StatusCode::OK);
    let early_complete = body_json(early_complete).await?;
    assert_eq!(early_complete["data"]["series"]["status"], "COMPLETED");
    assert_eq!(early_complete["data"]["series"]["revision"], 4);
    assert_eq!(early_complete["data"]["confirmedGameCount"], 1);
    assert_eq!(
        early_complete["data"]["nextHref"],
        format!("/g/{GUILD_ID}/series/{recreated_series_id}/result")
    );

    let early_complete_replay = browser_mutation(
        &app,
        Method::POST,
        &early_complete_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-series-early-complete-1",
        true,
        early_complete_body,
        None,
    )
    .await?;
    assert_eq!(early_complete_replay.status(), StatusCode::OK);
    assert_eq!(
        body_json(early_complete_replay).await?["data"],
        early_complete["data"]
    );
    let early_result_notifications =
        series_result_notifications(&pool, &recreated_series_id).await?;
    assert!(
        early_result_notifications.is_empty(),
        "entry channel must not receive a result card when result channel is unset"
    );

    let recreated_result_path = format!("{recreated_pick_ban_path}/result");
    let recreated_result = browser_get(&app, &recreated_result_path, MEMBER_SESSION).await?;
    assert_eq!(recreated_result.status(), StatusCode::OK);
    let recreated_result = body_json(recreated_result).await?;
    assert_eq!(recreated_result["data"]["series"]["winningTeam"], "TEAM_1");
    assert_eq!(
        recreated_result["data"]["series"]["terminalReason"],
        "EARLY_WIN"
    );
    assert_eq!(
        recreated_result["data"]["score"],
        json!({ "team1": 1, "team2": 0 })
    );
    assert_eq!(
        recreated_result["data"]["games"].as_array().map(Vec::len),
        Some(1)
    );
    assert!(
        recreated_result["data"]["games"]
            .as_array()
            .is_some_and(|games| games[0]["mmrChanges"]
                .as_array()
                .is_some_and(|changes| changes.len() == 10))
    );

    let early_complete_side_effects = sqlx::query_as::<_, (i64, i64, i64, i64)>(
        "SELECT
             (SELECT count(*) FROM admin_audit_log
                WHERE guild_id = $1 AND action = 'SERIES_EARLY_COMPLETED' AND target_id = $2),
             (SELECT count(*) FROM outbox_events
                WHERE guild_id = $1 AND aggregate_type = 'SERIES' AND aggregate_id = $2
                  AND event_type = 'SERIES_EARLY_COMPLETED'),
             (SELECT count(*) FROM idempotency_records
                WHERE guild_id = $1 AND principal_type = 'USER' AND principal_id = $3
                  AND method = 'POST' AND route_template = '/api/guilds/{guildId}/series/{id}/complete'),
             (SELECT count(*) FROM game_inputs
                WHERE guild_id = $1 AND series_id = $4 AND set_number = 2 AND status = 'DRAFT')",
    )
    .bind(GUILD_ID)
    .bind(&recreated_series_id)
    .bind(MEMBER_ID)
    .bind(Uuid::parse_str(&recreated_series_id)?)
    .fetch_one(&pool)
    .await?;
    assert_eq!(early_complete_side_effects, (1, 1, 1, 1));

    let restored_after_early_win = browser_mutation(
        &app,
        Method::DELETE,
        &format!("{recreated_game_path}/last"),
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-series-early-win-revert-last",
        true,
        json!({ "expectedRevision": 4 }),
        None,
    )
    .await?;
    assert_eq!(restored_after_early_win.status(), StatusCode::OK);
    let restored_after_early_win = body_json(restored_after_early_win).await?;
    assert_eq!(
        restored_after_early_win["data"]["series"]["status"],
        "IN_PROGRESS"
    );
    assert_eq!(restored_after_early_win["data"]["series"]["revision"], 5);

    set_result_channel(&pool, Some(RESULT_CHANNEL_ID)).await?;
    let resubmitted_for_result = browser_mutation(
        &app,
        Method::POST,
        &recreated_game_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-series-early-win-result-game-1",
        true,
        game_command(1, 5),
        None,
    )
    .await?;
    assert_eq!(resubmitted_for_result.status(), StatusCode::CREATED);
    assert_eq!(
        body_json(resubmitted_for_result).await?["data"]["series"]["revision"],
        6
    );
    let early_win_with_result = browser_mutation(
        &app,
        Method::POST,
        &early_complete_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-series-early-complete-with-result",
        true,
        json!({ "expectedRevision": 6, "outcome": "TEAM_1" }),
        None,
    )
    .await?;
    assert_eq!(early_win_with_result.status(), StatusCode::OK);
    let early_win_with_result = body_json(early_win_with_result).await?;
    assert_eq!(
        early_win_with_result["data"]["series"]["status"],
        "COMPLETED"
    );
    assert_eq!(early_win_with_result["data"]["series"]["revision"], 7);

    let early_win_notifications = series_result_notifications(&pool, &recreated_series_id).await?;
    assert_eq!(early_win_notifications.len(), 1);
    let (channel_id, deduplication_key, payload) = &early_win_notifications[0];
    assert_eq!(channel_id, RESULT_CHANNEL_ID);
    assert_eq!(
        deduplication_key,
        &format!("series:{recreated_series_id}:result:7")
    );
    assert_eq!(payload["schemaVersion"], 2);
    assert_eq!(payload["snapshot"]["series"]["terminalReason"], "EARLY_WIN");
    assert_eq!(payload["snapshot"]["series"]["winningTeam"], "TEAM_1");
    assert_eq!(
        payload["snapshot"]["score"],
        json!({ "team1": 1, "team2": 0 })
    );
    assert_eq!(
        payload["snapshot"]["teams"].as_array().map(Vec::len),
        Some(2)
    );
    let early_win_games = payload["snapshot"]["games"]
        .as_array()
        .ok_or("early-win result notification games are missing")?;
    assert_eq!(early_win_games.len(), 1);
    assert_eq!(early_win_games[0]["setNumber"], 1);
    assert_eq!(early_win_games[0]["winner"], "TEAM_1");
    assert_eq!(early_win_games[0]["sidePreset"], "DEFAULT");
    assert_eq!(
        early_win_games[0]["team1"]["picks"]
            .as_array()
            .map(Vec::len),
        Some(5)
    );
    assert_eq!(
        early_win_games[0]["team2"]["picks"]
            .as_array()
            .map(Vec::len),
        Some(5)
    );
    assert!(early_win_games[0]["team1"]["picks"][0]["championId"].is_null());
    assert_eq!(
        early_win_games[0]["bans"],
        json!({
            "blue": [null, null, null, null, null],
            "red": [null, null, null, null, null]
        })
    );
    assert!(payload["snapshot"].get("mmr").is_none());

    let restored_after_result_notification = browser_mutation(
        &app,
        Method::DELETE,
        &format!("{recreated_game_path}/last"),
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-series-early-win-result-revert-last",
        true,
        json!({ "expectedRevision": 7 }),
        None,
    )
    .await?;
    assert_eq!(restored_after_result_notification.status(), StatusCode::OK);
    let restored_after_result_notification = body_json(restored_after_result_notification).await?;
    assert_eq!(
        restored_after_result_notification["data"]["series"]["status"],
        "IN_PROGRESS"
    );
    assert_eq!(
        restored_after_result_notification["data"]["series"]["revision"],
        8
    );

    let tied_first_game = browser_mutation(
        &app,
        Method::POST,
        &recreated_game_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-series-tied-game-1",
        true,
        game_command(1, 8),
        None,
    )
    .await?;
    assert_eq!(tied_first_game.status(), StatusCode::CREATED);
    assert_eq!(
        body_json(tied_first_game).await?["data"]["series"]["revision"],
        9
    );

    let tied_second_game = browser_mutation(
        &app,
        Method::POST,
        &recreated_game_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-series-tied-game-2",
        true,
        json!({
            "setNumber": 2,
            "expectedRevision": 9,
            "sidePreset": "DEFAULT",
            "winner": "RED",
            "patchVersion": "15.14"
        }),
        None,
    )
    .await?;
    assert_eq!(tied_second_game.status(), StatusCode::CREATED);
    let tied_second_game = body_json(tied_second_game).await?;
    assert_eq!(tied_second_game["data"]["series"]["status"], "IN_PROGRESS");
    assert_eq!(tied_second_game["data"]["series"]["revision"], 10);
    assert_eq!(tied_second_game["data"]["confirmedGameCount"], 2);

    set_result_channel(&pool, Some(RESULT_CHANNEL_ID)).await?;

    let early_draw = browser_mutation(
        &app,
        Method::POST,
        &early_complete_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-series-early-draw-1",
        true,
        json!({ "expectedRevision": 10, "outcome": "DRAW" }),
        None,
    )
    .await?;
    assert_eq!(early_draw.status(), StatusCode::OK);
    let early_draw = body_json(early_draw).await?;
    assert_eq!(early_draw["data"]["series"]["status"], "COMPLETED");
    assert_eq!(early_draw["data"]["series"]["revision"], 11);
    assert_eq!(early_draw["data"]["confirmedGameCount"], 2);
    let result_notifications = series_result_notifications(&pool, &recreated_series_id).await?;
    assert_eq!(result_notifications.len(), 2);
    let (channel_id, _deduplication_key, payload) = result_notifications
        .iter()
        .find(|(_, key, _)| key == &format!("series:{recreated_series_id}:result:11"))
        .ok_or("early-draw result notification was not enqueued")?;
    assert_eq!(channel_id, RESULT_CHANNEL_ID);
    assert_eq!(payload["schemaVersion"], 2);
    assert_eq!(
        payload["snapshot"]["series"]["terminalReason"],
        "EARLY_DRAW"
    );
    assert!(payload["snapshot"]["series"]["winningTeam"].is_null());
    assert_eq!(
        payload["snapshot"]["score"],
        json!({ "team1": 1, "team2": 1 })
    );
    assert_eq!(
        payload["snapshot"]["teams"].as_array().map(Vec::len),
        Some(2)
    );
    let early_draw_games = payload["snapshot"]["games"]
        .as_array()
        .ok_or("early-draw result notification games are missing")?;
    assert_eq!(early_draw_games.len(), 2);
    assert_eq!(early_draw_games[0]["setNumber"], 1);
    assert_eq!(early_draw_games[0]["winner"], "TEAM_1");
    assert_eq!(
        early_draw_games[0]["team1"]["picks"]
            .as_array()
            .map(Vec::len),
        Some(5)
    );
    assert_eq!(
        early_draw_games[0]["team2"]["picks"]
            .as_array()
            .map(Vec::len),
        Some(5)
    );
    assert_eq!(
        early_draw_games[0]["bans"],
        json!({
            "blue": [null, null, null, null, null],
            "red": [null, null, null, null, null]
        })
    );
    assert_eq!(early_draw_games[1]["setNumber"], 2);
    assert_eq!(early_draw_games[1]["winner"], "TEAM_2");
    assert_eq!(early_draw_games[1]["sidePreset"], "DEFAULT");
    assert!(
        early_draw_games
            .iter()
            .all(|game| game.get("mmrChanges").is_none())
    );

    let tied_result = browser_get(&app, &recreated_result_path, MEMBER_SESSION).await?;
    assert_eq!(tied_result.status(), StatusCode::OK);
    let tied_result = body_json(tied_result).await?;
    assert!(tied_result["data"]["series"]["winningTeam"].is_null());
    assert_eq!(
        tied_result["data"]["series"]["terminalReason"],
        "EARLY_DRAW"
    );
    assert_eq!(
        tied_result["data"]["score"],
        json!({ "team1": 1, "team2": 1 })
    );
    assert_eq!(
        tied_result["data"]["games"].as_array().map(Vec::len),
        Some(2)
    );
    assert!(
        tied_result["data"]["games"]
            .as_array()
            .is_some_and(|games| {
                games.iter().all(|game| {
                    game["mmrChanges"]
                        .as_array()
                        .is_some_and(|changes| changes.len() == 10)
                })
            })
    );
    let tied_draw_ledger = sqlx::query_as::<_, (i64, i64)>(
        "SELECT
             (SELECT count(*) FROM games
                WHERE guild_id = $1 AND series_id = $2 AND status = 'CONFIRMED'),
             (SELECT count(*) FROM mmr_ledger_entries AS ledger
                JOIN games AS game ON game.guild_id = ledger.guild_id AND game.id = ledger.game_id
                WHERE ledger.guild_id = $1 AND game.series_id = $2
                  AND game.status = 'CONFIRMED' AND ledger.source_type = 'GAME_RESULT')",
    )
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(&recreated_series_id)?)
    .fetch_one(&pool)
    .await?;
    assert_eq!(tied_draw_ledger, (2, 20));

    let delete_path = format!("/api/guilds/{GUILD_ID}/series/{recreated_series_id}");
    let deleted = browser_mutation(
        &app,
        Method::DELETE,
        &delete_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-series-delete-1",
        true,
        json!({ "expectedRevision": 11 }),
        None,
    )
    .await?;
    assert_eq!(deleted.status(), StatusCode::OK);
    assert_eq!(
        body_json(deleted).await?["data"]["nextHref"],
        format!("/g/{GUILD_ID}/series")
    );
    let deleted_source_state = sqlx::query_as::<_, (String, i64, bool, bool)>(
        "SELECT status, revision, converted_at IS NULL, cancelled_at IS NOT NULL
         FROM recruitments
         WHERE guild_id = $1 AND id = $2",
    )
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(&recruitment_id)?)
    .fetch_one(&pool)
    .await?;
    assert_eq!(
        deleted_source_state,
        ("CANCELLED".to_owned(), 11, true, true)
    );

    pool.close().await;
    drop(server);
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
        MEMBER_ID,
        "Active Member",
        MEMBER_SESSION,
        MEMBER_CSRF,
    )
    .await?;
    create_login(
        &auth_repository,
        OUTSIDER_ID,
        "Outside Member",
        OUTSIDER_SESSION,
        OUTSIDER_CSRF,
    )
    .await?;
    create_login(
        &auth_repository,
        CANDIDATE_ID,
        "Eligible Candidate",
        CANDIDATE_SESSION,
        CANDIDATE_CSRF,
    )
    .await?;
    for (index, user_id) in FULL_ROSTER_MEMBER_IDS.iter().skip(2).enumerate() {
        let session = format!("web-recruitment-full-roster-{index}-session");
        let csrf = format!("web-recruitment-full-roster-{index}-csrf");
        create_login(
            &auth_repository,
            user_id,
            &format!("Full Roster Member {}", index + 3),
            &session,
            &csrf,
        )
        .await?;
    }
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
    let path = format!("/api/guilds/{GUILD_ID}/recruitments");

    let unauthenticated = app
        .clone()
        .oneshot(
            Request::post(&path)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))?,
        )
        .await?;
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);

    let missing_csrf = browser_create(
        &app,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "missing-csrf",
        false,
        None,
    )
    .await?;
    assert_eq!(missing_csrf.status(), StatusCode::FORBIDDEN);
    let missing_csrf = body_json(missing_csrf).await?;
    assert_eq!(missing_csrf["error"]["code"], "csrf_validation_failed");
    assert_eq!(
        missing_csrf["error"]["message"],
        "요청 출처를 확인할 수 없습니다."
    );

    let outsider = browser_create(
        &app,
        OUTSIDER_SESSION,
        OUTSIDER_CSRF,
        "outsider-create",
        true,
        None,
    )
    .await?;
    assert_eq!(outsider.status(), StatusCode::FORBIDDEN);
    let outsider = body_json(outsider).await?;
    assert_eq!(outsider["error"]["code"], "guild_membership_required");
    assert_eq!(
        outsider["error"]["message"],
        "해당 길드의 활성 멤버만 내전을 만들 수 있습니다."
    );

    let missing_channel = browser_create(
        &app,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "missing-recruitment-channel",
        true,
        None,
    )
    .await?;
    assert_eq!(missing_channel.status(), StatusCode::CONFLICT);
    assert_eq!(
        body_json(missing_channel).await?["error"]["code"],
        "recruitment_channel_not_configured"
    );

    sqlx::query(
        "UPDATE guild_config
         SET recruitment_channel_id = $1
         WHERE guild_id = $2",
    )
    .bind(RECRUITMENT_CHANNEL_ID)
    .bind(GUILD_ID)
    .execute(&pool)
    .await?;

    let invalid_best_of = browser_mutation(
        &app,
        Method::POST,
        &path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-recruitment-invalid-best-of",
        true,
        json!({ "bestOf": 2, "hardFearless": true }),
        None,
    )
    .await?;
    assert_eq!(invalid_best_of.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let invalid_best_of = body_json(invalid_best_of).await?;
    assert_eq!(
        invalid_best_of["error"]["code"],
        "invalid_recruitment_best_of"
    );
    assert_eq!(
        invalid_best_of["error"]["message"],
        "시리즈 형식은 BO1, BO3 또는 BO5만 선택할 수 있습니다."
    );
    assert_eq!(
        invalid_best_of["error"]["details"],
        json!({
            "field": "bestOf",
            "allowedValues": [1, 3, 5],
            "receivedValue": 2
        })
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM recruitments WHERE guild_id = $1")
            .bind(GUILD_ID)
            .fetch_one(&pool)
            .await?,
        0
    );

    let idempotency_key = "web-recruitment-create-1";
    let created = browser_create(
        &app,
        MEMBER_SESSION,
        MEMBER_CSRF,
        idempotency_key,
        true,
        None,
    )
    .await?;
    assert_eq!(created.status(), StatusCode::CREATED);
    let created = body_json(created).await?;
    let recruitment_id = created["data"]["recruitment"]["id"]
        .as_str()
        .ok_or("web create response did not contain a recruitment ID")?
        .to_owned();
    assert_eq!(created["data"]["recruitment"]["displayNumber"], 1);
    assert_eq!(created["data"]["recruitment"]["title"], "일반 내전 모집 #1");
    assert_eq!(created["data"]["recruitment"]["status"], "OPEN");
    assert_eq!(created["data"]["recruitment"]["targetCount"], 10);
    assert_eq!(created["data"]["recruitment"]["bestOf"], 3);
    assert_eq!(created["data"]["recruitment"]["hardFearless"], false);
    assert_eq!(created["data"]["recruitment"]["createdById"], MEMBER_ID);
    assert_eq!(created["data"]["participants"], json!([]));
    assert_eq!(
        created["data"]["links"]["entry"],
        format!("/g/{GUILD_ID}/recruitments/{recruitment_id}/entry")
    );

    let list = browser_get(&app, &format!("{path}?state=actionable"), MEMBER_SESSION).await?;
    assert_eq!(list.status(), StatusCode::OK);
    let list = body_json(list).await?;
    assert_eq!(
        list["data"]["recruitments"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(list["data"]["recruitments"][0]["id"], recruitment_id);
    assert_eq!(list["data"]["recruitments"][0]["status"], "OPEN");
    assert_eq!(
        list["data"]["recruitments"][0]["href"],
        format!("/g/{GUILD_ID}/recruitments/{recruitment_id}")
    );

    let detail = browser_get(
        &app,
        &format!("/api/guilds/{GUILD_ID}/recruitments/{recruitment_id}"),
        MEMBER_SESSION,
    )
    .await?;
    assert_eq!(detail.status(), StatusCode::OK);
    assert_eq!(body_json(detail).await?["data"], created["data"]);

    let replay = browser_mutation(
        &app,
        Method::POST,
        &path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        idempotency_key,
        true,
        json!({ "bestOf": 3, "hardFearless": false }),
        None,
    )
    .await?;
    assert_eq!(replay.status(), StatusCode::CREATED);
    assert_eq!(body_json(replay).await?["data"], created["data"]);
    assert_eq!(mutation_counts(&pool).await?, (1, 1, 0, 1, 1, 1, 1));
    assert_eq!(
        pending_notification_types(&pool, &recruitment_id).await?,
        vec!["RECRUITMENT_MESSAGE_CREATE"]
    );

    let claimed = bot_post(
        &app,
        "/internal/notifications/claim",
        json!({ "instanceId": BOT_INSTANCE_ID, "limit": 1 }),
    )
    .await?;
    assert_eq!(claimed.status(), StatusCode::OK);
    let claimed = body_json(claimed).await?;
    let notification = &claimed["data"]["notifications"][0];
    let bound = bot_post(
        &app,
        "/internal/notifications/recruitment-message/ack",
        json!({
            "instanceId": BOT_INSTANCE_ID,
            "notificationId": notification["id"],
            "leaseToken": notification["leaseToken"],
            "discordMessageId": MESSAGE_ID
        }),
    )
    .await?;
    assert_eq!(bound.status(), StatusCode::OK);
    let bound = body_json(bound).await?;
    assert_eq!(bound["data"]["recruitment"]["id"], recruitment_id);

    let member_path = format!("{path}/{recruitment_id}/members");
    let missing_member_csrf = browser_mutation(
        &app,
        Method::POST,
        &member_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "missing-member-csrf",
        false,
        json!({ "memberId": CANDIDATE_ID }),
        None,
    )
    .await?;
    assert_eq!(missing_member_csrf.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        body_json(missing_member_csrf).await?["error"]["code"],
        "csrf_validation_failed"
    );

    let non_operator_add = browser_mutation(
        &app,
        Method::POST,
        &member_path,
        CANDIDATE_SESSION,
        CANDIDATE_CSRF,
        "non-operator-add",
        true,
        json!({ "memberId": CANDIDATE_ID }),
        None,
    )
    .await?;
    assert_eq!(non_operator_add.status(), StatusCode::FORBIDDEN);
    let non_operator_add = body_json(non_operator_add).await?;
    assert_eq!(non_operator_add["error"]["code"], "operator_required");
    assert_eq!(
        non_operator_add["error"]["message"],
        "길드 운영자 권한이 필요합니다."
    );

    let add_member_key = "web-recruitment-add-member-1";
    let added = browser_mutation(
        &app,
        Method::POST,
        &member_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        add_member_key,
        true,
        json!({ "memberId": CANDIDATE_ID }),
        None,
    )
    .await?;
    assert_eq!(added.status(), StatusCode::OK);
    let added = body_json(added).await?;
    assert_eq!(added["data"]["recruitment"]["status"], "OPEN");
    assert_eq!(
        added["data"]["participants"],
        json!([{
            "id": CANDIDATE_ID,
            "displayName": "Eligible Candidate",
            "preferredLanes": []
        }])
    );
    assert_eq!(
        pending_notification_types(&pool, &recruitment_id).await?,
        vec!["RECRUITMENT_MESSAGE_UPDATE"]
    );

    let add_replay = browser_mutation(
        &app,
        Method::POST,
        &member_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        add_member_key,
        true,
        json!({ "memberId": CANDIDATE_ID }),
        None,
    )
    .await?;
    assert_eq!(add_replay.status(), StatusCode::OK);
    assert_eq!(body_json(add_replay).await?["data"], added["data"]);

    let remove_member_path = format!("{member_path}/{CANDIDATE_ID}");
    let non_operator_remove = browser_mutation(
        &app,
        Method::DELETE,
        &remove_member_path,
        CANDIDATE_SESSION,
        CANDIDATE_CSRF,
        "non-operator-remove",
        true,
        json!({}),
        None,
    )
    .await?;
    assert_eq!(non_operator_remove.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        body_json(non_operator_remove).await?["error"]["code"],
        "operator_required"
    );

    let remove_member_key = "web-recruitment-remove-member-1";
    let removed = browser_mutation(
        &app,
        Method::DELETE,
        &remove_member_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        remove_member_key,
        true,
        json!({}),
        None,
    )
    .await?;
    assert_eq!(removed.status(), StatusCode::OK);
    let removed = body_json(removed).await?;
    assert_eq!(removed["data"]["recruitment"]["status"], "OPEN");
    assert_eq!(removed["data"]["participants"], json!([]));
    assert_eq!(
        pending_notification_types(&pool, &recruitment_id).await?,
        vec!["RECRUITMENT_MESSAGE_UPDATE", "RECRUITMENT_MESSAGE_UPDATE"]
    );

    let remove_replay = browser_mutation(
        &app,
        Method::DELETE,
        &remove_member_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        remove_member_key,
        true,
        json!({}),
        None,
    )
    .await?;
    assert_eq!(remove_replay.status(), StatusCode::OK);
    assert_eq!(body_json(remove_replay).await?["data"], removed["data"]);

    let cancel_path = format!("{path}/{recruitment_id}");
    let non_operator_cancel = browser_mutation(
        &app,
        Method::DELETE,
        &cancel_path,
        CANDIDATE_SESSION,
        CANDIDATE_CSRF,
        "non-operator-cancel",
        true,
        json!({}),
        None,
    )
    .await?;
    assert_eq!(non_operator_cancel.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        body_json(non_operator_cancel).await?["error"]["code"],
        "operator_required"
    );

    let cancel_key = "web-recruitment-cancel-1";
    let cancelled = browser_mutation(
        &app,
        Method::DELETE,
        &cancel_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        cancel_key,
        true,
        json!({}),
        None,
    )
    .await?;
    assert_eq!(cancelled.status(), StatusCode::OK);
    let cancelled = body_json(cancelled).await?;
    assert_eq!(cancelled["data"]["recruitment"]["status"], "CANCELLED");
    assert_eq!(
        cancelled["data"]["participants"],
        removed["data"]["participants"]
    );
    assert_eq!(
        pending_notification_types(&pool, &recruitment_id).await?,
        vec![
            "RECRUITMENT_MESSAGE_DELETE",
            "RECRUITMENT_MESSAGE_UPDATE",
            "RECRUITMENT_MESSAGE_UPDATE"
        ]
    );

    let cancel_replay = browser_mutation(
        &app,
        Method::DELETE,
        &cancel_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        cancel_key,
        true,
        json!({}),
        None,
    )
    .await?;
    assert_eq!(cancel_replay.status(), StatusCode::OK);
    assert_eq!(body_json(cancel_replay).await?["data"], cancelled["data"]);

    let actionable_after_cancel =
        browser_get(&app, &format!("{path}?state=actionable"), MEMBER_SESSION).await?;
    assert_eq!(actionable_after_cancel.status(), StatusCode::OK);
    assert_eq!(
        body_json(actionable_after_cancel).await?["data"]["recruitments"],
        json!([])
    );

    let detail_after_cancel = browser_get(&app, &cancel_path, MEMBER_SESSION).await?;
    assert_eq!(detail_after_cancel.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        body_json(detail_after_cancel).await?["error"]["code"],
        "not_found"
    );
    assert_eq!(
        web_mutation_counts(&pool, &recruitment_id).await?,
        (0, 3, 3)
    );

    let full_created = browser_mutation(
        &app,
        Method::POST,
        &path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-recruitment-close-create",
        true,
        json!({ "bestOf": 5, "hardFearless": true }),
        None,
    )
    .await?;
    assert_eq!(full_created.status(), StatusCode::CREATED);
    let full_created = body_json(full_created).await?;
    let full_recruitment_id = full_created["data"]["recruitment"]["id"]
        .as_str()
        .ok_or("full recruitment create response did not contain a recruitment ID")?
        .to_owned();
    assert_eq!(full_created["data"]["recruitment"]["bestOf"], 5);
    assert_eq!(full_created["data"]["recruitment"]["hardFearless"], true);
    mark_recruitment_message_bound(&pool, &full_recruitment_id).await?;
    seed_recruitment_participants(&pool, &full_recruitment_id, &FULL_ROSTER_MEMBER_IDS[..9])
        .await?;

    let close_path = format!("{path}/{full_recruitment_id}/close");
    let non_operator_close = browser_mutation(
        &app,
        Method::POST,
        &close_path,
        CANDIDATE_SESSION,
        CANDIDATE_CSRF,
        "web-recruitment-close-non-operator",
        true,
        json!({}),
        None,
    )
    .await?;
    assert_eq!(non_operator_close.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        body_json(non_operator_close).await?["error"]["code"],
        "operator_required"
    );

    let incomplete_close = browser_mutation(
        &app,
        Method::POST,
        &close_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-recruitment-close-incomplete",
        true,
        json!({}),
        None,
    )
    .await?;
    assert_eq!(incomplete_close.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body_json(incomplete_close).await?["error"]["code"],
        "recruitment_not_full"
    );

    seed_recruitment_participants(&pool, &full_recruitment_id, &FULL_ROSTER_MEMBER_IDS[9..])
        .await?;
    let close_key = "web-recruitment-close-1";
    let closed = browser_mutation(
        &app,
        Method::POST,
        &close_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        close_key,
        true,
        json!({}),
        None,
    )
    .await?;
    assert_eq!(closed.status(), StatusCode::OK);
    let closed = body_json(closed).await?;
    assert_eq!(closed["data"]["recruitment"]["status"], "CLOSED");
    assert_eq!(
        closed["data"]["participants"].as_array().map(Vec::len),
        Some(FULL_ROSTER_MEMBER_IDS.len())
    );
    assert_eq!(
        closed["data"]["links"]["entry"],
        format!("/g/{GUILD_ID}/recruitments/{full_recruitment_id}/entry")
    );
    assert_eq!(
        pending_notification_types(&pool, &full_recruitment_id).await?,
        vec!["RECRUITMENT_MESSAGE_UPDATE"]
    );
    assert_eq!(
        web_close_mutation_counts(&pool, &full_recruitment_id).await?,
        (1, 1)
    );

    let close_replay = browser_mutation(
        &app,
        Method::POST,
        &close_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        close_key,
        true,
        json!({}),
        None,
    )
    .await?;
    assert_eq!(close_replay.status(), StatusCode::OK);
    assert_eq!(body_json(close_replay).await?["data"], closed["data"]);
    assert_eq!(
        web_close_mutation_counts(&pool, &full_recruitment_id).await?,
        (1, 1)
    );

    seed_entry_draft_riot_tier(&pool).await?;
    let entry_path = format!("{path}/{full_recruitment_id}/entry-draft");
    let entry_snapshot = browser_get(&app, &entry_path, MEMBER_SESSION).await?;
    assert_eq!(entry_snapshot.status(), StatusCode::OK);
    let entry_snapshot = body_json(entry_snapshot).await?;
    assert_eq!(entry_snapshot["data"]["recruitment"]["status"], "CLOSED");
    assert_eq!(
        entry_snapshot["data"]["draft"]["participants"]
            .as_array()
            .map(Vec::len),
        Some(FULL_ROSTER_MEMBER_IDS.len())
    );
    assert_eq!(entry_snapshot["data"]["capabilities"]["canEdit"], true);
    assert_eq!(entry_snapshot["data"]["capabilities"]["canReopen"], true);
    assert_eq!(entry_snapshot["data"]["capabilities"]["canCancel"], true);
    let tiered_participant = entry_snapshot["data"]["draft"]["participants"]
        .as_array()
        .and_then(|participants| {
            participants
                .iter()
                .find(|participant| participant["id"] == MEMBER_ID)
        })
        .ok_or("entry draft response did not contain the tiered participant")?;
    assert_eq!(tiered_participant["riotTier"], "P3");
    let unlinked_participant = entry_snapshot["data"]["draft"]["participants"]
        .as_array()
        .and_then(|participants| {
            participants
                .iter()
                .find(|participant| participant["id"] == CANDIDATE_ID)
        })
        .ok_or("entry draft response did not contain the unlinked participant")?;
    assert_eq!(unlinked_participant["riotTier"], Value::Null);
    let initial_draft_revision = entry_snapshot["data"]["draft"]["revision"]
        .as_i64()
        .ok_or("entry draft response did not contain a revision")?;
    let recruitment_revision = entry_snapshot["data"]["recruitment"]["revision"]
        .as_i64()
        .ok_or("entry draft response did not contain a recruitment revision")?;

    let non_operator_entry = browser_get(&app, &entry_path, CANDIDATE_SESSION).await?;
    assert_eq!(non_operator_entry.status(), StatusCode::OK);
    let non_operator_entry = body_json(non_operator_entry).await?;
    assert_eq!(non_operator_entry["data"]["capabilities"]["canEdit"], false);

    let non_operator_move = browser_mutation(
        &app,
        Method::PATCH,
        &entry_path,
        CANDIDATE_SESSION,
        CANDIDATE_CSRF,
        "web-entry-non-operator-move",
        true,
        json!({
            "expectedRevision": initial_draft_revision,
            "operation": {
                "type": "MOVE_PARTICIPANT",
                "participantId": MEMBER_ID,
                "slot": "TEAM_1_TOP"
            }
        }),
        None,
    )
    .await?;
    assert_eq!(non_operator_move.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        body_json(non_operator_move).await?["error"]["code"],
        "operator_required"
    );

    let invalid_slot = browser_mutation(
        &app,
        Method::PATCH,
        &entry_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-entry-invalid-slot",
        true,
        json!({
            "expectedRevision": initial_draft_revision,
            "operation": {
                "type": "MOVE_PARTICIPANT",
                "participantId": MEMBER_ID,
                "slot": "TEAM_3_TOP"
            }
        }),
        None,
    )
    .await?;
    assert_eq!(invalid_slot.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body_json(invalid_slot).await?["error"]["code"],
        "invalid_entry_slot"
    );

    let unknown_operation_field = browser_mutation(
        &app,
        Method::PATCH,
        &entry_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-entry-unknown-operation-field",
        true,
        json!({
            "expectedRevision": initial_draft_revision,
            "operation": {
                "type": "MOVE_PARTICIPANT",
                "participantId": MEMBER_ID,
                "slot": "TEAM_1_TOP",
                "unexpectedField": true
            }
        }),
        None,
    )
    .await?;
    assert_eq!(
        unknown_operation_field.status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );

    let first_move = browser_mutation(
        &app,
        Method::PATCH,
        &entry_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-entry-first-move",
        true,
        json!({
            "expectedRevision": initial_draft_revision,
            "operation": {
                "type": "MOVE_PARTICIPANT",
                "participantId": MEMBER_ID,
                "slot": "TEAM_1_TOP"
            }
        }),
        None,
    )
    .await?;
    assert_eq!(first_move.status(), StatusCode::OK);
    let first_move = body_json(first_move).await?;
    let first_move_revision = first_move["data"]["draft"]["revision"]
        .as_i64()
        .ok_or("first entry move did not return a revision")?;
    assert_eq!(first_move_revision, initial_draft_revision + 1);
    assert_eq!(
        outbox_topic_for_event(
            &pool,
            &full_recruitment_id,
            "RECRUITMENT_ENTRY_DRAFT",
            "RECRUITMENT_ENTRY_DRAFT_UPDATED"
        )
        .await?,
        format!("guild:{GUILD_ID}:recruitment-entry-draft:{full_recruitment_id}")
    );
    assert!(
        first_move["data"]["draft"]["assignments"]
            .as_array()
            .is_some_and(|assignments| {
                assignments.iter().any(|assignment| {
                    assignment["participantId"] == MEMBER_ID
                        && assignment["team"] == "TEAM_1"
                        && assignment["lane"] == "TOP"
                })
            })
    );

    let second_move = browser_mutation(
        &app,
        Method::PATCH,
        &entry_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-entry-second-move",
        true,
        json!({
            "expectedRevision": first_move_revision,
            "operation": {
                "type": "MOVE_PARTICIPANT",
                "participantId": CANDIDATE_ID,
                "slot": "TEAM_2_JUNGLE"
            }
        }),
        None,
    )
    .await?;
    assert_eq!(second_move.status(), StatusCode::OK);
    let second_move = body_json(second_move).await?;
    let second_move_revision = second_move["data"]["draft"]["revision"]
        .as_i64()
        .ok_or("second entry move did not return a revision")?;

    let swap_move = browser_mutation(
        &app,
        Method::PATCH,
        &entry_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-entry-swap-move",
        true,
        json!({
            "expectedRevision": second_move_revision,
            "operation": {
                "type": "MOVE_PARTICIPANT",
                "participantId": CANDIDATE_ID,
                "slot": "TEAM_1_TOP"
            }
        }),
        None,
    )
    .await?;
    assert_eq!(swap_move.status(), StatusCode::OK);
    let swap_move = body_json(swap_move).await?;
    let swap_move_revision = swap_move["data"]["draft"]["revision"]
        .as_i64()
        .ok_or("entry swap did not return a revision")?;
    assert!(
        swap_move["data"]["draft"]["assignments"]
            .as_array()
            .is_some_and(|assignments| {
                assignments.iter().any(|assignment| {
                    assignment["participantId"] == CANDIDATE_ID
                        && assignment["team"] == "TEAM_1"
                        && assignment["lane"] == "TOP"
                }) && assignments.iter().any(|assignment| {
                    assignment["participantId"] == MEMBER_ID
                        && assignment["team"] == "TEAM_2"
                        && assignment["lane"] == "JUNGLE"
                })
            })
    );

    let stale_reset = browser_mutation(
        &app,
        Method::POST,
        &format!("{entry_path}/reset"),
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-entry-stale-reset",
        true,
        json!({ "expectedRevision": second_move_revision }),
        None,
    )
    .await?;
    assert_eq!(stale_reset.status(), StatusCode::CONFLICT);
    assert_eq!(
        body_json(stale_reset).await?["error"]["code"],
        "entry_draft_revision_conflict"
    );

    let reset = browser_mutation(
        &app,
        Method::POST,
        &format!("{entry_path}/reset"),
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-entry-reset",
        true,
        json!({ "expectedRevision": swap_move_revision }),
        None,
    )
    .await?;
    assert_eq!(reset.status(), StatusCode::OK);
    let reset = body_json(reset).await?;
    let reset_revision = reset["data"]["draft"]["revision"]
        .as_i64()
        .ok_or("entry reset did not return a revision")?;
    assert_eq!(reset["data"]["draft"]["assignments"], json!([]));

    let reopen_path = format!("{path}/{full_recruitment_id}/reopen");
    let reopened = browser_mutation(
        &app,
        Method::POST,
        &reopen_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-entry-reopen",
        true,
        json!({ "expectedRevision": recruitment_revision }),
        None,
    )
    .await?;
    assert_eq!(reopened.status(), StatusCode::OK);
    let reopened = body_json(reopened).await?;
    assert_eq!(reopened["data"]["recruitment"]["status"], "OPEN");
    assert_eq!(
        reopened["data"]["draft"]["revision"],
        json!(reset_revision + 1)
    );
    assert_eq!(reopened["data"]["capabilities"]["canEdit"], false);
    assert_eq!(
        outbox_topic_for_event(
            &pool,
            &full_recruitment_id,
            "RECRUITMENT",
            "RECRUITMENT_REOPENED"
        )
        .await?,
        format!("guild:{GUILD_ID}:recruitment:{full_recruitment_id}")
    );
    assert_eq!(
        pending_notification_types(&pool, &full_recruitment_id).await?,
        vec!["RECRUITMENT_MESSAGE_UPDATE", "RECRUITMENT_MESSAGE_UPDATE"]
    );

    let closed_again = browser_mutation(
        &app,
        Method::POST,
        &close_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-recruitment-close-after-reopen",
        true,
        json!({}),
        None,
    )
    .await?;
    assert_eq!(closed_again.status(), StatusCode::OK);
    assert_eq!(
        body_json(closed_again).await?["data"]["recruitment"]["status"],
        "CLOSED"
    );

    let closed_entry = browser_get(&app, &entry_path, MEMBER_SESSION).await?;
    assert_eq!(closed_entry.status(), StatusCode::OK);
    assert_eq!(
        body_json(closed_entry).await?["data"]["capabilities"]["canCancel"],
        true
    );

    let closed_cancel_path = format!("{path}/{full_recruitment_id}");
    let closed_cancel = browser_mutation(
        &app,
        Method::DELETE,
        &closed_cancel_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-recruitment-cancel-closed",
        true,
        json!({}),
        None,
    )
    .await?;
    assert_eq!(closed_cancel.status(), StatusCode::OK);
    let closed_cancel = body_json(closed_cancel).await?;
    assert_eq!(closed_cancel["data"]["recruitment"]["status"], "CANCELLED");

    let closed_cancel_replay = browser_mutation(
        &app,
        Method::DELETE,
        &closed_cancel_path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "web-recruitment-cancel-closed",
        true,
        json!({}),
        None,
    )
    .await?;
    assert_eq!(closed_cancel_replay.status(), StatusCode::OK);
    assert_eq!(
        body_json(closed_cancel_replay).await?["data"],
        closed_cancel["data"]
    );

    let closed_entry_after_cancel = browser_get(&app, &entry_path, MEMBER_SESSION).await?;
    assert_eq!(closed_entry_after_cancel.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        body_json(closed_entry_after_cancel).await?["error"]["code"],
        "not_found"
    );

    let malformed_bearer = browser_create(
        &app,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "would-have-been-valid-session-request",
        true,
        Some("Bearer not-the-bot-service-token"),
    )
    .await?;
    assert_eq!(malformed_bearer.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        body_json(malformed_bearer).await?["error"]["code"],
        "unauthorized"
    );

    pool.close().await;
    drop(server);
    Ok(())
}

async fn browser_create(
    app: &Router,
    session: &str,
    csrf: &str,
    idempotency_key: &str,
    include_csrf: bool,
    authorization: Option<&str>,
) -> TestResult<Response<Body>> {
    browser_mutation(
        app,
        Method::POST,
        &format!("/api/guilds/{GUILD_ID}/recruitments"),
        session,
        csrf,
        idempotency_key,
        include_csrf,
        json!({}),
        authorization,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn browser_mutation(
    app: &Router,
    method: Method,
    path: &str,
    session: &str,
    csrf: &str,
    idempotency_key: &str,
    include_csrf: bool,
    body: Value,
    authorization: Option<&str>,
) -> TestResult<Response<Body>> {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(
            header::COOKIE,
            format!("monkeybot_session={session}; monkeybot_csrf={csrf}"),
        )
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, PUBLIC_ORIGIN)
        .header("idempotency-key", idempotency_key);
    if include_csrf {
        request = request.header("x-csrf-token", csrf);
    }
    if let Some(authorization) = authorization {
        request = request.header(header::AUTHORIZATION, authorization);
    }
    Ok(app
        .clone()
        .oneshot(request.body(Body::from(serde_json::to_vec(&body)?))?)
        .await?)
}

async fn browser_get(app: &Router, path: &str, session: &str) -> TestResult<Response<Body>> {
    Ok(app
        .clone()
        .oneshot(
            Request::get(path)
                .header(header::COOKIE, format!("monkeybot_session={session}"))
                .body(Body::empty())?,
        )
        .await?)
}

async fn bot_post(app: &Router, path: &str, body: Value) -> TestResult<Response<Body>> {
    Ok(app
        .clone()
        .oneshot(
            Request::post(path)
                .header(header::AUTHORIZATION, format!("Bearer {BOT_TOKEN}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&body)?))?,
        )
        .await?)
}

async fn mutation_counts(pool: &PgPool) -> TestResult<(i64, i64, i64, i64, i64, i64, i64)> {
    Ok(sqlx::query_as::<_, (i64, i64, i64, i64, i64, i64, i64)>(
        "SELECT
             (SELECT count(*) FROM recruitments WHERE guild_id = $1),
             (SELECT count(*) FROM recruitment_entry_drafts WHERE guild_id = $1),
             (SELECT count(*) FROM series WHERE guild_id = $1),
             (SELECT count(*) FROM admin_audit_log
                WHERE guild_id = $1 AND action = 'RECRUITMENT_CREATE'),
             (SELECT count(*) FROM outbox_events
                WHERE guild_id = $1 AND event_type = 'RECRUITMENT_CREATED'),
             (SELECT count(*) FROM idempotency_records
                WHERE guild_id = $1
                  AND principal_type = 'USER'
                  AND principal_id = $2
                  AND route_template = '/api/guilds/{guildId}/recruitments'),
             (SELECT count(*) FROM bot_notification_outbox
                WHERE guild_id = $1 AND notification_type = 'RECRUITMENT_MESSAGE_CREATE')",
    )
    .bind(GUILD_ID)
    .bind(MEMBER_ID)
    .fetch_one(pool)
    .await?)
}

async fn series_pick_ban_draft_side_effects(
    pool: &PgPool,
    series_id: Uuid,
) -> TestResult<(i64, i64)> {
    Ok(sqlx::query_as::<_, (i64, i64)>(
        "SELECT
             (SELECT count(*) FROM admin_audit_log
                WHERE guild_id = $1
                  AND target_type = 'SERIES_PICKBAN_DRAFT'
                  AND target_id = $2),
             (SELECT count(*) FROM outbox_events
                WHERE guild_id = $1
                  AND aggregate_type = 'SERIES'
                  AND aggregate_id = $2
                  AND event_type = 'SERIES_PICKBAN_UPDATED')",
    )
    .bind(GUILD_ID)
    .bind(series_id.to_string())
    .fetch_one(pool)
    .await?)
}

async fn pending_notification_types(
    pool: &PgPool,
    recruitment_id: &str,
) -> TestResult<Vec<String>> {
    Ok(sqlx::query_scalar::<_, String>(
        "SELECT notification_type
         FROM bot_notification_outbox
         WHERE guild_id = $1
           AND payload ->> 'recruitmentId' = $2
           AND delivered_at IS NULL
           AND failed_at IS NULL
         ORDER BY notification_type",
    )
    .bind(GUILD_ID)
    .bind(recruitment_id)
    .fetch_all(pool)
    .await?)
}

async fn series_result_notifications(
    pool: &PgPool,
    series_id: &str,
) -> TestResult<Vec<(String, String, Value)>> {
    Ok(sqlx::query_as::<_, (String, String, Value)>(
        "SELECT channel_id, deduplication_key, payload
         FROM bot_notification_outbox
         WHERE guild_id = $1
           AND notification_type = 'SERIES_RESULT_CREATE'
           AND payload ->> 'seriesId' = $2
         ORDER BY deduplication_key",
    )
    .bind(GUILD_ID)
    .bind(series_id)
    .fetch_all(pool)
    .await?)
}

async fn set_entry_channel(pool: &PgPool, channel_id: Option<&str>) -> TestResult {
    sqlx::query(
        "UPDATE guild_config
         SET entry_channel_id = $1
         WHERE guild_id = $2",
    )
    .bind(channel_id)
    .bind(GUILD_ID)
    .execute(pool)
    .await?;
    Ok(())
}

async fn set_result_channel(pool: &PgPool, channel_id: Option<&str>) -> TestResult {
    sqlx::query(
        "UPDATE guild_config
         SET result_channel_id = $1
         WHERE guild_id = $2",
    )
    .bind(channel_id)
    .bind(GUILD_ID)
    .execute(pool)
    .await?;
    Ok(())
}

async fn web_mutation_counts(pool: &PgPool, recruitment_id: &str) -> TestResult<(i64, i64, i64)> {
    Ok(sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT
             (SELECT count(*) FROM recruitment_participants
                WHERE guild_id = $1 AND recruitment_id = $2 AND user_id = $3 AND status = 'JOINED'),
             (SELECT count(*) FROM admin_audit_log
                WHERE guild_id = $1
                  AND action IN (
                      'RECRUITMENT_MEMBER_ADDED',
                      'RECRUITMENT_MEMBER_REMOVED',
                      'RECRUITMENT_CANCELLED'
                  )),
             (SELECT count(*) FROM outbox_events
                WHERE guild_id = $1
                  AND event_type IN (
                      'RECRUITMENT_MEMBER_ADDED',
                      'RECRUITMENT_MEMBER_REMOVED',
                      'RECRUITMENT_CANCELLED'
                  ))",
    )
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(recruitment_id)?)
    .bind(CANDIDATE_ID)
    .fetch_one(pool)
    .await?)
}

async fn web_close_mutation_counts(pool: &PgPool, recruitment_id: &str) -> TestResult<(i64, i64)> {
    Ok(sqlx::query_as::<_, (i64, i64)>(
        "SELECT
             (SELECT count(*) FROM admin_audit_log
                WHERE guild_id = $1
                  AND target_id = $2
                  AND action = 'RECRUITMENT_CLOSE'),
             (SELECT count(*) FROM outbox_events
                WHERE guild_id = $1
                  AND aggregate_id = $2
                  AND event_type = 'RECRUITMENT_CLOSED')",
    )
    .bind(GUILD_ID)
    .bind(recruitment_id)
    .fetch_one(pool)
    .await?)
}

async fn outbox_topic_for_event(
    pool: &PgPool,
    recruitment_id: &str,
    aggregate_type: &str,
    event_type: &str,
) -> TestResult<String> {
    Ok(sqlx::query_scalar::<_, String>(
        "SELECT topic
         FROM outbox_events
         WHERE guild_id = $1
           AND aggregate_id = $2
           AND aggregate_type = $3
           AND event_type = $4
         ORDER BY occurred_at DESC, id DESC
         LIMIT 1",
    )
    .bind(GUILD_ID)
    .bind(recruitment_id)
    .bind(aggregate_type)
    .bind(event_type)
    .fetch_one(pool)
    .await?)
}

async fn mark_recruitment_message_bound(pool: &PgPool, recruitment_id: &str) -> TestResult {
    let now = OffsetDateTime::now_utc();
    sqlx::query(
        "UPDATE recruitments
         SET channel_id = $1, message_id = $2
         WHERE guild_id = $3 AND id = $4",
    )
    .bind(RECRUITMENT_CHANNEL_ID)
    .bind(MESSAGE_ID)
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(recruitment_id)?)
    .execute(pool)
    .await?;
    sqlx::query(
        "UPDATE bot_notification_outbox
         SET delivered_at = $1,
             discord_message_id = $2,
             lease_owner = NULL,
             lease_token = NULL,
             lease_until = NULL
         WHERE guild_id = $3
           AND notification_type = 'RECRUITMENT_MESSAGE_CREATE'
           AND payload ->> 'recruitmentId' = $4",
    )
    .bind(now)
    .bind(MESSAGE_ID)
    .bind(GUILD_ID)
    .bind(recruitment_id)
    .execute(pool)
    .await?;
    Ok(())
}

async fn seed_recruitment_participants(
    pool: &PgPool,
    recruitment_id: &str,
    user_ids: &[&str],
) -> TestResult {
    let now = OffsetDateTime::now_utc();
    for user_id in user_ids {
        sqlx::query(
            "INSERT INTO recruitment_participants (
                 guild_id, recruitment_id, user_id, status, joined_at, withdrawn_at
             ) VALUES ($1, $2, $3, 'JOINED', $4, NULL)",
        )
        .bind(GUILD_ID)
        .bind(Uuid::parse_str(recruitment_id)?)
        .bind(user_id)
        .bind(now)
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn seed_closed_entry_recruitment(
    pool: &PgPool,
    best_of: i16,
    hard_fearless: bool,
) -> TestResult<String> {
    let now = OffsetDateTime::now_utc();
    let recruitment_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO recruitments (
             guild_id, id, season_id, display_number, title, target_count, best_of, hard_fearless, status,
             revision, channel_id, message_id, created_by, created_at, updated_by, updated_at,
             closed_at, converted_at, cancelled_at
         ) VALUES (
             $1, $2, $3, 1, '일반 내전 모집 #1', 10, $4, $5, 'CLOSED',
             7, $6, $7, $8, $9, $8, $9,
             $9, NULL, NULL
         )",
    )
    .bind(GUILD_ID)
    .bind(recruitment_id)
    .bind(Uuid::parse_str(SEASON_ID)?)
    .bind(best_of)
    .bind(hard_fearless)
    .bind(RECRUITMENT_CHANNEL_ID)
    .bind(MESSAGE_ID)
    .bind(MEMBER_ID)
    .bind(now)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO recruitment_entry_drafts (
             guild_id, recruitment_id, revision, updated_by, updated_at
         ) VALUES ($1, $2, 3, $3, $4)",
    )
    .bind(GUILD_ID)
    .bind(recruitment_id)
    .bind(MEMBER_ID)
    .bind(now)
    .execute(pool)
    .await?;
    seed_recruitment_participants(pool, &recruitment_id.to_string(), &FULL_ROSTER_MEMBER_IDS)
        .await?;
    Ok(recruitment_id.to_string())
}

async fn seed_complete_entry_assignments(pool: &PgPool, recruitment_id: &str) -> TestResult {
    const SLOTS: [(&str, &str); 10] = [
        ("TEAM_1", "TOP"),
        ("TEAM_1", "JUNGLE"),
        ("TEAM_1", "MID"),
        ("TEAM_1", "BOTTOM"),
        ("TEAM_1", "SUPPORT"),
        ("TEAM_2", "TOP"),
        ("TEAM_2", "JUNGLE"),
        ("TEAM_2", "MID"),
        ("TEAM_2", "BOTTOM"),
        ("TEAM_2", "SUPPORT"),
    ];
    let recruitment_id = Uuid::parse_str(recruitment_id)?;
    for (user_id, (team, lane)) in FULL_ROSTER_MEMBER_IDS.iter().zip(SLOTS) {
        sqlx::query(
            "INSERT INTO recruitment_entry_assignments (
                 guild_id, recruitment_id, user_id, team, lane
             ) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(GUILD_ID)
        .bind(recruitment_id)
        .bind(user_id)
        .bind(team)
        .bind(lane)
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn seed_entry_draft_riot_tier(pool: &PgPool) -> TestResult {
    let now = OffsetDateTime::now_utc();
    let puuid = "entry-draft-tier-puuid";
    sqlx::query(
        "INSERT INTO riot_accounts (
             puuid, user_id, platform, routing_region,
             game_name, tag_line, normalized_game_name, normalized_tag_line,
             profile_icon_id, is_main, main_position, main_position_updated_at,
             last_synced_at, created_at, updated_at, origin
         ) VALUES (
             $1, $2, 'KR', 'ASIA',
             'Tiered Player', 'KR1', 'tiered player', 'kr1',
             29, true, NULL, NULL,
             $3, $3, $3, 'RIOT'
         )",
    )
    .bind(puuid)
    .bind(MEMBER_ID)
    .bind(now)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO riot_records_snapshots (
             platform, puuid, routing_region, game_name, tag_line,
             normalized_game_name, normalized_tag_line,
             schema_version, snapshot, refreshed_at, created_at, updated_at
         ) VALUES (
             'KR', $1, 'ASIA', 'Tiered Player', 'KR1',
             'tiered player', 'kr1',
             1,
             jsonb_build_object(
                 'ranks', jsonb_build_array(
                     jsonb_build_object(
                         'queueType', 'RANKED_SOLO_5x5',
                         'tier', 'PLATINUM',
                         'rank', 'III'
                     ),
                     jsonb_build_object(
                         'queueType', 'RANKED_FLEX_SR',
                         'tier', 'GOLD',
                         'rank', 'II'
                     )
                 )
             ),
             $2, $2, $2
         )",
    )
    .bind(puuid)
    .bind(now)
    .execute(pool)
    .await?;
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
    sqlx::query(
        "INSERT INTO guilds (
             guild_id, discord_name, icon_hash, status, installed_by,
             installed_at, activated_at, left_at, updated_at
         ) VALUES ($1, 'Web Create Guild', NULL, 'ACTIVE', $2, $3, $3, NULL, $3)",
    )
    .bind(GUILD_ID)
    .bind(MEMBER_ID)
    .bind(now)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO guild_members (
             guild_id, user_id, display_name, status, main_lane, first_seen_at,
             registered_at, discord_joined_at, last_active_at, last_synced_at, inactivated_at
         ) VALUES ($1, $2, 'Eligible Candidate', 'ACTIVE', NULL, $3, $3, $3, $3, $3, NULL)",
    )
    .bind(GUILD_ID)
    .bind(CANDIDATE_ID)
    .bind(now)
    .execute(pool)
    .await?;
    for (index, user_id) in FULL_ROSTER_MEMBER_IDS.iter().skip(2).enumerate() {
        let display_name = format!("Full Roster Member {}", index + 3);
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
        "INSERT INTO guild_members (
             guild_id, user_id, display_name, status, main_lane, first_seen_at,
             registered_at, discord_joined_at, last_active_at, last_synced_at, inactivated_at
         ) VALUES ($1, $2, 'Active Member', 'ACTIVE', NULL, $3, $3, $3, $3, $3, NULL)",
    )
    .bind(GUILD_ID)
    .bind(MEMBER_ID)
    .bind(now)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO guild_member_discord_roles (guild_id, user_id, role_id, last_synced_at)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(GUILD_ID)
    .bind(MEMBER_ID)
    .bind(OPERATOR_ROLE_ID)
    .bind(now)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO guild_config (
             guild_id, display_name, operator_role_id, recruitment_channel_id,
             entry_channel_id, result_channel_id, log_channel_id, is_externally_public, revision,
             updated_by, updated_at
         ) VALUES ($1, 'Web Create Guild', $2, NULL, $3, NULL, NULL, false, 0, $4, $5)",
    )
    .bind(GUILD_ID)
    .bind(OPERATOR_ROLE_ID)
    .bind(ENTRY_CHANNEL_ID)
    .bind(MEMBER_ID)
    .bind(now)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO seasons (
             guild_id, id, name, status, started_at, ended_at, created_by, created_at
         ) VALUES ($1, $2, '시즌 1', 'ACTIVE', $3, NULL, $4, $3)",
    )
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(SEASON_ID)?)
    .bind(now)
    .bind(MEMBER_ID)
    .execute(pool)
    .await?;
    for counter_key in [
        "RECRUITMENT",
        "SERIES",
        "AUCTION_RECRUITMENT",
        "AUCTION_TOURNAMENT",
    ] {
        sqlx::query(
            "INSERT INTO guild_counters (guild_id, counter_key, next_value)
             VALUES ($1, $2, 1)",
        )
        .bind(GUILD_ID)
        .bind(counter_key)
        .execute(pool)
        .await?;
    }
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
            "MONKEYBOT_BOT_SERVICE_TOKEN".to_owned(),
            BOT_TOKEN.to_owned(),
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
