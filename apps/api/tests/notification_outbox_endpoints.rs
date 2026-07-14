use std::{collections::HashMap, error::Error, sync::Arc, time::Duration};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, Response, StatusCode, header},
};
use monkeybot_api::{
    AppState, build_router,
    config::Config,
    domain::{DiscordChannelId, GuildId, NotificationId, RecruitmentId},
    infrastructure::postgres::{MIGRATOR, PostgresReadinessProbe},
    repository::notification_outbox::{
        self, EnqueueNotification, RECRUITMENT_MESSAGE_CREATE_TYPE,
        RECRUITMENT_MESSAGE_DELETE_TYPE, RECRUITMENT_MESSAGE_SCHEMA_VERSION,
        RECRUITMENT_MESSAGE_UPDATE_TYPE,
    },
};
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use testcontainers::{
    ContainerAsync, GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use time::{Duration as TimeDuration, OffsetDateTime};
use tower::ServiceExt;
use uuid::Uuid;

const POSTGRES_IMAGE: &str = "postgres";
const POSTGRES_TAG: &str = "18.4-alpine3.23";
const POSTGRES_PASSWORD: &str = "monkeybot-notification-outbox-password";
const DATABASE: &str = "monkeybot_notification_outbox";
const BOT_TOKEN: &str = "bot-service-token-that-is-at-least-32-characters";
const GUILD_ID: &str = "200000000000000001";
const CREATOR_ID: &str = "100000000000000001";
const CHANNEL_ID: &str = "400000000000000001";
const MESSAGE_ID: &str = "500000000000000001";
const RECRUITMENT_ID: &str = "01900000-0000-7000-8000-000000000001";
const ORDERED_RECRUITMENT_ID: &str = "01900000-0000-7000-8000-000000000003";
const SEASON_ID: &str = "01900000-0000-7000-8000-000000000002";
const ORDERED_MESSAGE_ID: &str = "500000000000000002";
const PUBLIC_ORIGIN: &str = "http://localhost:5173";

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn notification_queue_claims_retries_and_binds_recruitment_messages_atomically() -> TestResult
{
    tokio::time::timeout(Duration::from_secs(120), run_flow())
        .await
        .map_err(|_| "notification outbox endpoint test exceeded 120 seconds")??;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn recruitment_message_notifications_are_claimed_in_delivery_order() -> TestResult {
    tokio::time::timeout(Duration::from_secs(120), run_delivery_order_flow())
        .await
        .map_err(|_| "notification delivery order test exceeded 120 seconds")??;
    Ok(())
}

async fn run_flow() -> TestResult {
    let server = TestPostgres::start().await?;
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&server.database_url())
        .await?;
    MIGRATOR.run(&pool).await?;
    seed_recruitment(&pool).await?;

    let app = build_router(AppState::with_postgres(
        test_config(server.database_url())?,
        pool.clone(),
        Arc::new(PostgresReadinessProbe::new(
            pool.clone(),
            Duration::from_secs(1),
        )),
    ));

    let unauthenticated = app
        .clone()
        .oneshot(
            Request::post("/internal/notifications/claim")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))?,
        )
        .await?;
    assert_error(
        unauthenticated,
        StatusCode::UNAUTHORIZED,
        "unauthorized",
        "인증이 필요합니다.",
    )
    .await?;

    let recruitment_notification_id = NotificationId::new();
    enqueue_recruitment_message(&pool, recruitment_notification_id).await?;

    let claimed = bot_post(
        &app,
        "/internal/notifications/claim",
        json!({ "instanceId": "queue-test-1", "limit": 5 }),
    )
    .await?;
    assert_eq!(claimed.status(), StatusCode::OK);
    let claimed = body_json(claimed).await?;
    let notification = claimed["data"]["notifications"]
        .as_array()
        .and_then(|notifications| notifications.first())
        .ok_or("claim response did not contain the recruitment notification")?;
    assert_eq!(notification["id"], recruitment_notification_id.to_string());
    assert_eq!(notification["type"], RECRUITMENT_MESSAGE_CREATE_TYPE);
    assert_eq!(notification["guildId"], GUILD_ID);
    assert_eq!(notification["channelId"], CHANNEL_ID);
    assert_eq!(notification["attempts"], 1);
    assert_eq!(
        notification["discordNonce"].as_str().map(str::len),
        Some(22)
    );
    let lease_token = notification["leaseToken"]
        .as_str()
        .ok_or("claim response did not contain leaseToken")?;

    let generic_create_ack = bot_post(
        &app,
        "/internal/notifications/ack",
        json!({
            "instanceId": "queue-test-1",
            "notificationId": recruitment_notification_id,
            "leaseToken": lease_token,
            "discordMessageId": MESSAGE_ID
        }),
    )
    .await?;
    assert_error(
        generic_create_ack,
        StatusCode::UNPROCESSABLE_ENTITY,
        "recruitment_message_create_requires_typed_ack",
        "모집 메시지 생성 알림은 전용 확인 경로로 처리해야 합니다.",
    )
    .await?;
    let still_leased = sqlx::query_as::<_, (bool, Option<String>, bool)>(
        "SELECT delivered_at IS NULL, lease_owner, lease_token IS NOT NULL
         FROM bot_notification_outbox
         WHERE id = $1",
    )
    .bind(recruitment_notification_id.as_uuid())
    .fetch_one(&pool)
    .await?;
    assert_eq!(still_leased, (true, Some("queue-test-1".to_owned()), true));

    let wrong_owner = bot_post(
        &app,
        "/internal/notifications/recruitment-message/ack",
        json!({
            "instanceId": "queue-test-2",
            "notificationId": recruitment_notification_id,
            "leaseToken": lease_token,
            "discordMessageId": MESSAGE_ID
        }),
    )
    .await?;
    assert_error(
        wrong_owner,
        StatusCode::CONFLICT,
        "notification_lease_conflict",
        "알림 전송 lease가 만료되었거나 다른 Bot 인스턴스로 이전되었습니다.",
    )
    .await?;

    let acknowledged = bot_post(
        &app,
        "/internal/notifications/recruitment-message/ack",
        json!({
            "instanceId": "queue-test-1",
            "notificationId": recruitment_notification_id,
            "leaseToken": lease_token,
            "discordMessageId": MESSAGE_ID
        }),
    )
    .await?;
    assert_eq!(acknowledged.status(), StatusCode::OK);
    let acknowledged = body_json(acknowledged).await?;
    assert_eq!(acknowledged["data"]["acknowledged"], true);
    assert_eq!(acknowledged["data"]["recruitment"]["id"], RECRUITMENT_ID);
    assert_eq!(acknowledged["data"]["recruitment"]["revision"], 1);

    let delivered = sqlx::query_as::<_, (Option<String>, bool)>(
        "SELECT discord_message_id, delivered_at IS NOT NULL
         FROM bot_notification_outbox
         WHERE id = $1",
    )
    .bind(recruitment_notification_id.as_uuid())
    .fetch_one(&pool)
    .await?;
    assert_eq!(delivered, (Some(MESSAGE_ID.to_owned()), true));
    let bound = sqlx::query_as::<_, (Option<String>, Option<String>, i64)>(
        "SELECT channel_id, message_id, revision
         FROM recruitments
         WHERE guild_id = $1 AND id = $2",
    )
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(RECRUITMENT_ID)?)
    .fetch_one(&pool)
    .await?;
    assert_eq!(
        bound,
        (Some(CHANNEL_ID.to_owned()), Some(MESSAGE_ID.to_owned()), 1)
    );

    let retry_notification_id = NotificationId::new();
    enqueue_generic_notification(&pool, retry_notification_id).await?;
    let retry_claim = bot_post(
        &app,
        "/internal/notifications/claim",
        json!({ "instanceId": "queue-test-1" }),
    )
    .await?;
    let retry_claim = body_json(retry_claim).await?;
    let retry = retry_claim["data"]["notifications"]
        .as_array()
        .and_then(|notifications| notifications.first())
        .ok_or("claim response did not contain retry notification")?;
    assert_eq!(retry["id"], retry_notification_id.to_string());
    let retry_lease_token = retry["leaseToken"]
        .as_str()
        .ok_or("retry claim response did not contain leaseToken")?;

    let nacked = bot_post(
        &app,
        "/internal/notifications/nack",
        json!({
            "instanceId": "queue-test-1",
            "notificationId": retry_notification_id,
            "leaseToken": retry_lease_token,
            "failure": {
                "kind": "TRANSPORT",
                "message": "Discord request timed out"
            }
        }),
    )
    .await?;
    assert_eq!(nacked.status(), StatusCode::OK);
    let nacked = body_json(nacked).await?;
    assert_eq!(nacked["data"]["state"], "RETRY_SCHEDULED");
    assert_eq!(nacked["data"]["attempts"], 1);
    assert!(nacked["data"]["availableAt"].is_string());

    let retry_state = sqlx::query_as::<_, (i32, Option<String>, Option<Uuid>, Option<String>)>(
        "SELECT attempts, lease_owner, lease_token, failure_code
         FROM bot_notification_outbox
         WHERE id = $1",
    )
    .bind(retry_notification_id.as_uuid())
    .fetch_one(&pool)
    .await?;
    assert_eq!(retry_state, (1, None, None, None));

    pool.close().await;
    drop(server);
    Ok(())
}

async fn run_delivery_order_flow() -> TestResult {
    let server = TestPostgres::start().await?;
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&server.database_url())
        .await?;
    MIGRATOR.run(&pool).await?;
    seed_recruitment(&pool).await?;
    seed_ordered_recruitment(&pool).await?;

    let app = build_router(AppState::with_postgres(
        test_config(server.database_url())?,
        pool.clone(),
        Arc::new(PostgresReadinessProbe::new(
            pool.clone(),
            Duration::from_secs(1),
        )),
    ));

    let create_id = NotificationId::new();
    let update_id = NotificationId::new();
    let delete_id = NotificationId::new();
    let created_at = OffsetDateTime::now_utc() - TimeDuration::seconds(10);
    enqueue_ordered_recruitment_message(
        &pool,
        create_id,
        RECRUITMENT_MESSAGE_CREATE_TYPE,
        "recruitment:ordered:create",
        created_at,
    )
    .await?;
    enqueue_ordered_recruitment_message(
        &pool,
        update_id,
        RECRUITMENT_MESSAGE_UPDATE_TYPE,
        "recruitment:ordered:update",
        created_at + TimeDuration::seconds(1),
    )
    .await?;
    enqueue_ordered_recruitment_message(
        &pool,
        delete_id,
        RECRUITMENT_MESSAGE_DELETE_TYPE,
        "recruitment:ordered:delete",
        created_at + TimeDuration::seconds(2),
    )
    .await?;

    let first = claim_one(&app, "queue-order-1").await?;
    assert_eq!(first["id"], create_id.to_string());
    assert_eq!(first["type"], RECRUITMENT_MESSAGE_CREATE_TYPE);

    let while_create_is_leased = bot_post(
        &app,
        "/internal/notifications/claim",
        json!({ "instanceId": "queue-order-2", "limit": 10 }),
    )
    .await?;
    assert_eq!(while_create_is_leased.status(), StatusCode::OK);
    assert!(
        body_json(while_create_is_leased).await?["data"]["notifications"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );

    acknowledge_recruitment_message(&app, "queue-order-1", create_id, &first).await?;

    let second = claim_one(&app, "queue-order-2").await?;
    assert_eq!(second["id"], update_id.to_string());
    assert_eq!(second["type"], RECRUITMENT_MESSAGE_UPDATE_TYPE);
    acknowledge_generic_notification(&app, "queue-order-2", update_id, &second).await?;

    let third = claim_one(&app, "queue-order-3").await?;
    assert_eq!(third["id"], delete_id.to_string());
    assert_eq!(third["type"], RECRUITMENT_MESSAGE_DELETE_TYPE);
    acknowledge_generic_notification(&app, "queue-order-3", delete_id, &third).await?;

    let delivered = sqlx::query_as::<_, (Uuid, bool)>(
        "SELECT id, delivered_at IS NOT NULL
         FROM bot_notification_outbox
         WHERE id = ANY($1)
         ORDER BY created_at, id",
    )
    .bind(vec![
        create_id.as_uuid(),
        update_id.as_uuid(),
        delete_id.as_uuid(),
    ])
    .fetch_all(&pool)
    .await?;
    assert_eq!(
        delivered,
        vec![
            (create_id.as_uuid(), true),
            (update_id.as_uuid(), true),
            (delete_id.as_uuid(), true),
        ]
    );

    pool.close().await;
    drop(server);
    Ok(())
}

async fn claim_one(app: &Router, instance_id: &str) -> TestResult<Value> {
    let response = bot_post(
        app,
        "/internal/notifications/claim",
        json!({ "instanceId": instance_id, "limit": 10 }),
    )
    .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await?;
    let notifications = body["data"]["notifications"]
        .as_array()
        .ok_or("claim response did not contain a notification array")?;
    assert_eq!(notifications.len(), 1);
    Ok(notifications[0].clone())
}

async fn acknowledge_recruitment_message(
    app: &Router,
    instance_id: &str,
    notification_id: NotificationId,
    claimed: &Value,
) -> TestResult {
    let lease_token = claimed["leaseToken"]
        .as_str()
        .ok_or("claim response did not contain leaseToken")?;
    let response = bot_post(
        app,
        "/internal/notifications/recruitment-message/ack",
        json!({
            "instanceId": instance_id,
            "notificationId": notification_id,
            "leaseToken": lease_token,
            "discordMessageId": ORDERED_MESSAGE_ID
        }),
    )
    .await?;
    assert_eq!(response.status(), StatusCode::OK);
    Ok(())
}

async fn acknowledge_generic_notification(
    app: &Router,
    instance_id: &str,
    notification_id: NotificationId,
    claimed: &Value,
) -> TestResult {
    let lease_token = claimed["leaseToken"]
        .as_str()
        .ok_or("claim response did not contain leaseToken")?;
    let response = bot_post(
        app,
        "/internal/notifications/ack",
        json!({
            "instanceId": instance_id,
            "notificationId": notification_id,
            "leaseToken": lease_token,
            "discordMessageId": ORDERED_MESSAGE_ID
        }),
    )
    .await?;
    assert_eq!(response.status(), StatusCode::OK);
    Ok(())
}

async fn enqueue_recruitment_message(pool: &PgPool, id: NotificationId) -> TestResult {
    let guild_id = GuildId::parse(GUILD_ID)?;
    let channel_id = DiscordChannelId::parse(CHANNEL_ID)?;
    let recruitment_id = RecruitmentId::from_uuid(Uuid::parse_str(RECRUITMENT_ID)?);
    let payload = json!({
        "schemaVersion": RECRUITMENT_MESSAGE_SCHEMA_VERSION,
        "recruitmentId": recruitment_id,
        "snapshot": {
            "recruitment": {
                "id": recruitment_id,
                "status": "OPEN",
                "revision": 0
            }
        }
    });
    let mut transaction = pool.begin().await?;
    notification_outbox::enqueue(
        &mut transaction,
        &EnqueueNotification {
            id,
            guild_id: &guild_id,
            deduplication_key: "recruitment:initial-message",
            channel_id: &channel_id,
            notification_type: RECRUITMENT_MESSAGE_CREATE_TYPE,
            payload: &payload,
            created_at: OffsetDateTime::now_utc(),
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(())
}

async fn enqueue_ordered_recruitment_message(
    pool: &PgPool,
    id: NotificationId,
    notification_type: &str,
    deduplication_key: &str,
    created_at: OffsetDateTime,
) -> TestResult {
    let guild_id = GuildId::parse(GUILD_ID)?;
    let channel_id = DiscordChannelId::parse(CHANNEL_ID)?;
    let recruitment_id = RecruitmentId::from_uuid(Uuid::parse_str(ORDERED_RECRUITMENT_ID)?);
    let revision = if notification_type == RECRUITMENT_MESSAGE_CREATE_TYPE {
        0
    } else {
        1
    };
    let payload = json!({
        "schemaVersion": RECRUITMENT_MESSAGE_SCHEMA_VERSION,
        "recruitmentId": recruitment_id,
        "snapshot": {
            "recruitment": {
                "id": recruitment_id,
                "status": "OPEN",
                "revision": revision
            }
        }
    });
    let mut transaction = pool.begin().await?;
    notification_outbox::enqueue(
        &mut transaction,
        &EnqueueNotification {
            id,
            guild_id: &guild_id,
            deduplication_key,
            channel_id: &channel_id,
            notification_type,
            payload: &payload,
            created_at,
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(())
}

async fn enqueue_generic_notification(pool: &PgPool, id: NotificationId) -> TestResult {
    let guild_id = GuildId::parse(GUILD_ID)?;
    let channel_id = DiscordChannelId::parse(CHANNEL_ID)?;
    let payload = json!({ "message": "generic notification" });
    let mut transaction = pool.begin().await?;
    notification_outbox::enqueue(
        &mut transaction,
        &EnqueueNotification {
            id,
            guild_id: &guild_id,
            deduplication_key: "generic:retry-message",
            channel_id: &channel_id,
            notification_type: "GENERIC_TEST_NOTIFICATION",
            payload: &payload,
            created_at: OffsetDateTime::now_utc(),
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(())
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

async fn seed_recruitment(pool: &PgPool) -> TestResult {
    let now = OffsetDateTime::now_utc();
    sqlx::query(
        "INSERT INTO users (user_id, display_name, avatar_hash, created_at, updated_at, deleted_at)
         VALUES ($1, 'Recruitment Creator', NULL, $2, $2, NULL)",
    )
    .bind(CREATOR_ID)
    .bind(now)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO guilds (
             guild_id, discord_name, icon_hash, status, installed_by,
             installed_at, activated_at, left_at, updated_at
         ) VALUES ($1, 'Queue Guild', NULL, 'ACTIVE', $2, $3, $3, NULL, $3)",
    )
    .bind(GUILD_ID)
    .bind(CREATOR_ID)
    .bind(now)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO guild_members (
             guild_id, user_id, display_name, status, main_lane, first_seen_at,
             registered_at, discord_joined_at, last_active_at, last_synced_at, inactivated_at
         ) VALUES ($1, $2, 'Recruitment Creator', 'ACTIVE', NULL, $3, $3, $3, $3, $3, NULL)",
    )
    .bind(GUILD_ID)
    .bind(CREATOR_ID)
    .bind(now)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO seasons (
             guild_id, id, name, status, started_at, ended_at, created_by, created_at
         ) VALUES ($1, $2, 'Season 1', 'ACTIVE', $3, NULL, $4, $3)",
    )
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(SEASON_ID)?)
    .bind(now)
    .bind(CREATOR_ID)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO recruitments (
             guild_id, id, season_id, display_number, title, target_count, status,
             revision, channel_id, message_id, created_by, created_at, updated_by, updated_at
         ) VALUES ($1, $2, $3, 1, 'Queue Recruitment', 10, 'OPEN',
                   0, NULL, NULL, $4, $5, $4, $5)",
    )
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(RECRUITMENT_ID)?)
    .bind(Uuid::parse_str(SEASON_ID)?)
    .bind(CREATOR_ID)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

async fn seed_ordered_recruitment(pool: &PgPool) -> TestResult {
    let now = OffsetDateTime::now_utc();
    sqlx::query(
        "INSERT INTO recruitments (
             guild_id, id, season_id, display_number, title, target_count, status,
             revision, channel_id, message_id, created_by, created_at, updated_by, updated_at
         ) VALUES ($1, $2, $3, 2, 'Ordered Queue Recruitment', 10, 'OPEN',
                   0, NULL, NULL, $4, $5, $4, $5)",
    )
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(ORDERED_RECRUITMENT_ID)?)
    .bind(Uuid::parse_str(SEASON_ID)?)
    .bind(CREATOR_ID)
    .bind(now)
    .execute(pool)
    .await?;
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
