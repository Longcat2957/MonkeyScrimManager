use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sqlx::{FromRow, PgConnection, PgPool};
use thiserror::Error;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use super::{
    TenantScope,
    idempotency::{
        self, AcquireIdempotency, AcquireIdempotencyOutcome, CompleteIdempotency,
        CompleteIdempotencyOutcome, IdempotencyPrincipalType, IdempotencyRepositoryError,
        IdempotentResponse,
    },
};
use crate::{
    auth::AuthService,
    domain::{GuildId, IdempotencyRecordId, SeasonId, SeasonStatus, UserId},
    infrastructure::postgres::{
        DatabaseTransaction, DeadlockRetryPolicy, TransactionAttemptError, TransactionRetryError,
        run_with_deadlock_retry,
    },
};

const IDEMPOTENCY_TTL: Duration = Duration::hours(24);
const ROTATE_SEASON_METHOD: &str = "POST";
const ROTATE_SEASON_ROUTE: &str = "/api/guilds/{guildId}/seasons";
const MAX_SEASON_NAME_CHARS: usize = 100;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RotateSeasonCommand {
    pub name: String,
    pub expected_current_season_id: SeasonId,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildSeasonsSnapshot {
    pub current: SeasonSummary,
    pub completed: Vec<SeasonSummary>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeasonSummary {
    pub id: SeasonId,
    pub name: String,
    pub status: SeasonStatus,
    #[serde(with = "time::serde::rfc3339")]
    pub started_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub ended_at: Option<OffsetDateTime>,
    pub created_by: Option<String>,
    pub participant_count: i64,
    pub general_series_count: i64,
    pub auction_tournament_count: i64,
    pub auction_ten_count: i64,
    pub auction_twenty_count: i64,
    pub confirmed_game_count: i64,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_game_at: Option<OffsetDateTime>,
}

pub async fn get(
    pool: &PgPool,
    guild_id: &GuildId,
    actor_id: &UserId,
) -> Result<GuildSeasonsSnapshot, GuildSeasonsRepositoryError> {
    let mut transaction = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *transaction)
        .await?;
    authorize_operator(&mut transaction, guild_id, actor_id)
        .await
        .map_err(AttemptError::into_public)?;
    let snapshot = fetch_snapshot(&mut transaction, guild_id)
        .await
        .map_err(AttemptError::into_public)?;
    transaction.commit().await?;
    Ok(snapshot)
}

pub async fn rotate(
    pool: &PgPool,
    guild_id: &GuildId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &str,
    command: RotateSeasonCommand,
) -> Result<GuildSeasonsSnapshot, GuildSeasonsRepositoryError> {
    validate_command(&command)?;
    let request_hash = request_hash(guild_id, &command)?;
    let guild_id = guild_id.clone();
    let actor_id = actor_id.clone();
    let idempotency_key = idempotency_key.to_owned();
    let request_id = request_id.to_owned();
    let prepared = RotatePrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let actor_id = actor_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let request_hash = request_hash.clone();
            let command = command.clone();

            Box::pin(async move {
                rotate_attempt(
                    transaction,
                    &guild_id,
                    &actor_id,
                    &idempotency_key,
                    &request_id,
                    &request_hash,
                    &command,
                    prepared,
                )
                .await
            })
        },
        attempt_sql_error,
    )
    .await
    .map_err(map_transaction_error)
}

#[allow(clippy::too_many_arguments)]
async fn rotate_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &str,
    request_hash: &[u8],
    command: &RotateSeasonCommand,
    prepared: RotatePrepared,
) -> Result<GuildSeasonsSnapshot, AttemptError> {
    let now = OffsetDateTime::now_utc();
    authorize_operator(transaction, guild_id, actor_id).await?;

    let scope = TenantScope::guild(guild_id.clone());
    let idempotency_record_id = match acquire_idempotency::<GuildSeasonsSnapshot>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            principal_id: actor_id.as_str(),
            key: idempotency_key,
            request_hash,
            now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(snapshot) => return Ok(snapshot),
    };

    lock_guild_rotation(transaction, guild_id).await?;
    let previous = lock_active_season(transaction, guild_id).await?;
    if previous.id != command.expected_current_season_id {
        return Err(AttemptError::CurrentSeasonConflict {
            expected: command.expected_current_season_id,
            current: previous.id,
        });
    }

    let closed = sqlx::query(
        "UPDATE seasons
         SET status = 'CLOSED', ended_at = $1
         WHERE guild_id = $2 AND id = $3 AND status = 'ACTIVE' AND ended_at IS NULL",
    )
    .bind(now)
    .bind(guild_id)
    .bind(previous.id.as_uuid())
    .execute(&mut **transaction)
    .await?;
    if closed.rows_affected() != 1 {
        return Err(AttemptError::InvariantViolation);
    }

    let name = command.name.trim();
    sqlx::query(
        "INSERT INTO seasons (
             guild_id, id, name, status, started_at, ended_at, created_by, created_at
         ) VALUES ($1, $2, $3, 'ACTIVE', $4, NULL, $5, $4)",
    )
    .bind(guild_id)
    .bind(prepared.season_id.as_uuid())
    .bind(name)
    .bind(now)
    .bind(actor_id)
    .execute(&mut **transaction)
    .await?;

    insert_audit(
        transaction,
        guild_id,
        actor_id,
        request_id,
        prepared.audit_id,
        &previous,
        prepared.season_id,
        name,
        now,
    )
    .await?;

    let snapshot = fetch_snapshot(transaction, guild_id).await?;
    complete_idempotency(transaction, &scope, idempotency_record_id, &snapshot, now).await?;
    Ok(snapshot)
}

async fn lock_guild_rotation(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
) -> Result<(), AttemptError> {
    sqlx::query(
        "SELECT pg_advisory_xact_lock(
             hashtextextended(
                 jsonb_build_array(
                     'monkeybot:season-rotation:v1',
                     $1::text
                 )::text,
                 0
             )
         )",
    )
    .bind(guild_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn authorize_operator(
    connection: &mut PgConnection,
    guild_id: &GuildId,
    actor_id: &UserId,
) -> Result<(), AttemptError> {
    let is_operator = sqlx::query_scalar::<_, bool>(
        "SELECT true
         FROM guilds AS guild
         JOIN guild_members AS actor
           ON actor.guild_id = guild.guild_id
          AND actor.user_id = $2
          AND actor.status = 'ACTIVE'
         JOIN guild_config AS config ON config.guild_id = guild.guild_id
         JOIN guild_member_discord_roles AS actor_role
           ON actor_role.guild_id = actor.guild_id
          AND actor_role.user_id = actor.user_id
          AND actor_role.role_id = config.operator_role_id
         WHERE guild.guild_id = $1 AND guild.status = 'ACTIVE'
         FOR SHARE OF guild, actor, config, actor_role",
    )
    .bind(guild_id)
    .bind(actor_id)
    .fetch_optional(&mut *connection)
    .await?;
    if is_operator.is_some() {
        return Ok(());
    }

    let is_member = sqlx::query_scalar::<_, bool>(
        "SELECT true
         FROM guilds AS guild
         JOIN guild_members AS actor
           ON actor.guild_id = guild.guild_id
          AND actor.user_id = $2
          AND actor.status = 'ACTIVE'
         WHERE guild.guild_id = $1 AND guild.status = 'ACTIVE'",
    )
    .bind(guild_id)
    .bind(actor_id)
    .fetch_optional(&mut *connection)
    .await?;
    if is_member.is_some() {
        Err(AttemptError::OperatorRequired)
    } else {
        Err(AttemptError::GuildNotFound)
    }
}

async fn fetch_snapshot(
    connection: &mut PgConnection,
    guild_id: &GuildId,
) -> Result<GuildSeasonsSnapshot, AttemptError> {
    let rows = sqlx::query_as::<_, SeasonSummaryRow>(
        "SELECT season.id,
                season.name,
                season.status,
                season.started_at,
                season.ended_at,
                creator.display_name AS created_by,
                COALESCE(participants.participant_count, 0) AS participant_count,
                COALESCE(general.general_series_count, 0) AS general_series_count,
                COALESCE(auction.auction_tournament_count, 0) AS auction_tournament_count,
                COALESCE(auction.auction_ten_count, 0) AS auction_ten_count,
                COALESCE(auction.auction_twenty_count, 0) AS auction_twenty_count,
                COALESCE(game_summary.confirmed_game_count, 0) AS confirmed_game_count,
                game_summary.last_game_at
         FROM seasons AS season
         LEFT JOIN guild_members AS creator
           ON creator.guild_id = season.guild_id
          AND creator.user_id = season.created_by
         LEFT JOIN LATERAL (
             SELECT COUNT(*) AS general_series_count
             FROM series AS normal_series
             WHERE normal_series.guild_id = season.guild_id
               AND normal_series.season_id = season.id
               AND normal_series.status = 'COMPLETED'
               AND normal_series.deleted_at IS NULL
         ) AS general ON true
         LEFT JOIN LATERAL (
             SELECT COUNT(*) AS auction_tournament_count,
                    COUNT(*) FILTER (WHERE tournament.format = 'TEN_PERSON') AS auction_ten_count,
                    COUNT(*) FILTER (WHERE tournament.format = 'TWENTY_PERSON') AS auction_twenty_count
             FROM auction_tournaments AS tournament
             WHERE tournament.guild_id = season.guild_id
               AND tournament.season_id = season.id
               AND tournament.status = 'COMPLETED'
               AND tournament.deleted_at IS NULL
         ) AS auction ON true
         LEFT JOIN LATERAL (
             SELECT COUNT(*) AS confirmed_game_count,
                    MAX(game.confirmed_at) AS last_game_at
             FROM games AS game
             WHERE game.guild_id = season.guild_id
               AND game.season_id = season.id
               AND game.status = 'CONFIRMED'
         ) AS game_summary ON true
         LEFT JOIN LATERAL (
             SELECT COUNT(DISTINCT participant.user_id) AS participant_count
             FROM games AS participant_game
             JOIN game_participants AS participant
               ON participant.guild_id = participant_game.guild_id
              AND participant.game_id = participant_game.id
             WHERE participant_game.guild_id = season.guild_id
               AND participant_game.season_id = season.id
               AND participant_game.status = 'CONFIRMED'
         ) AS participants ON true
         WHERE season.guild_id = $1
         ORDER BY CASE season.status WHEN 'ACTIVE' THEN 0 ELSE 1 END,
                  season.ended_at DESC NULLS FIRST,
                  season.id DESC",
    )
    .bind(guild_id)
    .fetch_all(&mut *connection)
    .await?;

    snapshot_from_rows(rows)
}

fn snapshot_from_rows(rows: Vec<SeasonSummaryRow>) -> Result<GuildSeasonsSnapshot, AttemptError> {
    let mut current = None;
    let mut completed = Vec::new();

    for row in rows {
        let summary = row.into_summary()?;
        match summary.status {
            SeasonStatus::Active => {
                if current.replace(summary).is_some() {
                    return Err(AttemptError::InvariantViolation);
                }
            }
            SeasonStatus::Closed => completed.push(summary),
        }
    }

    Ok(GuildSeasonsSnapshot {
        current: current.ok_or(AttemptError::NoActiveSeason)?,
        completed,
    })
}

async fn lock_active_season(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
) -> Result<LockedSeason, AttemptError> {
    sqlx::query_as::<_, LockedSeason>(
        "SELECT id, name, started_at
         FROM seasons
         WHERE guild_id = $1 AND status = 'ACTIVE'
         FOR UPDATE",
    )
    .bind(guild_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::NoActiveSeason)
}

#[allow(clippy::too_many_arguments)]
async fn insert_audit(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &UserId,
    request_id: &str,
    audit_id: Uuid,
    previous: &LockedSeason,
    current_id: SeasonId,
    current_name: &str,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let before_summary = serde_json::to_value(RotationBefore {
        current: AuditSeason {
            id: previous.id,
            name: &previous.name,
            status: SeasonStatus::Active,
            started_at: previous.started_at,
            ended_at: None,
        },
    })?;
    let after_summary = serde_json::to_value(RotationAfter {
        closed: AuditSeason {
            id: previous.id,
            name: &previous.name,
            status: SeasonStatus::Closed,
            started_at: previous.started_at,
            ended_at: Some(now),
        },
        current: AuditSeason {
            id: current_id,
            name: current_name,
            status: SeasonStatus::Active,
            started_at: now,
            ended_at: None,
        },
    })?;

    sqlx::query(
        "INSERT INTO admin_audit_log (
             guild_id, id, principal_type, principal_id, action, target_type, target_id,
             checkpoint_id, request_id, before_summary, after_summary, result, note, created_at
         ) VALUES (
             $1, $2, 'USER', $3, 'SEASON_ROTATED', 'SEASON', $4,
             NULL, $5, $6, $7, 'APPLIED', NULL, $8
         )",
    )
    .bind(guild_id)
    .bind(audit_id)
    .bind(actor_id)
    .bind(current_id.to_string())
    .bind(request_id)
    .bind(before_summary)
    .bind(after_summary)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn acquire_idempotency<T: DeserializeOwned>(
    transaction: &mut DatabaseTransaction,
    request: IdempotencyAcquire<'_>,
) -> Result<IdempotencyDecision<T>, AttemptError> {
    let command = AcquireIdempotency {
        record_id: request.record_id,
        scope: request.scope,
        principal_type: IdempotencyPrincipalType::User,
        principal_id: request.principal_id,
        method: ROTATE_SEASON_METHOD,
        route_template: ROTATE_SEASON_ROUTE,
        idempotency_key: request.key,
        request_hash: request.request_hash,
        created_at: request.now,
        expires_at: request.now + IDEMPOTENCY_TTL,
    };

    match idempotency::acquire(transaction, &command).await? {
        AcquireIdempotencyOutcome::Acquired { record_id } => {
            Ok(IdempotencyDecision::Acquired(record_id))
        }
        AcquireIdempotencyOutcome::Replay { response, .. } => {
            if response.status_code != 200 {
                return Err(AttemptError::CorruptIdempotentResponse);
            }
            let body = response
                .body
                .ok_or(AttemptError::CorruptIdempotentResponse)?;
            Ok(IdempotencyDecision::Replay(serde_json::from_value(body)?))
        }
        AcquireIdempotencyOutcome::Conflict { .. } => Err(AttemptError::IdempotencyConflict),
        AcquireIdempotencyOutcome::InProgress => Err(AttemptError::IdempotencyInProgress),
    }
}

async fn complete_idempotency(
    transaction: &mut DatabaseTransaction,
    scope: &TenantScope,
    record_id: IdempotencyRecordId,
    snapshot: &GuildSeasonsSnapshot,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let outcome = idempotency::complete(
        transaction,
        CompleteIdempotency {
            scope,
            record_id,
            response: IdempotentResponse {
                status_code: 200,
                body: Some(serde_json::to_value(snapshot)?),
            },
            completed_at: now,
        },
    )
    .await?;
    if outcome != CompleteIdempotencyOutcome::Completed {
        return Err(AttemptError::IdempotencyCompletionLost);
    }
    Ok(())
}

fn validate_command(command: &RotateSeasonCommand) -> Result<(), GuildSeasonsRepositoryError> {
    let name = command.name.trim();
    if name.is_empty() || name.chars().count() > MAX_SEASON_NAME_CHARS {
        Err(GuildSeasonsRepositoryError::InvalidSeasonName)
    } else {
        Ok(())
    }
}

fn request_hash(
    guild_id: &GuildId,
    command: &RotateSeasonCommand,
) -> Result<Vec<u8>, GuildSeasonsRepositoryError> {
    let canonical = serde_json::to_string(&(guild_id, command))?;
    Ok(AuthService::hash_secret(&canonical))
}

fn attempt_sql_error(error: &AttemptError) -> Option<&sqlx::Error> {
    match error {
        AttemptError::Sql(error) => Some(error),
        AttemptError::Idempotency(IdempotencyRepositoryError::Sql(error)) => Some(error),
        _ => None,
    }
}

fn map_transaction_error(
    error: TransactionRetryError<AttemptError>,
) -> GuildSeasonsRepositoryError {
    match error {
        TransactionRetryError::DeadlockRetriesExhausted { .. } => {
            GuildSeasonsRepositoryError::DatabaseBusy
        }
        TransactionRetryError::NonRetryable { failure, .. } => match failure {
            TransactionAttemptError::Begin(error) | TransactionAttemptError::Commit(error) => {
                GuildSeasonsRepositoryError::Database(error)
            }
            TransactionAttemptError::Operation(error) => error.into_public(),
        },
    }
}

#[derive(Debug, FromRow)]
struct SeasonSummaryRow {
    id: SeasonId,
    name: String,
    status: SeasonStatus,
    started_at: OffsetDateTime,
    ended_at: Option<OffsetDateTime>,
    created_by: Option<String>,
    participant_count: i64,
    general_series_count: i64,
    auction_tournament_count: i64,
    auction_ten_count: i64,
    auction_twenty_count: i64,
    confirmed_game_count: i64,
    last_game_at: Option<OffsetDateTime>,
}

impl SeasonSummaryRow {
    fn into_summary(self) -> Result<SeasonSummary, AttemptError> {
        let valid_lifecycle = match self.status {
            SeasonStatus::Active => self.ended_at.is_none(),
            SeasonStatus::Closed => self.ended_at.is_some(),
        };
        let valid_counts = [
            self.participant_count,
            self.general_series_count,
            self.auction_tournament_count,
            self.auction_ten_count,
            self.auction_twenty_count,
            self.confirmed_game_count,
        ]
        .into_iter()
        .all(|count| count >= 0)
            && self.auction_ten_count + self.auction_twenty_count == self.auction_tournament_count;
        if !valid_lifecycle || !valid_counts {
            return Err(AttemptError::InvariantViolation);
        }

        Ok(SeasonSummary {
            id: self.id,
            name: self.name,
            status: self.status,
            started_at: self.started_at,
            ended_at: self.ended_at,
            created_by: self.created_by,
            participant_count: self.participant_count,
            general_series_count: self.general_series_count,
            auction_tournament_count: self.auction_tournament_count,
            auction_ten_count: self.auction_ten_count,
            auction_twenty_count: self.auction_twenty_count,
            confirmed_game_count: self.confirmed_game_count,
            last_game_at: self.last_game_at,
        })
    }
}

#[derive(Debug, FromRow)]
struct LockedSeason {
    id: SeasonId,
    name: String,
    started_at: OffsetDateTime,
}

#[derive(Clone, Copy)]
struct RotatePrepared {
    season_id: SeasonId,
    audit_id: Uuid,
    idempotency_record_id: IdempotencyRecordId,
}

impl RotatePrepared {
    fn new() -> Self {
        Self {
            season_id: SeasonId::new(),
            audit_id: Uuid::now_v7(),
            idempotency_record_id: IdempotencyRecordId::new(),
        }
    }
}

struct IdempotencyAcquire<'a> {
    scope: &'a TenantScope,
    record_id: IdempotencyRecordId,
    principal_id: &'a str,
    key: &'a str,
    request_hash: &'a [u8],
    now: OffsetDateTime,
}

enum IdempotencyDecision<T> {
    Acquired(IdempotencyRecordId),
    Replay(T),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RotationBefore<'a> {
    current: AuditSeason<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RotationAfter<'a> {
    closed: AuditSeason<'a>,
    current: AuditSeason<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AuditSeason<'a> {
    id: SeasonId,
    name: &'a str,
    status: SeasonStatus,
    #[serde(with = "time::serde::rfc3339")]
    started_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    ended_at: Option<OffsetDateTime>,
}

#[derive(Debug, Error)]
enum AttemptError {
    #[error("requested guild was not found in the caller tenant")]
    GuildNotFound,
    #[error("guild operator role is required")]
    OperatorRequired,
    #[error("guild has no active season")]
    NoActiveSeason,
    #[error("active season changed since the caller loaded it")]
    CurrentSeasonConflict {
        expected: SeasonId,
        current: SeasonId,
    },
    #[error("same idempotency key was used with another request")]
    IdempotencyConflict,
    #[error("idempotent request is already in progress")]
    IdempotencyInProgress,
    #[error("idempotent response could not be restored")]
    CorruptIdempotentResponse,
    #[error("idempotency completion was lost")]
    IdempotencyCompletionLost,
    #[error("stored season invariant was violated")]
    InvariantViolation,
    #[error("PostgreSQL season operation failed")]
    Sql(#[from] sqlx::Error),
    #[error("season response serialization failed")]
    Serialization(#[from] serde_json::Error),
    #[error("idempotency operation failed")]
    Idempotency(#[from] IdempotencyRepositoryError),
}

impl AttemptError {
    fn into_public(self) -> GuildSeasonsRepositoryError {
        match self {
            Self::GuildNotFound => GuildSeasonsRepositoryError::GuildNotFound,
            Self::OperatorRequired => GuildSeasonsRepositoryError::OperatorRequired,
            Self::NoActiveSeason => GuildSeasonsRepositoryError::NoActiveSeason,
            Self::CurrentSeasonConflict { expected, current } => {
                GuildSeasonsRepositoryError::CurrentSeasonConflict { expected, current }
            }
            Self::IdempotencyConflict => GuildSeasonsRepositoryError::IdempotencyConflict,
            Self::IdempotencyInProgress => GuildSeasonsRepositoryError::IdempotencyInProgress,
            Self::CorruptIdempotentResponse
            | Self::IdempotencyCompletionLost
            | Self::InvariantViolation => GuildSeasonsRepositoryError::InvariantViolation,
            Self::Sql(error) => GuildSeasonsRepositoryError::Database(error),
            Self::Serialization(error) => GuildSeasonsRepositoryError::Serialization(error),
            Self::Idempotency(IdempotencyRepositoryError::Sql(error)) => {
                GuildSeasonsRepositoryError::Database(error)
            }
            Self::Idempotency(_) => GuildSeasonsRepositoryError::InvariantViolation,
        }
    }
}

#[derive(Debug, Error)]
pub enum GuildSeasonsRepositoryError {
    #[error("requested guild was not found in the caller tenant")]
    GuildNotFound,
    #[error("guild operator role is required")]
    OperatorRequired,
    #[error("guild has no active season")]
    NoActiveSeason,
    #[error("season name must contain between one and one hundred characters")]
    InvalidSeasonName,
    #[error("active season changed since the caller loaded it")]
    CurrentSeasonConflict {
        expected: SeasonId,
        current: SeasonId,
    },
    #[error("same idempotency key was used with another request")]
    IdempotencyConflict,
    #[error("idempotent request is already in progress")]
    IdempotencyInProgress,
    #[error("database remained busy after bounded retries")]
    DatabaseBusy,
    #[error("stored season invariant was violated")]
    InvariantViolation,
    #[error("PostgreSQL season operation failed")]
    Database(#[from] sqlx::Error),
    #[error("season response serialization failed")]
    Serialization(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(name: &str) -> RotateSeasonCommand {
        RotateSeasonCommand {
            name: name.to_owned(),
            expected_current_season_id: SeasonId::new(),
        }
    }

    #[test]
    fn season_names_are_trimmed_and_limited_by_character_count() {
        assert!(validate_command(&command("  2026 시즌 4  ")).is_ok());
        assert!(validate_command(&command("   ")).is_err());
        assert!(validate_command(&command(&"가".repeat(100))).is_ok());
        assert!(validate_command(&command(&"가".repeat(101))).is_err());
    }

    #[test]
    fn request_hash_is_scoped_to_the_guild_and_body() {
        let guild_one = GuildId::parse("200000000000000001").expect("guild one");
        let guild_two = GuildId::parse("200000000000000002").expect("guild two");
        let command = command("2026 시즌 4");

        assert_ne!(
            request_hash(&guild_one, &command).expect("first hash"),
            request_hash(&guild_two, &command).expect("second hash")
        );
        assert_ne!(
            request_hash(&guild_one, &command).expect("first hash"),
            request_hash(
                &guild_one,
                &RotateSeasonCommand {
                    name: "2026 시즌 5".to_owned(),
                    ..command
                }
            )
            .expect("changed body hash")
        );
    }
}
