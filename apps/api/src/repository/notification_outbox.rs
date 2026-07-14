use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;
use thiserror::Error;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    domain::{DiscordChannelId, DiscordMessageId, GuildId, NotificationId, RecruitmentStatus},
    infrastructure::postgres::DatabaseTransaction,
};

pub const DEFAULT_CLAIM_LIMIT: u32 = 20;
pub const MAX_CLAIM_LIMIT: u32 = 50;
pub const DEFAULT_LEASE_DURATION: Duration = Duration::from_secs(30);
pub const MAX_NOTIFICATION_ATTEMPTS: i32 = 8;
pub const RECRUITMENT_MESSAGE_CREATE_TYPE: &str = "RECRUITMENT_MESSAGE_CREATE";
pub const RECRUITMENT_MESSAGE_UPDATE_TYPE: &str = "RECRUITMENT_MESSAGE_UPDATE";
pub const RECRUITMENT_MESSAGE_DELETE_TYPE: &str = "RECRUITMENT_MESSAGE_DELETE";
pub const RECRUITMENT_MESSAGE_SCHEMA_VERSION: u16 = 1;
pub const ENTRY_CONFIRMATION_CREATE_TYPE: &str = "ENTRY_CONFIRMATION_CREATE";
pub const ENTRY_CONFIRMATION_SCHEMA_VERSION: u16 = 1;
pub const SERIES_RESULT_CREATE_TYPE: &str = "SERIES_RESULT_CREATE";
pub const SERIES_RESULT_SCHEMA_VERSION: u16 = 2;

const MAX_LEASE_OWNER_BYTES: usize = 128;
const MAX_DEDUPLICATION_KEY_BYTES: usize = 255;
const MAX_NOTIFICATION_TYPE_BYTES: usize = 100;
const MAX_FAILURE_MESSAGE_BYTES: usize = 1_024;
const MAX_RETRY_AFTER: Duration = Duration::from_secs(60 * 60);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(5 * 60);

#[derive(Clone, Debug)]
pub struct EnqueueNotification<'a> {
    pub id: NotificationId,
    pub guild_id: &'a GuildId,
    pub deduplication_key: &'a str,
    pub channel_id: &'a DiscordChannelId,
    pub notification_type: &'a str,
    pub payload: &'a Value,
    pub created_at: OffsetDateTime,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnqueuedNotification {
    pub id: NotificationId,
    pub discord_nonce: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EnqueueNotificationOutcome {
    Enqueued(EnqueuedNotification),
    Existing(EnqueuedNotification),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecruitmentMessageNotificationPayload {
    pub schema_version: u16,
    pub recruitment_id: crate::domain::RecruitmentId,
    pub snapshot: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntryConfirmationNotificationPayload {
    pub schema_version: u16,
    pub series_id: crate::domain::SeriesId,
    pub snapshot: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SeriesResultNotificationPayload {
    pub schema_version: u16,
    pub series_id: crate::domain::SeriesId,
    pub snapshot: Value,
}

#[derive(Clone, Debug)]
pub struct ClaimNotifications<'a> {
    pub lease_owner: &'a str,
    pub lease_duration: Duration,
    pub limit: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationLease {
    pub owner: String,
    pub token: Uuid,
    pub until: OffsetDateTime,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClaimedNotification {
    pub id: NotificationId,
    pub guild_id: GuildId,
    pub channel_id: DiscordChannelId,
    pub notification_type: String,
    pub payload: Value,
    pub discord_nonce: String,
    pub attempts: i32,
    pub lease: NotificationLease,
}

#[derive(Clone, Debug)]
pub struct AckNotification<'a> {
    pub notification_id: NotificationId,
    pub lease_owner: &'a str,
    pub lease_token: Uuid,
    pub discord_message_id: &'a DiscordMessageId,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AcknowledgedNotification {
    pub id: NotificationId,
    pub guild_id: GuildId,
    pub channel_id: DiscordChannelId,
    pub notification_type: String,
    pub payload: Value,
    pub discord_message_id: DiscordMessageId,
    pub delivered_at: OffsetDateTime,
    pub attempts: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecruitmentMessageAcknowledgement {
    pub notification: AcknowledgedNotification,
    pub recruitment_id: crate::domain::RecruitmentId,
    pub recruitment_revision: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationFailureKind {
    Http,
    Transport,
    DeliveryUnknown,
}

#[derive(Clone, Debug)]
pub struct NotificationFailure<'a> {
    pub kind: NotificationFailureKind,
    pub status: Option<u16>,
    pub retry_after: Option<Duration>,
    pub message: &'a str,
}

#[derive(Clone, Debug)]
pub struct NackNotification<'a> {
    pub notification_id: NotificationId,
    pub lease_owner: &'a str,
    pub lease_token: Uuid,
    pub failure: NotificationFailure<'a>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NackNotificationOutcome {
    RetryScheduled {
        available_at: OffsetDateTime,
        attempts: i32,
    },
    Failed {
        failure_code: &'static str,
        attempts: i32,
    },
}

pub fn discord_nonce(notification_id: NotificationId) -> String {
    URL_SAFE_NO_PAD.encode(notification_id.as_uuid().as_bytes())
}

pub async fn enqueue(
    transaction: &mut DatabaseTransaction,
    request: &EnqueueNotification<'_>,
) -> Result<EnqueueNotificationOutcome, NotificationOutboxError> {
    validate_enqueue(request)?;
    let nonce = discord_nonce(request.id);
    let inserted = sqlx::query_as::<_, EnqueuedRow>(
        "INSERT INTO bot_notification_outbox (
             id, guild_id, deduplication_key, discord_nonce, channel_id, notification_type,
             payload, created_at, available_at
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $8)
         ON CONFLICT (guild_id, deduplication_key) DO NOTHING
         RETURNING id, discord_nonce",
    )
    .bind(request.id.as_uuid())
    .bind(request.guild_id)
    .bind(request.deduplication_key)
    .bind(&nonce)
    .bind(request.channel_id)
    .bind(request.notification_type)
    .bind(request.payload)
    .bind(request.created_at)
    .fetch_optional(&mut **transaction)
    .await?;

    if let Some(row) = inserted {
        return Ok(EnqueueNotificationOutcome::Enqueued(EnqueuedNotification {
            id: NotificationId::from_uuid(row.id),
            discord_nonce: row.discord_nonce,
        }));
    }

    let existing = sqlx::query_as::<_, ExistingEnqueueRow>(
        "SELECT id, discord_nonce, channel_id, notification_type, payload
         FROM bot_notification_outbox
         WHERE guild_id = $1 AND deduplication_key = $2",
    )
    .bind(request.guild_id)
    .bind(request.deduplication_key)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(NotificationOutboxError::DeduplicationRowDisappeared)?;

    if existing.channel_id != *request.channel_id
        || existing.notification_type != request.notification_type
        || &existing.payload != request.payload
    {
        return Err(NotificationOutboxError::DeduplicationConflict);
    }

    Ok(EnqueueNotificationOutcome::Existing(EnqueuedNotification {
        id: NotificationId::from_uuid(existing.id),
        discord_nonce: existing.discord_nonce,
    }))
}

pub async fn claim(
    transaction: &mut DatabaseTransaction,
    request: &ClaimNotifications<'_>,
) -> Result<Vec<ClaimedNotification>, NotificationOutboxError> {
    let lease_interval = validate_claim(request)?;
    reap_exhausted(transaction).await?;

    let rows = sqlx::query_as::<_, ClaimCandidateRow>(
        "WITH claim_clock AS MATERIALIZED (
             SELECT clock_timestamp() AS current_time
         )
         SELECT notification.id
         FROM bot_notification_outbox AS notification
         CROSS JOIN claim_clock
         WHERE notification.delivered_at IS NULL
           AND notification.failed_at IS NULL
           AND notification.attempts < $1
           AND notification.available_at <= claim_clock.current_time
           AND (
                notification.lease_until IS NULL
                OR notification.lease_until < claim_clock.current_time
           )
           AND (
                notification.notification_type NOT IN ($3, $4, $5)
                OR NOT EXISTS (
                    SELECT 1
                    FROM bot_notification_outbox AS earlier
                    WHERE earlier.guild_id = notification.guild_id
                      AND earlier.notification_type IN ($3, $4, $5)
                      AND earlier.delivered_at IS NULL
                      AND earlier.failed_at IS NULL
                      AND earlier.payload ->> 'recruitmentId'
                          = notification.payload ->> 'recruitmentId'
                      AND (earlier.created_at, earlier.id)
                          < (notification.created_at, notification.id)
                )
           )
         ORDER BY notification.available_at, notification.id
         LIMIT $2
         FOR UPDATE OF notification SKIP LOCKED",
    )
    .bind(MAX_NOTIFICATION_ATTEMPTS)
    .bind(i64::from(request.limit))
    .bind(RECRUITMENT_MESSAGE_CREATE_TYPE)
    .bind(RECRUITMENT_MESSAGE_UPDATE_TYPE)
    .bind(RECRUITMENT_MESSAGE_DELETE_TYPE)
    .fetch_all(&mut **transaction)
    .await?;

    let mut claimed = Vec::with_capacity(rows.len());
    for row in rows {
        let lease_token = Uuid::now_v7();
        let updated = sqlx::query_as::<_, ClaimedNotificationRow>(
            "WITH claim_clock AS MATERIALIZED (
                 SELECT clock_timestamp() AS current_time
             )
             UPDATE bot_notification_outbox AS notification
             SET attempts = notification.attempts + 1,
                 first_attempt_at = COALESCE(notification.first_attempt_at, claim_clock.current_time),
                 lease_owner = $1,
                 lease_token = $2,
                 lease_until = claim_clock.current_time + $3
             FROM claim_clock
             WHERE notification.id = $4
               AND notification.delivered_at IS NULL
               AND notification.failed_at IS NULL
               AND notification.attempts < $5
             RETURNING notification.id, notification.guild_id, notification.channel_id,
                       notification.notification_type, notification.payload,
                       notification.discord_nonce, notification.attempts,
                       notification.lease_until",
        )
        .bind(request.lease_owner)
        .bind(lease_token)
        .bind(lease_interval)
        .bind(row.id)
        .bind(MAX_NOTIFICATION_ATTEMPTS)
        .fetch_optional(&mut **transaction)
        .await?
        .ok_or(NotificationOutboxError::ClaimedRowDisappeared)?;

        claimed.push(ClaimedNotification {
            id: NotificationId::from_uuid(updated.id),
            guild_id: updated.guild_id,
            channel_id: updated.channel_id,
            notification_type: updated.notification_type,
            payload: updated.payload,
            discord_nonce: updated.discord_nonce,
            attempts: updated.attempts,
            lease: NotificationLease {
                owner: request.lease_owner.to_owned(),
                token: lease_token,
                until: updated.lease_until,
            },
        });
    }

    Ok(claimed)
}

pub async fn ack(
    transaction: &mut DatabaseTransaction,
    request: &AckNotification<'_>,
) -> Result<AcknowledgedNotification, NotificationOutboxError> {
    acknowledge(transaction, request, AcknowledgementMode::Generic).await
}

async fn acknowledge(
    transaction: &mut DatabaseTransaction,
    request: &AckNotification<'_>,
    mode: AcknowledgementMode,
) -> Result<AcknowledgedNotification, NotificationOutboxError> {
    validate_lease_owner(request.lease_owner)?;
    let notification_type = sqlx::query_scalar::<_, String>(
        "SELECT notification.notification_type
         FROM bot_notification_outbox AS notification
         WHERE notification.id = $1
           AND notification.lease_owner = $2
           AND notification.lease_token = $3
           AND notification.lease_until >= clock_timestamp()
           AND notification.delivered_at IS NULL
           AND notification.failed_at IS NULL
         FOR UPDATE",
    )
    .bind(request.notification_id.as_uuid())
    .bind(request.lease_owner)
    .bind(request.lease_token)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(NotificationOutboxError::LeaseConflict)?;
    match mode {
        AcknowledgementMode::Generic if notification_type == RECRUITMENT_MESSAGE_CREATE_TYPE => {
            return Err(NotificationOutboxError::RecruitmentMessageCreateRequiresTypedAck);
        }
        AcknowledgementMode::RecruitmentMessageCreate
            if notification_type != RECRUITMENT_MESSAGE_CREATE_TYPE =>
        {
            return Err(NotificationOutboxError::UnexpectedNotificationType);
        }
        _ => {}
    }

    let row = sqlx::query_as::<_, AcknowledgedNotificationRow>(
        "UPDATE bot_notification_outbox AS notification
         SET discord_message_id = $1,
             delivered_at = clock_timestamp(),
             lease_owner = NULL,
             lease_token = NULL,
             lease_until = NULL
         WHERE notification.id = $2
           AND notification.lease_owner = $3
           AND notification.lease_token = $4
           AND notification.lease_until >= clock_timestamp()
           AND notification.delivered_at IS NULL
           AND notification.failed_at IS NULL
         RETURNING notification.id, notification.guild_id, notification.channel_id,
                   notification.notification_type, notification.payload,
                   notification.discord_message_id, notification.delivered_at,
                   notification.attempts",
    )
    .bind(request.discord_message_id)
    .bind(request.notification_id.as_uuid())
    .bind(request.lease_owner)
    .bind(request.lease_token)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(NotificationOutboxError::LeaseConflict)?;

    Ok(AcknowledgedNotification {
        id: NotificationId::from_uuid(row.id),
        guild_id: row.guild_id,
        channel_id: row.channel_id,
        notification_type: row.notification_type,
        payload: row.payload,
        discord_message_id: row.discord_message_id,
        delivered_at: row.delivered_at,
        attempts: row.attempts,
    })
}

pub async fn ack_recruitment_message(
    transaction: &mut DatabaseTransaction,
    request: &AckNotification<'_>,
) -> Result<RecruitmentMessageAcknowledgement, NotificationOutboxError> {
    let notification = acknowledge(
        transaction,
        request,
        AcknowledgementMode::RecruitmentMessageCreate,
    )
    .await?;
    let payload = serde_json::from_value::<RecruitmentMessageNotificationPayload>(
        notification.payload.clone(),
    )
    .map_err(|_| NotificationOutboxError::InvalidRecruitmentMessagePayload)?;
    if payload.schema_version != RECRUITMENT_MESSAGE_SCHEMA_VERSION || !payload.snapshot.is_object()
    {
        return Err(NotificationOutboxError::InvalidRecruitmentMessagePayload);
    }

    let binding = sqlx::query_as::<_, RecruitmentBindingRow>(
        "UPDATE recruitments
         SET channel_id = $1,
             message_id = $2,
             revision = CASE
                 WHEN message_id IS NULL THEN revision + 1
                 ELSE revision
             END,
             updated_at = CASE
                 WHEN message_id IS NULL THEN $3
                 ELSE updated_at
             END
         WHERE guild_id = $4
           AND id = $5
           AND (
                message_id IS NULL
                OR (channel_id = $1 AND message_id = $2)
           )
         RETURNING revision, status",
    )
    .bind(&notification.channel_id)
    .bind(&notification.discord_message_id)
    .bind(notification.delivered_at)
    .bind(&notification.guild_id)
    .bind(payload.recruitment_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(NotificationOutboxError::RecruitmentMessageBindConflict)?;

    let sent_revision = payload
        .snapshot
        .pointer("/recruitment/revision")
        .and_then(Value::as_i64)
        .ok_or(NotificationOutboxError::InvalidRecruitmentMessagePayload)?;
    let notification_type = if binding.status == RecruitmentStatus::Cancelled {
        Some(RECRUITMENT_MESSAGE_DELETE_TYPE)
    } else if binding.revision > sent_revision.saturating_add(1) {
        Some(RECRUITMENT_MESSAGE_UPDATE_TYPE)
    } else {
        None
    };
    if let Some(notification_type) = notification_type {
        let current_payload = recruitment_message_payload(
            transaction,
            &notification.guild_id,
            payload.recruitment_id,
        )
        .await?;
        let deduplication_key = match notification_type {
            RECRUITMENT_MESSAGE_DELETE_TYPE => {
                format!("recruitment:{}:message:delete", payload.recruitment_id)
            }
            RECRUITMENT_MESSAGE_UPDATE_TYPE => format!(
                "recruitment:{}:message:update:{}",
                payload.recruitment_id, binding.revision
            ),
            _ => unreachable!("only recruitment message update/delete types are selected"),
        };
        enqueue(
            transaction,
            &EnqueueNotification {
                id: NotificationId::new(),
                guild_id: &notification.guild_id,
                deduplication_key: &deduplication_key,
                channel_id: &notification.channel_id,
                notification_type,
                payload: &current_payload,
                created_at: notification.delivered_at,
            },
        )
        .await?;
    }

    Ok(RecruitmentMessageAcknowledgement {
        notification,
        recruitment_id: payload.recruitment_id,
        recruitment_revision: binding.revision,
    })
}

pub async fn recruitment_message_payload(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: crate::domain::RecruitmentId,
) -> Result<Value, NotificationOutboxError> {
    sqlx::query_scalar::<_, Value>(
        "SELECT jsonb_build_object(
             'schemaVersion', $3,
             'recruitmentId', recruitment.id::text,
             'snapshot', jsonb_build_object(
                 'recruitment', jsonb_build_object(
                     'id', recruitment.id::text,
                     'displayNumber', recruitment.display_number,
                     'title', recruitment.title,
                     'status', recruitment.status,
                     'targetCount', recruitment.target_count,
                     'revision', recruitment.revision,
                     'channelId', recruitment.channel_id,
                     'messageId', recruitment.message_id,
                     'createdById', recruitment.created_by,
                     'updatedAt', recruitment.updated_at
                 ),
                 'participants', COALESCE((
                     SELECT jsonb_agg(
                         jsonb_build_object(
                             'id', participant.user_id,
                             'displayName', member.display_name,
                             'preferredLanes', COALESCE((
                                 SELECT jsonb_agg(preferred.lane ORDER BY preferred.priority)
                                 FROM recruitment_preferred_lanes AS preferred
                                 WHERE preferred.guild_id = participant.guild_id
                                   AND preferred.recruitment_id = participant.recruitment_id
                                   AND preferred.user_id = participant.user_id
                             ), '[]'::jsonb)
                         )
                         ORDER BY participant.joined_at, participant.user_id
                     )
                     FROM recruitment_participants AS participant
                     JOIN guild_members AS member
                       ON member.guild_id = participant.guild_id
                      AND member.user_id = participant.user_id
                     WHERE participant.guild_id = recruitment.guild_id
                       AND participant.recruitment_id = recruitment.id
                       AND participant.status = 'JOINED'
                 ), '[]'::jsonb),
                 'links', jsonb_build_object(
                     'entry', format('/g/%s/recruitments/%s/entry', recruitment.guild_id, recruitment.id)
                 )
             )
         )
         FROM recruitments AS recruitment
         WHERE recruitment.guild_id = $1 AND recruitment.id = $2",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .bind(i32::from(RECRUITMENT_MESSAGE_SCHEMA_VERSION))
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(NotificationOutboxError::RecruitmentMessageBindConflict)
}

pub async fn nack(
    transaction: &mut DatabaseTransaction,
    request: &NackNotification<'_>,
) -> Result<NackNotificationOutcome, NotificationOutboxError> {
    validate_lease_owner(request.lease_owner)?;
    validate_failure(&request.failure)?;

    let current = sqlx::query_as::<_, PendingNotificationRow>(
        "SELECT notification.attempts
         FROM bot_notification_outbox AS notification
         WHERE notification.id = $1
           AND notification.lease_owner = $2
           AND notification.lease_token = $3
           AND notification.lease_until >= clock_timestamp()
           AND notification.delivered_at IS NULL
           AND notification.failed_at IS NULL
         FOR UPDATE",
    )
    .bind(request.notification_id.as_uuid())
    .bind(request.lease_owner)
    .bind(request.lease_token)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(NotificationOutboxError::LeaseConflict)?;

    match classify_failure(request.notification_id, current.attempts, &request.failure)? {
        FailureDisposition::Retry(delay) => {
            let retry_interval =
                postgres_interval(delay).ok_or(NotificationOutboxError::InvalidRetryDelay)?;
            let available_at = sqlx::query_scalar::<_, OffsetDateTime>(
                "UPDATE bot_notification_outbox AS notification
                 SET available_at = clock_timestamp() + $1,
                     last_error = $2,
                     lease_owner = NULL,
                     lease_token = NULL,
                     lease_until = NULL
                 WHERE notification.id = $3
                   AND notification.lease_owner = $4
                   AND notification.lease_token = $5
                   AND notification.lease_until >= clock_timestamp()
                   AND notification.delivered_at IS NULL
                   AND notification.failed_at IS NULL
                 RETURNING notification.available_at",
            )
            .bind(retry_interval)
            .bind(request.failure.message)
            .bind(request.notification_id.as_uuid())
            .bind(request.lease_owner)
            .bind(request.lease_token)
            .fetch_optional(&mut **transaction)
            .await?
            .ok_or(NotificationOutboxError::LeaseConflict)?;

            Ok(NackNotificationOutcome::RetryScheduled {
                available_at,
                attempts: current.attempts,
            })
        }
        FailureDisposition::Failed(failure_code) => {
            let applied = sqlx::query(
                "UPDATE bot_notification_outbox AS notification
                 SET failed_at = clock_timestamp(),
                     failure_code = $1,
                     last_error = $2,
                     lease_owner = NULL,
                     lease_token = NULL,
                     lease_until = NULL
                 WHERE notification.id = $3
                   AND notification.lease_owner = $4
                   AND notification.lease_token = $5
                   AND notification.lease_until >= clock_timestamp()
                   AND notification.delivered_at IS NULL
                   AND notification.failed_at IS NULL",
            )
            .bind(failure_code)
            .bind(request.failure.message)
            .bind(request.notification_id.as_uuid())
            .bind(request.lease_owner)
            .bind(request.lease_token)
            .execute(&mut **transaction)
            .await?;
            if applied.rows_affected() != 1 {
                return Err(NotificationOutboxError::LeaseConflict);
            }

            Ok(NackNotificationOutcome::Failed {
                failure_code,
                attempts: current.attempts,
            })
        }
    }
}

pub async fn reap_exhausted(
    transaction: &mut DatabaseTransaction,
) -> Result<u64, NotificationOutboxError> {
    let result = sqlx::query(
        "WITH claim_clock AS MATERIALIZED (
             SELECT clock_timestamp() AS current_time
         )
         UPDATE bot_notification_outbox AS notification
         SET failed_at = claim_clock.current_time,
             failure_code = 'MAX_ATTEMPTS',
             last_error = COALESCE(
                 NULLIF(notification.last_error, ''),
                 'notification delivery exhausted maximum attempts'
             ),
             lease_owner = NULL,
             lease_token = NULL,
             lease_until = NULL
         FROM claim_clock
         WHERE notification.delivered_at IS NULL
           AND notification.failed_at IS NULL
           AND notification.attempts >= $1
           AND (
                notification.lease_until IS NULL
                OR notification.lease_until < claim_clock.current_time
           )",
    )
    .bind(MAX_NOTIFICATION_ATTEMPTS)
    .execute(&mut **transaction)
    .await?;
    Ok(result.rows_affected())
}

fn validate_enqueue(request: &EnqueueNotification<'_>) -> Result<(), NotificationOutboxError> {
    validate_bounded_text(
        request.deduplication_key,
        MAX_DEDUPLICATION_KEY_BYTES,
        NotificationOutboxError::InvalidDeduplicationKey,
    )?;
    validate_bounded_text(
        request.notification_type,
        MAX_NOTIFICATION_TYPE_BYTES,
        NotificationOutboxError::InvalidNotificationType,
    )
}

fn validate_claim(
    request: &ClaimNotifications<'_>,
) -> Result<sqlx::postgres::types::PgInterval, NotificationOutboxError> {
    validate_lease_owner(request.lease_owner)?;
    if !(1..=MAX_CLAIM_LIMIT).contains(&request.limit) {
        return Err(NotificationOutboxError::InvalidClaimLimit);
    }
    postgres_interval(request.lease_duration).ok_or(NotificationOutboxError::InvalidLeaseDuration)
}

fn validate_lease_owner(value: &str) -> Result<(), NotificationOutboxError> {
    if value.is_empty()
        || value.len() > MAX_LEASE_OWNER_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
    {
        return Err(NotificationOutboxError::InvalidLeaseOwner);
    }
    Ok(())
}

fn validate_failure(failure: &NotificationFailure<'_>) -> Result<(), NotificationOutboxError> {
    validate_bounded_text(
        failure.message,
        MAX_FAILURE_MESSAGE_BYTES,
        NotificationOutboxError::InvalidFailure,
    )?;
    match failure.kind {
        NotificationFailureKind::Http => {
            let status = failure
                .status
                .ok_or(NotificationOutboxError::InvalidFailure)?;
            if !(400..=599).contains(&status)
                || (failure.retry_after.is_some() && status != 429)
                || failure
                    .retry_after
                    .is_some_and(|value| value.is_zero() || value > MAX_RETRY_AFTER)
            {
                return Err(NotificationOutboxError::InvalidFailure);
            }
        }
        NotificationFailureKind::Transport | NotificationFailureKind::DeliveryUnknown => {
            if failure.status.is_some() || failure.retry_after.is_some() {
                return Err(NotificationOutboxError::InvalidFailure);
            }
        }
    }
    Ok(())
}

fn validate_bounded_text(
    value: &str,
    maximum: usize,
    error: NotificationOutboxError,
) -> Result<(), NotificationOutboxError> {
    if value.trim().is_empty() || value.len() > maximum {
        Err(error)
    } else {
        Ok(())
    }
}

fn postgres_interval(duration: Duration) -> Option<sqlx::postgres::types::PgInterval> {
    let microseconds = i64::try_from(duration.as_micros()).ok()?;
    (microseconds > 0).then_some(sqlx::postgres::types::PgInterval {
        months: 0,
        days: 0,
        microseconds,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FailureDisposition {
    Retry(Duration),
    Failed(&'static str),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AcknowledgementMode {
    Generic,
    RecruitmentMessageCreate,
}

fn classify_failure(
    notification_id: NotificationId,
    attempts: i32,
    failure: &NotificationFailure<'_>,
) -> Result<FailureDisposition, NotificationOutboxError> {
    let retry = |retry_after: Option<Duration>| {
        if attempts >= MAX_NOTIFICATION_ATTEMPTS {
            FailureDisposition::Failed("MAX_ATTEMPTS")
        } else {
            FailureDisposition::Retry(
                retry_after.unwrap_or_else(|| exponential_retry_delay(notification_id, attempts)),
            )
        }
    };

    match failure.kind {
        NotificationFailureKind::DeliveryUnknown => {
            Ok(FailureDisposition::Failed("DELIVERY_UNKNOWN"))
        }
        NotificationFailureKind::Transport => Ok(retry(None)),
        NotificationFailureKind::Http => match failure.status {
            Some(429) => Ok(retry(failure.retry_after)),
            Some(500..=599) => Ok(retry(None)),
            Some(400..=499) => Ok(FailureDisposition::Failed("DISCORD_HTTP_4XX")),
            _ => Err(NotificationOutboxError::InvalidFailure),
        },
    }
}

fn exponential_retry_delay(notification_id: NotificationId, attempts: i32) -> Duration {
    let exponent = attempts.saturating_sub(1).clamp(0, 8) as u32;
    let seconds = 1_u64 << exponent;
    let base = Duration::from_secs(seconds).min(MAX_RETRY_DELAY);
    let uuid = notification_id.as_uuid();
    let bytes = uuid.as_bytes();
    let seed = u64::from_be_bytes(bytes[..8].try_into().expect("UUID prefix is 8 bytes"))
        ^ u64::try_from(attempts).unwrap_or_default();
    let jitter = Duration::from_millis(seed % 1_000);
    base.checked_add(jitter)
        .unwrap_or(MAX_RETRY_DELAY)
        .min(MAX_RETRY_DELAY)
}

#[derive(Debug, FromRow)]
struct EnqueuedRow {
    id: Uuid,
    discord_nonce: String,
}

#[derive(Debug, FromRow)]
struct ExistingEnqueueRow {
    id: Uuid,
    discord_nonce: String,
    channel_id: DiscordChannelId,
    notification_type: String,
    payload: Value,
}

#[derive(Debug, FromRow)]
struct ClaimCandidateRow {
    id: Uuid,
}

#[derive(Debug, FromRow)]
struct ClaimedNotificationRow {
    id: Uuid,
    guild_id: GuildId,
    channel_id: DiscordChannelId,
    notification_type: String,
    payload: Value,
    discord_nonce: String,
    attempts: i32,
    lease_until: OffsetDateTime,
}

#[derive(Debug, FromRow)]
struct AcknowledgedNotificationRow {
    id: Uuid,
    guild_id: GuildId,
    channel_id: DiscordChannelId,
    notification_type: String,
    payload: Value,
    discord_message_id: DiscordMessageId,
    delivered_at: OffsetDateTime,
    attempts: i32,
}

#[derive(Debug, FromRow)]
struct RecruitmentBindingRow {
    revision: i64,
    status: RecruitmentStatus,
}

#[derive(Debug, FromRow)]
struct PendingNotificationRow {
    attempts: i32,
}

#[derive(Debug, Error)]
pub enum NotificationOutboxError {
    #[error("notification lease owner is invalid")]
    InvalidLeaseOwner,
    #[error("notification claim limit is invalid")]
    InvalidClaimLimit,
    #[error("notification lease duration is invalid")]
    InvalidLeaseDuration,
    #[error("notification retry delay is invalid")]
    InvalidRetryDelay,
    #[error("notification deduplication key is invalid")]
    InvalidDeduplicationKey,
    #[error("notification type is invalid")]
    InvalidNotificationType,
    #[error("notification failure payload is invalid")]
    InvalidFailure,
    #[error("notification deduplication key was reused for a different notification")]
    DeduplicationConflict,
    #[error("notification deduplication row disappeared after conflict")]
    DeduplicationRowDisappeared,
    #[error("notification claim row disappeared after it was locked")]
    ClaimedRowDisappeared,
    #[error("notification lease is no longer current")]
    LeaseConflict,
    #[error("recruitment message create notifications require the typed acknowledgement route")]
    RecruitmentMessageCreateRequiresTypedAck,
    #[error("notification type cannot be acknowledged as a recruitment message")]
    UnexpectedNotificationType,
    #[error("recruitment message notification payload is invalid")]
    InvalidRecruitmentMessagePayload,
    #[error("recruitment message cannot be bound to the notification target")]
    RecruitmentMessageBindConflict,
    #[error("PostgreSQL notification outbox operation failed")]
    Sql(#[from] sqlx::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notification_id() -> NotificationId {
        NotificationId::from_uuid(
            Uuid::parse_str("01900000-0000-7000-8000-000000000001").expect("valid UUID"),
        )
    }

    #[test]
    fn nonce_is_stable_and_discord_safe_length() {
        let nonce = discord_nonce(notification_id());
        assert_eq!(nonce, discord_nonce(notification_id()));
        assert_eq!(nonce.len(), 22);
        assert!(
            nonce
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        );
    }

    #[test]
    fn claim_validation_requires_a_bot_instance_shape_and_bounded_limit() {
        for owner in ["", "bot worker", "a".repeat(129).as_str()] {
            let request = ClaimNotifications {
                lease_owner: owner,
                lease_duration: DEFAULT_LEASE_DURATION,
                limit: 1,
            };
            assert!(matches!(
                validate_claim(&request),
                Err(NotificationOutboxError::InvalidLeaseOwner)
            ));
        }

        let owner = "bot-1";
        for limit in [0, MAX_CLAIM_LIMIT + 1] {
            let request = ClaimNotifications {
                lease_owner: owner,
                lease_duration: DEFAULT_LEASE_DURATION,
                limit,
            };
            assert!(matches!(
                validate_claim(&request),
                Err(NotificationOutboxError::InvalidClaimLimit)
            ));
        }
    }

    #[test]
    fn failure_classification_preserves_terminal_and_retry_boundaries() {
        let id = notification_id();
        let transport = NotificationFailure {
            kind: NotificationFailureKind::Transport,
            status: None,
            retry_after: None,
            message: "request timed out",
        };
        assert!(matches!(
            classify_failure(id, 1, &transport),
            Ok(FailureDisposition::Retry(_))
        ));
        assert!(matches!(
            classify_failure(id, MAX_NOTIFICATION_ATTEMPTS, &transport),
            Ok(FailureDisposition::Failed("MAX_ATTEMPTS"))
        ));

        let forbidden = NotificationFailure {
            kind: NotificationFailureKind::Http,
            status: Some(403),
            retry_after: None,
            message: "missing Send Messages",
        };
        assert!(matches!(
            classify_failure(id, 1, &forbidden),
            Ok(FailureDisposition::Failed("DISCORD_HTTP_4XX"))
        ));

        let unknown = NotificationFailure {
            kind: NotificationFailureKind::DeliveryUnknown,
            status: None,
            retry_after: None,
            message: "response lost after send",
        };
        assert!(matches!(
            classify_failure(id, 1, &unknown),
            Ok(FailureDisposition::Failed("DELIVERY_UNKNOWN"))
        ));
    }

    #[test]
    fn retry_after_is_only_valid_for_discord_rate_limits() {
        let invalid = NotificationFailure {
            kind: NotificationFailureKind::Http,
            status: Some(500),
            retry_after: Some(Duration::from_secs(1)),
            message: "server error",
        };
        assert!(matches!(
            validate_failure(&invalid),
            Err(NotificationOutboxError::InvalidFailure)
        ));

        let valid = NotificationFailure {
            kind: NotificationFailureKind::Http,
            status: Some(429),
            retry_after: Some(Duration::from_secs(1)),
            message: "rate limited",
        };
        assert!(validate_failure(&valid).is_ok());
    }
}
