use serde_json::Value;
use sqlx::FromRow;
use thiserror::Error;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    domain::{GuildId, OutboxEventId},
    infrastructure::postgres::DatabaseTransaction,
};

#[derive(Clone, Debug)]
pub struct ClaimOutbox<'a> {
    pub guild_id: &'a GuildId,
    pub lease_owner: &'a str,
    pub lease_duration: std::time::Duration,
    pub limit: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OutboxLease {
    pub owner: String,
    pub token: Uuid,
    pub until: OffsetDateTime,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClaimedOutboxEvent {
    pub id: OutboxEventId,
    pub guild_id: GuildId,
    pub topic: String,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub aggregate_revision: i64,
    pub event_type: String,
    pub payload: Value,
    pub occurred_at: OffsetDateTime,
    pub available_at: OffsetDateTime,
    pub attempts: i32,
    pub lease: OutboxLease,
}

#[derive(Clone, Debug)]
pub struct AckOutbox<'a> {
    pub guild_id: &'a GuildId,
    pub event_id: OutboxEventId,
    pub lease_owner: &'a str,
    pub lease_token: Uuid,
    pub published_at: OffsetDateTime,
}

#[derive(Clone, Debug)]
pub struct NackOutbox<'a> {
    pub guild_id: &'a GuildId,
    pub event_id: OutboxEventId,
    pub lease_owner: &'a str,
    pub lease_token: Uuid,
    pub retry_delay: std::time::Duration,
    pub last_error: &'a str,
}

pub async fn claim(
    transaction: &mut DatabaseTransaction,
    request: &ClaimOutbox<'_>,
) -> Result<Vec<ClaimedOutboxEvent>, OutboxRepositoryError> {
    let lease_interval = validate_claim(request)?;

    let rows = sqlx::query_as::<_, OutboxRow>(
        "WITH claim_clock AS MATERIALIZED (
             SELECT clock_timestamp() AS current_time
         )
         SELECT id, topic, aggregate_type, aggregate_id, aggregate_revision, event_type,
                payload, occurred_at, available_at, attempts
         FROM outbox_events
         CROSS JOIN claim_clock
         WHERE guild_id = $1
           AND published_at IS NULL
           AND available_at <= claim_clock.current_time
           AND (lease_until IS NULL OR lease_until < claim_clock.current_time)
         ORDER BY available_at, id
         LIMIT $2
         FOR UPDATE OF outbox_events SKIP LOCKED",
    )
    .bind(request.guild_id)
    .bind(i64::from(request.limit))
    .fetch_all(&mut **transaction)
    .await?;

    let mut claimed = Vec::with_capacity(rows.len());
    for row in rows {
        let lease_token = Uuid::now_v7();
        let lease_until = sqlx::query_scalar::<_, OffsetDateTime>(
            "UPDATE outbox_events
             SET lease_owner = $1,
                 lease_token = $2,
                 lease_until = clock_timestamp() + $3
             WHERE guild_id = $4 AND id = $5 AND published_at IS NULL
             RETURNING lease_until",
        )
        .bind(request.lease_owner)
        .bind(lease_token)
        .bind(lease_interval)
        .bind(request.guild_id)
        .bind(row.id)
        .fetch_optional(&mut **transaction)
        .await?;

        let lease_until = lease_until.ok_or(OutboxRepositoryError::ClaimedRowDisappeared)?;

        claimed.push(ClaimedOutboxEvent {
            id: OutboxEventId::from_uuid(row.id),
            guild_id: request.guild_id.clone(),
            topic: row.topic,
            aggregate_type: row.aggregate_type,
            aggregate_id: row.aggregate_id,
            aggregate_revision: row.aggregate_revision,
            event_type: row.event_type,
            payload: row.payload,
            occurred_at: row.occurred_at,
            available_at: row.available_at,
            attempts: row.attempts,
            lease: OutboxLease {
                owner: request.lease_owner.to_owned(),
                token: lease_token,
                until: lease_until,
            },
        });
    }

    Ok(claimed)
}

pub async fn ack(
    transaction: &mut DatabaseTransaction,
    request: &AckOutbox<'_>,
) -> Result<u64, OutboxRepositoryError> {
    let result = sqlx::query(
        "UPDATE outbox_events
         SET published_at = $1, lease_owner = NULL, lease_token = NULL, lease_until = NULL
         WHERE guild_id = $2
           AND id = $3
           AND lease_owner = $4
           AND lease_token = $5
           AND published_at IS NULL",
    )
    .bind(request.published_at)
    .bind(request.guild_id)
    .bind(request.event_id.as_uuid())
    .bind(request.lease_owner)
    .bind(request.lease_token)
    .execute(&mut **transaction)
    .await?;

    Ok(result.rows_affected())
}

pub async fn nack(
    transaction: &mut DatabaseTransaction,
    request: &NackOutbox<'_>,
) -> Result<u64, OutboxRepositoryError> {
    let retry_interval = validate_retry_delay(request.retry_delay)?;
    let result = sqlx::query(
        "UPDATE outbox_events
         SET attempts = attempts + 1,
             available_at = clock_timestamp() + $1,
             last_error = $2,
             lease_owner = NULL,
             lease_token = NULL,
             lease_until = NULL
         WHERE guild_id = $3
           AND id = $4
           AND lease_owner = $5
           AND lease_token = $6
           AND published_at IS NULL",
    )
    .bind(retry_interval)
    .bind(request.last_error)
    .bind(request.guild_id)
    .bind(request.event_id.as_uuid())
    .bind(request.lease_owner)
    .bind(request.lease_token)
    .execute(&mut **transaction)
    .await?;

    Ok(result.rows_affected())
}

fn validate_claim(
    request: &ClaimOutbox<'_>,
) -> Result<sqlx::postgres::types::PgInterval, OutboxRepositoryError> {
    if request.lease_owner.trim().is_empty() {
        return Err(OutboxRepositoryError::EmptyLeaseOwner);
    }
    if request.limit == 0 {
        return Err(OutboxRepositoryError::EmptyClaimBatch);
    }
    postgres_interval(request.lease_duration).ok_or(OutboxRepositoryError::InvalidLeaseDuration)
}

fn postgres_interval(duration: std::time::Duration) -> Option<sqlx::postgres::types::PgInterval> {
    let microseconds = i64::try_from(duration.as_micros()).ok()?;
    (microseconds > 0).then_some(sqlx::postgres::types::PgInterval {
        months: 0,
        days: 0,
        microseconds,
    })
}

fn validate_retry_delay(
    retry_delay: std::time::Duration,
) -> Result<sqlx::postgres::types::PgInterval, OutboxRepositoryError> {
    postgres_interval(retry_delay).ok_or(OutboxRepositoryError::InvalidRetryDelay)
}

#[derive(Debug, FromRow)]
struct OutboxRow {
    id: Uuid,
    topic: String,
    aggregate_type: String,
    aggregate_id: String,
    aggregate_revision: i64,
    event_type: String,
    payload: Value,
    occurred_at: OffsetDateTime,
    available_at: OffsetDateTime,
    attempts: i32,
}

#[derive(Debug, Error)]
pub enum OutboxRepositoryError {
    #[error("outbox lease owner must not be empty")]
    EmptyLeaseOwner,
    #[error("outbox claim batch must contain at least one row")]
    EmptyClaimBatch,
    #[error("outbox lease duration must be at least one microsecond and fit PostgreSQL interval")]
    InvalidLeaseDuration,
    #[error("outbox retry delay must be at least one microsecond and fit PostgreSQL interval")]
    InvalidRetryDelay,
    #[error("outbox row disappeared after it was locked for claim")]
    ClaimedRowDisappeared,
    #[error("PostgreSQL outbox operation failed")]
    Sql(#[from] sqlx::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guild_id() -> GuildId {
        GuildId::parse("123456789012345678").expect("valid guild ID")
    }

    #[test]
    fn claim_requires_a_positive_batch_and_lease_duration() {
        let guild_id = guild_id();
        let valid = ClaimOutbox {
            guild_id: &guild_id,
            lease_owner: "relay-1",
            lease_duration: std::time::Duration::from_secs(30),
            limit: 1,
        };
        assert!(validate_claim(&valid).is_ok());

        let invalid_limit = ClaimOutbox {
            limit: 0,
            ..valid.clone()
        };
        assert!(matches!(
            validate_claim(&invalid_limit),
            Err(OutboxRepositoryError::EmptyClaimBatch)
        ));

        let invalid_duration = ClaimOutbox {
            lease_duration: std::time::Duration::ZERO,
            ..valid.clone()
        };
        assert!(matches!(
            validate_claim(&invalid_duration),
            Err(OutboxRepositoryError::InvalidLeaseDuration)
        ));

        let invalid_owner = ClaimOutbox {
            lease_owner: "  ",
            ..valid
        };
        assert!(matches!(
            validate_claim(&invalid_owner),
            Err(OutboxRepositoryError::EmptyLeaseOwner)
        ));
    }

    #[test]
    fn retry_delay_must_fit_a_positive_postgresql_interval() {
        assert!(validate_retry_delay(std::time::Duration::from_micros(1)).is_ok());
        assert!(matches!(
            validate_retry_delay(std::time::Duration::ZERO),
            Err(OutboxRepositoryError::InvalidRetryDelay)
        ));
        assert!(matches!(
            validate_retry_delay(std::time::Duration::MAX),
            Err(OutboxRepositoryError::InvalidRetryDelay)
        ));
    }
}
