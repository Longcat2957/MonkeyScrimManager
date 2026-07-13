use serde_json::Value;
use sqlx::FromRow;
use thiserror::Error;
use time::OffsetDateTime;
use uuid::Uuid;

use super::TenantScope;
use crate::{domain::IdempotencyRecordId, infrastructure::postgres::DatabaseTransaction};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdempotencyPrincipalType {
    User,
    Bot,
    Service,
}

impl IdempotencyPrincipalType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "USER",
            Self::Bot => "BOT",
            Self::Service => "SERVICE",
        }
    }
}

#[derive(Debug)]
pub struct AcquireIdempotency<'a> {
    pub record_id: IdempotencyRecordId,
    pub scope: &'a TenantScope,
    pub principal_type: IdempotencyPrincipalType,
    pub principal_id: &'a str,
    pub method: &'a str,
    pub route_template: &'a str,
    pub idempotency_key: &'a str,
    pub request_hash: &'a [u8],
    pub created_at: OffsetDateTime,
    pub expires_at: OffsetDateTime,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IdempotentResponse {
    pub status_code: u16,
    pub body: Option<Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AcquireIdempotencyOutcome {
    Acquired {
        record_id: IdempotencyRecordId,
    },
    Replay {
        record_id: IdempotencyRecordId,
        response: IdempotentResponse,
    },
    Conflict {
        record_id: IdempotencyRecordId,
    },
    InProgress,
}

#[derive(Debug)]
pub struct CompleteIdempotency<'a> {
    pub scope: &'a TenantScope,
    pub record_id: IdempotencyRecordId,
    pub response: IdempotentResponse,
    pub completed_at: OffsetDateTime,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompleteIdempotencyOutcome {
    Completed,
    NotInProgress,
}

pub async fn acquire(
    transaction: &mut DatabaseTransaction,
    command: &AcquireIdempotency<'_>,
) -> Result<AcquireIdempotencyOutcome, IdempotencyRepositoryError> {
    let acquired_command_lock = sqlx::query_scalar::<_, bool>(
        "SELECT pg_try_advisory_xact_lock(
             hashtextextended(
                 jsonb_build_array(
                     'monkeybot:idempotency:v1',
                     $1::text,
                     $2::text,
                     $3::text,
                     $4::text,
                     $5::text,
                     $6::text
                 )::text,
                 0
             )
         )",
    )
    .bind(command.scope.scope_key())
    .bind(command.principal_type.as_str())
    .bind(command.principal_id)
    .bind(command.method)
    .bind(command.route_template)
    .bind(command.idempotency_key)
    .fetch_one(&mut **transaction)
    .await?;
    if !acquired_command_lock {
        return Ok(AcquireIdempotencyOutcome::InProgress);
    }

    sqlx::query(
        "DELETE FROM idempotency_records
         WHERE scope_key = $1
           AND guild_id IS NOT DISTINCT FROM $2::text
           AND principal_type = $3
           AND principal_id = $4
           AND method = $5
           AND route_template = $6
           AND idempotency_key = $7
           AND expires_at <= $8",
    )
    .bind(command.scope.scope_key())
    .bind(command.scope.database_guild_id())
    .bind(command.principal_type.as_str())
    .bind(command.principal_id)
    .bind(command.method)
    .bind(command.route_template)
    .bind(command.idempotency_key)
    .bind(command.created_at)
    .execute(&mut **transaction)
    .await?;

    let inserted = sqlx::query(
        "INSERT INTO idempotency_records (
            id, guild_id, scope_key, principal_type, principal_id, method, route_template,
            idempotency_key, request_hash, state, created_at, expires_at
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'IN_PROGRESS', $10, $11)
         ON CONFLICT ON CONSTRAINT idempotency_records_command_key DO NOTHING",
    )
    .bind(command.record_id.as_uuid())
    .bind(command.scope.database_guild_id())
    .bind(command.scope.scope_key())
    .bind(command.principal_type.as_str())
    .bind(command.principal_id)
    .bind(command.method)
    .bind(command.route_template)
    .bind(command.idempotency_key)
    .bind(command.request_hash)
    .bind(command.created_at)
    .bind(command.expires_at)
    .execute(&mut **transaction)
    .await?;

    if inserted.rows_affected() == 1 {
        return Ok(AcquireIdempotencyOutcome::Acquired {
            record_id: command.record_id,
        });
    }

    let existing = sqlx::query_as::<_, IdempotencyRow>(
        "SELECT id, request_hash, state, status_code, response_body
         FROM idempotency_records
         WHERE scope_key = $1
           AND guild_id IS NOT DISTINCT FROM $2::text
           AND principal_type = $3
           AND principal_id = $4
           AND method = $5
           AND route_template = $6
           AND idempotency_key = $7
         FOR UPDATE",
    )
    .bind(command.scope.scope_key())
    .bind(command.scope.database_guild_id())
    .bind(command.principal_type.as_str())
    .bind(command.principal_id)
    .bind(command.method)
    .bind(command.route_template)
    .bind(command.idempotency_key)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(IdempotencyRepositoryError::MissingConflictingRecord)?;

    let record_id = IdempotencyRecordId::from_uuid(existing.id);
    if existing.request_hash.as_slice() != command.request_hash {
        return Ok(AcquireIdempotencyOutcome::Conflict { record_id });
    }

    match existing.state.as_str() {
        "IN_PROGRESS" => Ok(AcquireIdempotencyOutcome::InProgress),
        "COMPLETED" => {
            let status_code = existing
                .status_code
                .ok_or(IdempotencyRepositoryError::CorruptCompletedRecord)?;
            let status_code = u16::try_from(status_code)
                .map_err(|_| IdempotencyRepositoryError::CorruptCompletedRecord)?;
            Ok(AcquireIdempotencyOutcome::Replay {
                record_id,
                response: IdempotentResponse {
                    status_code,
                    body: existing.response_body,
                },
            })
        }
        _ => Err(IdempotencyRepositoryError::UnknownStoredState),
    }
}

pub async fn complete(
    transaction: &mut DatabaseTransaction,
    completion: CompleteIdempotency<'_>,
) -> Result<CompleteIdempotencyOutcome, IdempotencyRepositoryError> {
    let status_code = database_status_code(completion.response.status_code)?;
    let result = sqlx::query(
        "UPDATE idempotency_records
         SET state = 'COMPLETED', status_code = $1, response_body = $2, completed_at = $3
         WHERE id = $4
           AND scope_key = $5
           AND guild_id IS NOT DISTINCT FROM $6::text
           AND state = 'IN_PROGRESS'",
    )
    .bind(status_code)
    .bind(completion.response.body)
    .bind(completion.completed_at)
    .bind(completion.record_id.as_uuid())
    .bind(completion.scope.scope_key())
    .bind(completion.scope.database_guild_id())
    .execute(&mut **transaction)
    .await?;

    Ok(if result.rows_affected() == 1 {
        CompleteIdempotencyOutcome::Completed
    } else {
        CompleteIdempotencyOutcome::NotInProgress
    })
}

fn database_status_code(status_code: u16) -> Result<i16, IdempotencyRepositoryError> {
    if !(100..=599).contains(&status_code) {
        return Err(IdempotencyRepositoryError::InvalidStatusCode(status_code));
    }
    Ok(status_code as i16)
}

#[derive(Debug, FromRow)]
struct IdempotencyRow {
    id: Uuid,
    request_hash: Vec<u8>,
    state: String,
    status_code: Option<i16>,
    response_body: Option<Value>,
}

#[derive(Debug, Error)]
pub enum IdempotencyRepositoryError {
    #[error("conflicting idempotency record disappeared while locked")]
    MissingConflictingRecord,
    #[error("completed idempotency record violates its lifecycle invariant")]
    CorruptCompletedRecord,
    #[error("idempotency record has an unknown stored state")]
    UnknownStoredState,
    #[error("HTTP status code {0} cannot be stored in an idempotency record")]
    InvalidStatusCode(u16),
    #[error("PostgreSQL idempotency operation failed")]
    Sql(#[from] sqlx::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_status_codes_match_the_schema_range() {
        assert_eq!(database_status_code(100).expect("lower bound"), 100);
        assert_eq!(database_status_code(599).expect("upper bound"), 599);
        assert!(database_status_code(99).is_err());
        assert!(database_status_code(600).is_err());
    }

    #[test]
    fn principal_types_use_canonical_database_values() {
        assert_eq!(IdempotencyPrincipalType::User.as_str(), "USER");
        assert_eq!(IdempotencyPrincipalType::Bot.as_str(), "BOT");
        assert_eq!(IdempotencyPrincipalType::Service.as_str(), "SERVICE");
    }
}
