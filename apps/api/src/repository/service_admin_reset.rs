use ring::digest;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
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
    domain::{GuildId, IdempotencyRecordId, UserId},
    infrastructure::postgres::DatabaseTransaction,
};

const RESET_METHOD: &str = "POST";
const RESET_ROUTE: &str = "/api/admin/guilds/{guild_id}/record-reset";
const IDEMPOTENCY_TTL: Duration = Duration::hours(24);

const RESET_TOTAL_ROWS_SQL: &str = "
    SELECT COALESCE(sum(row_count), 0)::bigint
    FROM (
        SELECT count(*)::bigint AS row_count FROM admin_audit_log WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM revert_checkpoints WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM bot_notification_outbox WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM outbox_events WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM guild_data_imports WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM guild_full_data_imports WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM idempotency_records WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM mmr_ledger_entries WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM game_participant_stats WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM game_bans WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM game_participants WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM games WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM game_input_bans WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM game_input_picks WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM game_inputs WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM auction_match_lineups WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM auction_matches WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM auction_bracket_slots WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM auction_point_ledger WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM auction_team_members WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM auction_actions WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM auction_lots WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM auction_bid_candidates WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM auction_teams WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM auction_tournaments WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM auction_setup_teams WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM auction_setup_drafts WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM auction_recruitment_participants WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM auction_recruitments WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM series_participants WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM series WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM recruitment_entry_assignments WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM recruitment_preferred_lanes WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM recruitment_entry_drafts WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM recruitment_participants WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM recruitments WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM user_lane_mmr WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM guild_counters WHERE guild_id = $1
        UNION ALL SELECT count(*) FROM seasons WHERE guild_id = $1
    ) AS reset_rows";

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildRecordResetCounts {
    pub total_rows: i64,
    pub seasons: i64,
    pub recruitments: i64,
    pub series: i64,
    pub games: i64,
    pub auction_recruitments: i64,
    pub auction_tournaments: i64,
    pub mmr_rows: i64,
    pub import_jobs: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildRecordResetPreview {
    pub guild_id: GuildId,
    pub display_name: String,
    pub member_count: i64,
    pub active_season_name: String,
    pub confirmation: String,
    pub counts: GuildRecordResetCounts,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FreshSeason {
    pub id: Uuid,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildRecordResetResult {
    pub guild_id: GuildId,
    pub display_name: String,
    #[serde(with = "time::serde::rfc3339")]
    pub reset_at: OffsetDateTime,
    pub deleted: GuildRecordResetCounts,
    pub fresh_season: FreshSeason,
}

pub async fn preview(
    pool: &PgPool,
    guild_id: &GuildId,
) -> Result<GuildRecordResetPreview, ServiceAdminResetRepositoryError> {
    let mut transaction = pool.begin().await?;
    let preview = load_preview(&mut transaction, guild_id, false).await?;
    transaction.rollback().await?;
    Ok(preview)
}

pub async fn reset(
    pool: &PgPool,
    guild_id: &GuildId,
    actor_id: &UserId,
    idempotency_key: &str,
) -> Result<GuildRecordResetResult, ServiceAdminResetRepositoryError> {
    let now = OffsetDateTime::now_utc();
    let request_hash = request_hash(guild_id)?;
    let scope = TenantScope::global();
    let mut transaction = pool.begin().await?;
    let acquired = idempotency::acquire(
        &mut transaction,
        &AcquireIdempotency {
            record_id: IdempotencyRecordId::new(),
            scope: &scope,
            principal_type: IdempotencyPrincipalType::User,
            principal_id: actor_id.as_str(),
            method: RESET_METHOD,
            route_template: RESET_ROUTE,
            idempotency_key,
            request_hash: &request_hash,
            created_at: now,
            expires_at: now + IDEMPOTENCY_TTL,
        },
    )
    .await?;
    let idempotency_record_id = match acquired {
        AcquireIdempotencyOutcome::Acquired { record_id } => record_id,
        AcquireIdempotencyOutcome::Replay { response, .. } => {
            let result = response
                .body
                .ok_or(ServiceAdminResetRepositoryError::InvariantViolation)
                .and_then(|value| {
                    serde_json::from_value(value)
                        .map_err(ServiceAdminResetRepositoryError::Serialization)
                })?;
            transaction.rollback().await?;
            return Ok(result);
        }
        AcquireIdempotencyOutcome::Conflict { .. } => {
            return Err(ServiceAdminResetRepositoryError::IdempotencyConflict);
        }
        AcquireIdempotencyOutcome::InProgress => {
            return Err(ServiceAdminResetRepositoryError::IdempotencyInProgress);
        }
    };

    let preview = load_preview(&mut transaction, guild_id, true).await?;
    let fresh_season_creator = preview_target_creator(&mut transaction, guild_id).await?;
    let deleted_rows =
        sqlx::query_scalar::<_, i64>("SELECT public.service_admin_reset_guild_records($1)")
            .bind(guild_id)
            .fetch_one(&mut *transaction)
            .await?;

    sqlx::query(
        "INSERT INTO guild_admin_state (guild_id, scope, revision, updated_by, updated_at)
         SELECT $1, scope, 1, NULL, $2
         FROM unnest(ARRAY['MEMBERS', 'OPERATIONS']) AS scope
         ON CONFLICT (guild_id, scope) DO UPDATE
         SET revision = guild_admin_state.revision + 1,
             updated_by = NULL,
             updated_at = EXCLUDED.updated_at",
    )
    .bind(guild_id)
    .bind(now)
    .execute(&mut *transaction)
    .await?;

    let fresh_season_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO seasons (
             guild_id, id, name, status, started_at, ended_at, created_by, created_at
         ) VALUES ($1, $2, $3, 'ACTIVE', $4, NULL, $5, $4)",
    )
    .bind(guild_id)
    .bind(fresh_season_id)
    .bind(&preview.active_season_name)
    .bind(now)
    .bind(&fresh_season_creator)
    .execute(&mut *transaction)
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
        .bind(guild_id)
        .bind(counter_key)
        .execute(&mut *transaction)
        .await?;
    }

    let mut deleted = preview.counts;
    deleted.total_rows = deleted_rows;
    let result = GuildRecordResetResult {
        guild_id: guild_id.clone(),
        display_name: preview.display_name,
        reset_at: now,
        deleted,
        fresh_season: FreshSeason {
            id: fresh_season_id,
            name: preview.active_season_name,
        },
    };
    let completed = idempotency::complete(
        &mut transaction,
        CompleteIdempotency {
            scope: &scope,
            record_id: idempotency_record_id,
            response: IdempotentResponse {
                status_code: 200,
                body: Some(serde_json::to_value(&result)?),
            },
            completed_at: now,
        },
    )
    .await?;
    if completed != CompleteIdempotencyOutcome::Completed {
        return Err(ServiceAdminResetRepositoryError::IdempotencyCompletionLost);
    }
    transaction.commit().await?;
    Ok(result)
}

async fn load_preview(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    lock_guild: bool,
) -> Result<GuildRecordResetPreview, ServiceAdminResetRepositoryError> {
    let query = if lock_guild {
        "SELECT guild.discord_name,
                COALESCE(config.display_name, guild.discord_name) AS display_name,
                guild.status
         FROM guilds AS guild
         LEFT JOIN guild_config AS config ON config.guild_id = guild.guild_id
         WHERE guild.guild_id = $1
         FOR UPDATE OF guild"
    } else {
        "SELECT guild.discord_name,
                COALESCE(config.display_name, guild.discord_name) AS display_name,
                guild.status
         FROM guilds AS guild
         LEFT JOIN guild_config AS config ON config.guild_id = guild.guild_id
         WHERE guild.guild_id = $1"
    };
    let target = sqlx::query_as::<_, ResetTargetRow>(query)
        .bind(guild_id)
        .fetch_optional(&mut **transaction)
        .await?
        .ok_or(ServiceAdminResetRepositoryError::GuildNotFound)?;
    if target.status != "ACTIVE" {
        return Err(ServiceAdminResetRepositoryError::GuildInactive);
    }
    let season = sqlx::query_as::<_, ActiveSeasonRow>(
        "SELECT id, name, created_by
         FROM seasons
         WHERE guild_id = $1 AND status = 'ACTIVE'",
    )
    .bind(guild_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(ServiceAdminResetRepositoryError::ActiveSeasonMissing)?;
    let counts = load_counts(transaction, guild_id).await?;
    let member_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM guild_members WHERE guild_id = $1 AND status = 'ACTIVE'",
    )
    .bind(guild_id)
    .fetch_one(&mut **transaction)
    .await?;

    // Keep these rows live until reset inserts the replacement season. This also validates the
    // old creator reference before any destructive work begins.
    let _ = (target.discord_name, season.id, season.created_by);
    Ok(GuildRecordResetPreview {
        guild_id: guild_id.clone(),
        display_name: target.display_name,
        member_count,
        active_season_name: season.name,
        confirmation: format!("RESET {}", guild_id.as_str()),
        counts,
    })
}

async fn load_counts(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
) -> Result<GuildRecordResetCounts, ServiceAdminResetRepositoryError> {
    let row = sqlx::query_as::<_, ResetCountRow>(
        "SELECT
             (SELECT count(*) FROM seasons WHERE guild_id = $1) AS seasons,
             (SELECT count(*) FROM recruitments WHERE guild_id = $1) AS recruitments,
             (SELECT count(*) FROM series WHERE guild_id = $1) AS series,
             (SELECT count(*) FROM games WHERE guild_id = $1) AS games,
             (SELECT count(*) FROM auction_recruitments WHERE guild_id = $1) AS auction_recruitments,
             (SELECT count(*) FROM auction_tournaments WHERE guild_id = $1) AS auction_tournaments,
             ((SELECT count(*) FROM user_lane_mmr WHERE guild_id = $1)
                + (SELECT count(*) FROM mmr_ledger_entries WHERE guild_id = $1)) AS mmr_rows,
             ((SELECT count(*) FROM guild_data_imports WHERE guild_id = $1)
                + (SELECT count(*) FROM guild_full_data_imports WHERE guild_id = $1)) AS import_jobs",
    )
    .bind(guild_id)
    .fetch_one(&mut **transaction)
    .await?;
    let total_rows = sqlx::query_scalar::<_, i64>(RESET_TOTAL_ROWS_SQL)
        .bind(guild_id)
        .fetch_one(&mut **transaction)
        .await?;
    Ok(GuildRecordResetCounts {
        total_rows,
        seasons: row.seasons,
        recruitments: row.recruitments,
        series: row.series,
        games: row.games,
        auction_recruitments: row.auction_recruitments,
        auction_tournaments: row.auction_tournaments,
        mmr_rows: row.mmr_rows,
        import_jobs: row.import_jobs,
    })
}

async fn preview_target_creator(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
) -> Result<Option<UserId>, ServiceAdminResetRepositoryError> {
    Ok(sqlx::query_scalar::<_, Option<UserId>>(
        "SELECT created_by
         FROM seasons
         WHERE guild_id = $1 AND status = 'ACTIVE'",
    )
    .bind(guild_id)
    .fetch_optional(&mut **transaction)
    .await?
    .flatten())
}

fn request_hash(guild_id: &GuildId) -> Result<Vec<u8>, ServiceAdminResetRepositoryError> {
    let bytes = serde_json::to_vec(&(RESET_ROUTE, guild_id))?;
    Ok(digest::digest(&digest::SHA256, &bytes).as_ref().to_vec())
}

#[derive(Debug, FromRow)]
struct ResetTargetRow {
    discord_name: String,
    display_name: String,
    status: String,
}

#[derive(Debug, FromRow)]
struct ActiveSeasonRow {
    id: Uuid,
    name: String,
    created_by: Option<UserId>,
}

#[derive(Debug, FromRow)]
struct ResetCountRow {
    seasons: i64,
    recruitments: i64,
    series: i64,
    games: i64,
    auction_recruitments: i64,
    auction_tournaments: i64,
    mmr_rows: i64,
    import_jobs: i64,
}

#[derive(Debug, Error)]
pub enum ServiceAdminResetRepositoryError {
    #[error("PostgreSQL service-admin reset operation failed")]
    Database(#[from] sqlx::Error),
    #[error("service-admin reset serialization failed")]
    Serialization(#[from] serde_json::Error),
    #[error("service-admin reset idempotency operation failed")]
    Idempotency(#[from] IdempotencyRepositoryError),
    #[error("guild was not found")]
    GuildNotFound,
    #[error("guild is not active")]
    GuildInactive,
    #[error("active season is missing")]
    ActiveSeasonMissing,
    #[error("same idempotency key was reused for another reset")]
    IdempotencyConflict,
    #[error("same reset is already in progress")]
    IdempotencyInProgress,
    #[error("idempotency completion was lost")]
    IdempotencyCompletionLost,
    #[error("stored service-admin reset invariant was violated")]
    InvariantViolation,
}
