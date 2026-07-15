use std::time::{Duration, Instant};

use axum::{
    Extension, Json, Router,
    extract::State,
    http::StatusCode,
    response::Response,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    domain::{DiscordMessageId, NotificationId},
    error::{self, ApiError},
    http::extractors::{AuthenticatedSession, BotServicePrincipal},
    infrastructure::postgres::begin,
    repository::notification_outbox::{
        self, AckNotification, ClaimNotifications, NackNotification, NackNotificationOutcome,
        NotificationFailure, NotificationFailureKind, NotificationOutboxError,
    },
    repository::service_status::{
        self, BotHeartbeatStatus, RecordBotHeartbeat, ServiceStatusRepositoryError,
    },
    state::{AppState, ComponentStatus, DependencySnapshot, RequestId},
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClaimNotificationsRequest {
    instance_id: String,
    limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AckNotificationRequest {
    instance_id: String,
    notification_id: NotificationId,
    lease_token: Uuid,
    discord_message_id: DiscordMessageId,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NackNotificationRequest {
    instance_id: String,
    notification_id: NotificationId,
    lease_token: Uuid,
    failure: NotificationFailureRequest,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HeartbeatRequest {
    instance_id: String,
    version: String,
    commit_sha: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NotificationFailureRequest {
    kind: NotificationFailureKindRequest,
    status: Option<u16>,
    retry_after_ms: Option<u64>,
    message: String,
}

#[derive(Debug, Deserialize)]
enum NotificationFailureKindRequest {
    #[serde(rename = "HTTP")]
    Http,
    #[serde(rename = "TRANSPORT")]
    Transport,
    #[serde(rename = "DELIVERY_UNKNOWN")]
    DeliveryUnknown,
}

impl From<NotificationFailureKindRequest> for NotificationFailureKind {
    fn from(value: NotificationFailureKindRequest) -> Self {
        match value {
            NotificationFailureKindRequest::Http => Self::Http,
            NotificationFailureKindRequest::Transport => Self::Transport,
            NotificationFailureKindRequest::DeliveryUnknown => Self::DeliveryUnknown,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClaimNotificationsData {
    notifications: Vec<ClaimedNotificationData>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClaimedNotificationData {
    id: NotificationId,
    guild_id: crate::domain::GuildId,
    lease_token: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    lease_until: OffsetDateTime,
    channel_id: crate::domain::DiscordChannelId,
    #[serde(rename = "type")]
    notification_type: String,
    payload: Value,
    discord_nonce: String,
    attempts: i32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AckNotificationData {
    acknowledged: bool,
    #[serde(with = "time::serde::rfc3339")]
    delivered_at: OffsetDateTime,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HeartbeatData {
    accepted: bool,
    #[serde(with = "time::serde::rfc3339")]
    last_heartbeat_at: OffsetDateTime,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceStatusData {
    #[serde(with = "time::serde::rfc3339")]
    checked_at: OffsetDateTime,
    services: Vec<ServiceStatusItem>,
}

#[derive(Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum ServiceStatusKey {
    Api,
    Postgresql,
    BotHeartbeat,
    Valkey,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum ServiceHealth {
    Normal,
    Delayed,
    Down,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceStatusItem {
    key: ServiceStatusKey,
    status: ServiceHealth,
    latency_ms: Option<u64>,
    value: String,
    #[serde(with = "time::serde::rfc3339::option")]
    last_healthy_at: Option<OffsetDateTime>,
    issue: Option<&'static str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecruitmentMessageAckData {
    acknowledged: bool,
    #[serde(with = "time::serde::rfc3339")]
    delivered_at: OffsetDateTime,
    recruitment: RecruitmentBindingData,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecruitmentBindingData {
    id: crate::domain::RecruitmentId,
    revision: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AuctionRecruitmentMessageAckData {
    acknowledged: bool,
    #[serde(with = "time::serde::rfc3339")]
    delivered_at: OffsetDateTime,
    auction_recruitment: AuctionRecruitmentBindingData,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AuctionRecruitmentBindingData {
    id: crate::domain::AuctionRecruitmentId,
    revision: i64,
}

#[derive(Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
enum NackNotificationData {
    #[serde(rename = "RETRY_SCHEDULED")]
    RetryScheduled {
        #[serde(rename = "availableAt", with = "time::serde::rfc3339")]
        available_at: OffsetDateTime,
        attempts: i32,
    },
    #[serde(rename = "FAILED")]
    Failed {
        #[serde(rename = "failureCode")]
        failure_code: &'static str,
        attempts: i32,
    },
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/internal/heartbeat", post(heartbeat))
        .route("/api/admin/status", get(service_status))
        .route("/internal/notifications/claim", post(claim))
        .route("/internal/notifications/ack", post(ack))
        .route(
            "/internal/notifications/recruitment-message/ack",
            post(ack_recruitment_message),
        )
        .route(
            "/internal/notifications/auction-recruitment-message/ack",
            post(ack_auction_recruitment_message),
        )
        .route("/internal/notifications/nack", post(nack))
}

pub async fn heartbeat(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    _principal: BotServicePrincipal,
    Json(request): Json<HeartbeatRequest>,
) -> Result<Response, ApiError> {
    let heartbeat = service_status::record_heartbeat(
        database(&state, &request_id)?,
        &RecordBotHeartbeat {
            instance_id: &request.instance_id,
            version: &request.version,
            commit_sha: request.commit_sha.as_deref(),
            recorded_at: OffsetDateTime::now_utc(),
        },
    )
    .await
    .map_err(|source| service_status_repository_error(source, &request_id))?;

    Ok(error::success(
        StatusCode::OK,
        &request_id,
        HeartbeatData {
            accepted: true,
            last_heartbeat_at: heartbeat.last_heartbeat_at,
        },
    ))
}

pub async fn service_status(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    principal: AuthenticatedSession,
) -> Result<Response, ApiError> {
    require_service_admin(&state, &principal, &request_id)?;

    let probe_started_at = Instant::now();
    let readiness = state.readiness().report().await;
    let probe_latency_ms =
        u64::try_from(probe_started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
    let checked_at = OffsetDateTime::now_utc();
    let heartbeat = service_status::latest_heartbeat(database(&state, &request_id)?)
        .await
        .map_err(|source| service_status_repository_error(source, &request_id))?;

    Ok(error::success(
        StatusCode::OK,
        &request_id,
        ServiceStatusData {
            checked_at,
            services: vec![
                api_service_status(readiness.accepting_requests, checked_at),
                postgres_service_status(&readiness.dependencies, probe_latency_ms, checked_at),
                bot_service_status(
                    heartbeat.as_ref().map(|value| value.last_heartbeat_at),
                    checked_at,
                ),
                valkey_service_status(&readiness.dependencies, probe_latency_ms, checked_at),
            ],
        },
    ))
}

fn require_service_admin(
    state: &AppState,
    principal: &AuthenticatedSession,
    request_id: &RequestId,
) -> Result<(), ApiError> {
    let is_service_admin = state
        .config()
        .auth()
        .is_some_and(|auth| auth.is_service_admin(&principal.session.discord_id));
    if is_service_admin {
        return Ok(());
    }

    Err(ApiError::new(
        StatusCode::FORBIDDEN,
        request_id,
        "service_admin_required",
        "서비스 관리자 권한이 필요합니다.",
        None,
    ))
}

fn api_service_status(accepting_requests: bool, checked_at: OffsetDateTime) -> ServiceStatusItem {
    if accepting_requests {
        ServiceStatusItem {
            key: ServiceStatusKey::Api,
            status: ServiceHealth::Normal,
            latency_ms: None,
            value: "요청 수신 가능".to_owned(),
            last_healthy_at: Some(checked_at),
            issue: None,
        }
    } else {
        ServiceStatusItem {
            key: ServiceStatusKey::Api,
            status: ServiceHealth::Down,
            latency_ms: None,
            value: "요청 수신 중지".to_owned(),
            last_healthy_at: None,
            issue: Some("API가 종료 중이거나 요청을 받지 않습니다."),
        }
    }
}

fn postgres_service_status(
    dependencies: &DependencySnapshot,
    latency_ms: u64,
    checked_at: OffsetDateTime,
) -> ServiceStatusItem {
    match (&dependencies.postgres, &dependencies.migration) {
        (ComponentStatus::Available, ComponentStatus::Available) => ServiceStatusItem {
            key: ServiceStatusKey::Postgresql,
            status: ServiceHealth::Normal,
            latency_ms: Some(latency_ms),
            value: "연결 정상".to_owned(),
            last_healthy_at: Some(checked_at),
            issue: None,
        },
        (ComponentStatus::Available, _) => ServiceStatusItem {
            key: ServiceStatusKey::Postgresql,
            status: ServiceHealth::Delayed,
            latency_ms: Some(latency_ms),
            value: "migration 확인 필요".to_owned(),
            last_healthy_at: None,
            issue: Some("PostgreSQL migration 상태를 확인할 수 없습니다."),
        },
        (ComponentStatus::Unavailable, _) => ServiceStatusItem {
            key: ServiceStatusKey::Postgresql,
            status: ServiceHealth::Down,
            latency_ms: None,
            value: "연결 불가".to_owned(),
            last_healthy_at: None,
            issue: Some("PostgreSQL 연결을 확인할 수 없습니다."),
        },
        (ComponentStatus::NotConfigured, _) => ServiceStatusItem {
            key: ServiceStatusKey::Postgresql,
            status: ServiceHealth::Down,
            latency_ms: None,
            value: "미구성".to_owned(),
            last_healthy_at: None,
            issue: Some("PostgreSQL이 구성되지 않았습니다."),
        },
    }
}

fn bot_service_status(
    last_heartbeat_at: Option<OffsetDateTime>,
    checked_at: OffsetDateTime,
) -> ServiceStatusItem {
    let status = service_status::classify_heartbeat(last_heartbeat_at, checked_at);
    let (status, issue) = match status {
        BotHeartbeatStatus::Normal => (ServiceHealth::Normal, None),
        BotHeartbeatStatus::Delayed => (
            ServiceHealth::Delayed,
            Some("마지막 heartbeat가 정상 기준 90초를 초과했습니다."),
        ),
        BotHeartbeatStatus::Down => (
            ServiceHealth::Down,
            Some("마지막 heartbeat가 5분을 초과했거나 아직 수신되지 않았습니다."),
        ),
    };

    ServiceStatusItem {
        key: ServiceStatusKey::BotHeartbeat,
        status,
        latency_ms: None,
        value: last_heartbeat_at.map_or_else(
            || "heartbeat 없음".to_owned(),
            |last_heartbeat_at| format_heartbeat_age(checked_at - last_heartbeat_at),
        ),
        // A persisted heartbeat is itself the most recent confirmed healthy Bot observation,
        // even after it has become stale for the current status classification.
        last_healthy_at: last_heartbeat_at,
        issue,
    }
}

fn valkey_service_status(
    dependencies: &DependencySnapshot,
    latency_ms: u64,
    checked_at: OffsetDateTime,
) -> ServiceStatusItem {
    match dependencies.valkey {
        ComponentStatus::Available => ServiceStatusItem {
            key: ServiceStatusKey::Valkey,
            status: ServiceHealth::Normal,
            latency_ms: Some(latency_ms),
            value: "연결 정상".to_owned(),
            last_healthy_at: Some(checked_at),
            issue: None,
        },
        ComponentStatus::Unavailable => ServiceStatusItem {
            key: ServiceStatusKey::Valkey,
            status: ServiceHealth::Down,
            latency_ms: None,
            value: "연결 불가".to_owned(),
            last_healthy_at: None,
            issue: Some("Valkey 연결을 확인할 수 없습니다."),
        },
        ComponentStatus::NotConfigured => ServiceStatusItem {
            key: ServiceStatusKey::Valkey,
            status: ServiceHealth::Delayed,
            latency_ms: None,
            value: "미구성".to_owned(),
            last_healthy_at: None,
            issue: Some("Valkey가 구성되지 않았습니다."),
        },
    }
}

fn format_heartbeat_age(age: time::Duration) -> String {
    let seconds = age.whole_seconds().max(0);
    let minutes = seconds / 60;
    let remaining_seconds = seconds % 60;
    if minutes == 0 {
        format!("{seconds}초 전")
    } else if remaining_seconds == 0 {
        format!("{minutes}분 전")
    } else {
        format!("{minutes}분 {remaining_seconds}초 전")
    }
}

fn service_status_repository_error(
    source: ServiceStatusRepositoryError,
    request_id: &RequestId,
) -> ApiError {
    match source {
        ServiceStatusRepositoryError::InvalidHeartbeat => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "invalid_heartbeat",
            "Bot heartbeat 요청 값이 올바르지 않습니다.",
            None,
        ),
        ServiceStatusRepositoryError::Database(database_error) => {
            tracing::error!(%database_error, "service status database operation failed");
            error::database_failure(&database_error, request_id)
        }
    }
}

pub async fn claim(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    _principal: BotServicePrincipal,
    Json(request): Json<ClaimNotificationsRequest>,
) -> Result<Response, ApiError> {
    let mut transaction = begin(database(&state, &request_id)?)
        .await
        .map_err(|source| error::database_failure(&source, &request_id))?;
    let notifications = notification_outbox::claim(
        &mut transaction,
        &ClaimNotifications {
            lease_owner: &request.instance_id,
            lease_duration: notification_outbox::DEFAULT_LEASE_DURATION,
            limit: request
                .limit
                .unwrap_or(notification_outbox::DEFAULT_CLAIM_LIMIT),
        },
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    transaction
        .commit()
        .await
        .map_err(|source| error::database_failure(&source, &request_id))?;

    let notifications = notifications
        .into_iter()
        .map(|notification| ClaimedNotificationData {
            id: notification.id,
            guild_id: notification.guild_id,
            lease_token: notification.lease.token,
            lease_until: notification.lease.until,
            channel_id: notification.channel_id,
            notification_type: notification.notification_type,
            payload: notification.payload,
            discord_nonce: notification.discord_nonce,
            attempts: notification.attempts,
        })
        .collect();
    Ok(error::success(
        StatusCode::OK,
        &request_id,
        ClaimNotificationsData { notifications },
    ))
}

pub async fn ack(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    _principal: BotServicePrincipal,
    Json(request): Json<AckNotificationRequest>,
) -> Result<Response, ApiError> {
    let mut transaction = begin(database(&state, &request_id)?)
        .await
        .map_err(|source| error::database_failure(&source, &request_id))?;
    let notification = notification_outbox::ack(
        &mut transaction,
        &AckNotification {
            notification_id: request.notification_id,
            lease_owner: &request.instance_id,
            lease_token: request.lease_token,
            discord_message_id: &request.discord_message_id,
        },
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    transaction
        .commit()
        .await
        .map_err(|source| error::database_failure(&source, &request_id))?;

    Ok(error::success(
        StatusCode::OK,
        &request_id,
        AckNotificationData {
            acknowledged: true,
            delivered_at: notification.delivered_at,
        },
    ))
}

pub async fn ack_recruitment_message(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    _principal: BotServicePrincipal,
    Json(request): Json<AckNotificationRequest>,
) -> Result<Response, ApiError> {
    let mut transaction = begin(database(&state, &request_id)?)
        .await
        .map_err(|source| error::database_failure(&source, &request_id))?;
    let acknowledgement = notification_outbox::ack_recruitment_message(
        &mut transaction,
        &AckNotification {
            notification_id: request.notification_id,
            lease_owner: &request.instance_id,
            lease_token: request.lease_token,
            discord_message_id: &request.discord_message_id,
        },
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    transaction
        .commit()
        .await
        .map_err(|source| error::database_failure(&source, &request_id))?;

    Ok(error::success(
        StatusCode::OK,
        &request_id,
        RecruitmentMessageAckData {
            acknowledged: true,
            delivered_at: acknowledgement.notification.delivered_at,
            recruitment: RecruitmentBindingData {
                id: acknowledgement.recruitment_id,
                revision: acknowledgement.recruitment_revision,
            },
        },
    ))
}

pub async fn ack_auction_recruitment_message(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    _principal: BotServicePrincipal,
    Json(request): Json<AckNotificationRequest>,
) -> Result<Response, ApiError> {
    let mut transaction = begin(database(&state, &request_id)?)
        .await
        .map_err(|source| error::database_failure(&source, &request_id))?;
    let acknowledgement = notification_outbox::ack_auction_recruitment_message(
        &mut transaction,
        &AckNotification {
            notification_id: request.notification_id,
            lease_owner: &request.instance_id,
            lease_token: request.lease_token,
            discord_message_id: &request.discord_message_id,
        },
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    transaction
        .commit()
        .await
        .map_err(|source| error::database_failure(&source, &request_id))?;

    Ok(error::success(
        StatusCode::OK,
        &request_id,
        AuctionRecruitmentMessageAckData {
            acknowledged: true,
            delivered_at: acknowledgement.notification.delivered_at,
            auction_recruitment: AuctionRecruitmentBindingData {
                id: acknowledgement.auction_recruitment_id,
                revision: acknowledgement.auction_recruitment_revision,
            },
        },
    ))
}

pub async fn nack(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    _principal: BotServicePrincipal,
    Json(request): Json<NackNotificationRequest>,
) -> Result<Response, ApiError> {
    let failure = NotificationFailure {
        kind: request.failure.kind.into(),
        status: request.failure.status,
        retry_after: request.failure.retry_after_ms.map(Duration::from_millis),
        message: &request.failure.message,
    };
    let mut transaction = begin(database(&state, &request_id)?)
        .await
        .map_err(|source| error::database_failure(&source, &request_id))?;
    let outcome = notification_outbox::nack(
        &mut transaction,
        &NackNotification {
            notification_id: request.notification_id,
            lease_owner: &request.instance_id,
            lease_token: request.lease_token,
            failure,
        },
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    transaction
        .commit()
        .await
        .map_err(|source| error::database_failure(&source, &request_id))?;

    let data = match outcome {
        NackNotificationOutcome::RetryScheduled {
            available_at,
            attempts,
        } => NackNotificationData::RetryScheduled {
            available_at,
            attempts,
        },
        NackNotificationOutcome::Failed {
            failure_code,
            attempts,
        } => NackNotificationData::Failed {
            failure_code,
            attempts,
        },
    };
    Ok(error::success(StatusCode::OK, &request_id, data))
}

fn database<'a>(state: &'a AppState, request_id: &RequestId) -> Result<&'a sqlx::PgPool, ApiError> {
    state.postgres().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            request_id,
            "database_unavailable",
            "데이터베이스를 일시적으로 사용할 수 없습니다.",
            None,
        )
    })
}

fn repository_error(source: NotificationOutboxError, request_id: &RequestId) -> ApiError {
    match source {
        NotificationOutboxError::InvalidLeaseOwner
        | NotificationOutboxError::InvalidClaimLimit
        | NotificationOutboxError::InvalidLeaseDuration
        | NotificationOutboxError::InvalidRetryDelay
        | NotificationOutboxError::InvalidFailure => ApiError::new(
            StatusCode::BAD_REQUEST,
            request_id,
            "invalid_notification_request",
            "Bot 알림 전송 요청 값이 올바르지 않습니다.",
            None,
        ),
        NotificationOutboxError::LeaseConflict => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "notification_lease_conflict",
            "알림 전송 lease가 만료되었거나 다른 Bot 인스턴스로 이전되었습니다.",
            None,
        ),
        NotificationOutboxError::UnexpectedNotificationType => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "notification_type_mismatch",
            "이 알림은 모집 메시지 확인으로 처리할 수 없습니다.",
            None,
        ),
        NotificationOutboxError::RecruitmentMessageCreateRequiresTypedAck => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "recruitment_message_create_requires_typed_ack",
            "모집 메시지 생성 알림은 전용 확인 경로로 처리해야 합니다.",
            None,
        ),
        NotificationOutboxError::RecruitmentMessageBindConflict
        | NotificationOutboxError::AuctionRecruitmentMessageBindConflict => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "recruitment_message_bind_conflict",
            "모집 메시지를 현재 모집 상태에 연결할 수 없습니다.",
            None,
        ),
        NotificationOutboxError::InvalidRecruitmentMessagePayload
        | NotificationOutboxError::InvalidAuctionRecruitmentMessagePayload
        | NotificationOutboxError::DeduplicationRowDisappeared
        | NotificationOutboxError::ClaimedRowDisappeared
        | NotificationOutboxError::DeduplicationConflict => {
            tracing::error!(%source, "notification outbox invariant failed");
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                request_id,
                "notification_outbox_invariant_failed",
                "저장된 알림 전송 상태를 처리할 수 없습니다.",
                None,
            )
        }
        NotificationOutboxError::InvalidDeduplicationKey
        | NotificationOutboxError::InvalidNotificationType => {
            tracing::error!(%source, "notification enqueue validation leaked into delivery route");
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                request_id,
                "notification_outbox_invariant_failed",
                "저장된 알림 전송 상태를 처리할 수 없습니다.",
                None,
            )
        }
        NotificationOutboxError::Sql(database_error) => {
            tracing::error!(%database_error, "notification outbox database operation failed");
            error::database_failure(&database_error, request_id)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_failure_wire_values_are_explicit() {
        let http: NotificationFailureRequest = serde_json::from_value(serde_json::json!({
            "kind": "HTTP",
            "status": 429,
            "retryAfterMs": 1000,
            "message": "rate limited"
        }))
        .expect("HTTP failure payload");
        assert!(matches!(http.kind, NotificationFailureKindRequest::Http));

        let unknown: NotificationFailureRequest = serde_json::from_value(serde_json::json!({
            "kind": "DELIVERY_UNKNOWN",
            "message": "response missing"
        }))
        .expect("unknown-delivery failure payload");
        assert!(matches!(
            unknown.kind,
            NotificationFailureKindRequest::DeliveryUnknown
        ));
    }

    #[test]
    fn nack_response_uses_stable_state_and_camel_case_fields() {
        let response = serde_json::to_value(NackNotificationData::Failed {
            failure_code: "MAX_ATTEMPTS",
            attempts: 8,
        })
        .expect("serializable response");
        assert_eq!(response["state"], "FAILED");
        assert_eq!(response["failureCode"], "MAX_ATTEMPTS");
        assert_eq!(response["attempts"], 8);
    }
}
