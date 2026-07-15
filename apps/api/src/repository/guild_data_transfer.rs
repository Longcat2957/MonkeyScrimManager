use std::collections::{HashMap, HashSet};

use ring::digest;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
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
    mmr::{self, MmrLaneKey, MmrRepositoryError},
};
use crate::{
    domain::{DiscordUserId, GuildId, IdempotencyRecordId, Lane, SeasonId, UserId},
    infrastructure::postgres::{
        DatabaseTransaction, DeadlockRetryPolicy, TransactionAttemptError, TransactionRetryError,
        run_with_deadlock_retry,
    },
};

const ARCHIVE_FORMAT: &str = "MONKEYBOT_DATA_TRANSFER";
const ARCHIVE_KIND: &str = "MEMBER_SEASON_MMR_SEED";
const ARCHIVE_SCHEMA_VERSION: u16 = 1;
const MAX_ARCHIVE_MEMBERS: usize = 1_000;
const MAX_TEXT_CHARS: usize = 100;
const PREVIEW_TTL: Duration = Duration::minutes(15);
const IDEMPOTENCY_TTL: Duration = Duration::hours(24);
const APPLY_METHOD: &str = "POST";
const APPLY_ROUTE: &str = "/api/guilds/{guildId}/data-imports/{importId}/apply";
const MMR_EPSILON: f64 = 0.000_001;

const CANONICAL_LANES: [Lane; 5] = [
    Lane::Top,
    Lane::Jungle,
    Lane::Mid,
    Lane::Bottom,
    Lane::Support,
];

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemberSeasonMmrSeedArchive {
    pub format: String,
    pub kind: String,
    pub schema_version: u16,
    #[serde(with = "time::serde::rfc3339")]
    pub exported_at: OffsetDateTime,
    pub source: ArchiveSource,
    pub members: Vec<ArchiveMember>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArchiveSource {
    pub guild_id: GuildId,
    pub guild_name: String,
    pub season: ArchiveSeason,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArchiveSeason {
    pub id: Uuid,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArchiveMember {
    pub discord_id: DiscordUserId,
    pub display_name: String,
    pub lanes: Vec<ArchiveLaneMmr>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArchiveLaneMmr {
    pub lane: Lane,
    pub mmr: f64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataExport {
    pub archive: MemberSeasonMmrSeedArchive,
    pub file_name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplyImportCommand {
    pub expected_digest: String,
    pub expected_target_revision: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportIssue {
    pub reason: String,
    pub message: String,
    pub discord_id: DiscordUserId,
    pub display_name: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub archive_members: usize,
    pub matched_members: usize,
    pub lane_entries: usize,
    pub changes: usize,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub import_id: Uuid,
    pub digest: String,
    #[serde(with = "time::serde::rfc3339")]
    pub expires_at: OffsetDateTime,
    pub source: ArchiveSource,
    pub summary: ImportSummary,
    pub conflicts: Vec<ImportIssue>,
    pub target_revision: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportApplyResult {
    pub import_id: Uuid,
    pub status: String,
    #[serde(with = "time::serde::rfc3339")]
    pub applied_at: OffsetDateTime,
    pub summary: ImportSummary,
    pub target_revision: i64,
}

pub async fn export(
    pool: &PgPool,
    guild_id: &GuildId,
    actor_id: &UserId,
    request_id: &str,
) -> Result<DataExport, GuildDataTransferRepositoryError> {
    let now = OffsetDateTime::now_utc();
    let mut transaction = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *transaction)
        .await?;
    authorize_operator(&mut transaction, guild_id, actor_id).await?;
    let season = active_season(&mut transaction, guild_id).await?;
    let rows = export_rows(&mut transaction, guild_id, season.id).await?;
    let members = build_archive_members(rows)?;
    let archive = MemberSeasonMmrSeedArchive {
        format: ARCHIVE_FORMAT.to_owned(),
        kind: ARCHIVE_KIND.to_owned(),
        schema_version: ARCHIVE_SCHEMA_VERSION,
        exported_at: now,
        source: ArchiveSource {
            guild_id: guild_id.clone(),
            guild_name: guild_display_name(&mut transaction, guild_id).await?,
            season: ArchiveSeason {
                id: season.id,
                name: season.name.clone(),
            },
        },
        members,
    };
    validate_archive(&archive)?;
    insert_audit(
        &mut transaction,
        guild_id,
        actor_id,
        "DATA_TRANSFER_EXPORTED",
        "MEMBER_SEASON_MMR_SEED",
        &season.id.to_string(),
        request_id,
        json!({
            "archiveMembers": archive.members.len(),
            "seasonId": season.id,
        }),
        json!({
            "schemaVersion": ARCHIVE_SCHEMA_VERSION,
            "kind": ARCHIVE_KIND,
        }),
        now,
    )
    .await?;
    transaction.commit().await?;

    Ok(DataExport {
        file_name: format!("monkeybot-mmr-{}-{}.json", guild_id.as_str(), season.id),
        archive,
    })
}

pub async fn preview(
    pool: &PgPool,
    guild_id: &GuildId,
    actor_id: &UserId,
    archive_value: Value,
    request_id: &str,
) -> Result<ImportPreview, GuildDataTransferRepositoryError> {
    let archive: MemberSeasonMmrSeedArchive = serde_json::from_value(archive_value)
        .map_err(|_| GuildDataTransferRepositoryError::InvalidArchive)?;
    validate_archive(&archive)?;
    let digest = archive_digest(&archive)?;
    let now = OffsetDateTime::now_utc();
    let expires_at = now + PREVIEW_TTL;

    let mut transaction = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *transaction)
        .await?;
    authorize_operator(&mut transaction, guild_id, actor_id).await?;
    let season = active_season(&mut transaction, guild_id).await?;
    let target_revision = members_revision(&mut transaction, guild_id).await?;
    let analysis = analyze_archive(&mut transaction, guild_id, season.id, &archive).await?;
    let import_id = Uuid::now_v7();
    let archive_payload = serde_json::to_value(&archive)?;
    let summary_payload = serde_json::to_value(&analysis.summary)?;
    let conflict_count = i32::try_from(analysis.conflicts.len())
        .map_err(|_| GuildDataTransferRepositoryError::InvariantViolation)?;
    sqlx::query(
        "INSERT INTO guild_data_imports (
             guild_id, id, actor_id, status, archive_digest, archive_payload, source_guild_id,
             source_season_id, target_season_id, target_members_revision, target_mmr_digest,
             summary, conflict_count, request_id, created_at, expires_at, applied_at
         ) VALUES (
             $1, $2, $3, 'PREVIEWED', $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, NULL
         )",
    )
    .bind(guild_id)
    .bind(import_id)
    .bind(actor_id)
    .bind(&digest)
    .bind(archive_payload)
    .bind(&archive.source.guild_id)
    .bind(archive.source.season.id)
    .bind(season.id)
    .bind(target_revision)
    .bind(&analysis.target_mmr_digest)
    .bind(summary_payload)
    .bind(conflict_count)
    .bind(request_id)
    .bind(now)
    .bind(expires_at)
    .execute(&mut *transaction)
    .await?;
    insert_audit(
        &mut transaction,
        guild_id,
        actor_id,
        "DATA_IMPORT_PREVIEWED",
        "DATA_IMPORT",
        &import_id.to_string(),
        request_id,
        json!({
            "digest": digest_hex(&digest),
            "sourceGuildId": archive.source.guild_id,
            "sourceSeasonId": archive.source.season.id,
        }),
        json!({
            "archiveMembers": analysis.summary.archive_members,
            "matchedMembers": analysis.summary.matched_members,
            "changes": analysis.summary.changes,
            "conflictCount": analysis.conflicts.len(),
        }),
        now,
    )
    .await?;
    transaction.commit().await?;

    Ok(ImportPreview {
        import_id,
        digest: digest_hex(&digest),
        expires_at,
        source: archive.source,
        summary: analysis.summary,
        conflicts: analysis.conflicts,
        target_revision,
    })
}

/// Deletes expired previews whose raw archive payload is no longer eligible for application.
/// Applied rows retain only their audit-safe metadata and are not touched here.
pub async fn cleanup_expired_previews(
    pool: &PgPool,
    now: OffsetDateTime,
) -> Result<u64, GuildDataTransferRepositoryError> {
    let result = sqlx::query(
        "DELETE FROM guild_data_imports
         WHERE status = 'PREVIEWED' AND expires_at <= $1",
    )
    .bind(now)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

#[allow(clippy::too_many_arguments)]
pub async fn apply(
    pool: &PgPool,
    guild_id: &GuildId,
    actor_id: &UserId,
    import_id: Uuid,
    idempotency_key: &str,
    request_id: &str,
    command: ApplyImportCommand,
) -> Result<ImportApplyResult, GuildDataTransferRepositoryError> {
    validate_apply_command(&command)?;
    let request_hash = request_hash(guild_id, import_id, &command)?;
    let prepared = ApplyPrepared {
        idempotency_record_id: IdempotencyRecordId::new(),
    };
    let guild_id = guild_id.clone();
    let actor_id = actor_id.clone();
    let idempotency_key = idempotency_key.to_owned();
    let request_id = request_id.to_owned();

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
                apply_attempt(
                    transaction,
                    &guild_id,
                    &actor_id,
                    import_id,
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
async fn apply_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &UserId,
    import_id: Uuid,
    idempotency_key: &str,
    request_id: &str,
    request_hash: &[u8],
    command: &ApplyImportCommand,
    prepared: ApplyPrepared,
) -> Result<ImportApplyResult, AttemptError> {
    let now = OffsetDateTime::now_utc();
    authorize_operator(&mut *transaction, guild_id, actor_id).await?;
    let scope = TenantScope::guild(guild_id.clone());
    let idempotency_record_id = match acquire_idempotency::<ImportApplyResult>(
        transaction,
        &scope,
        prepared.idempotency_record_id,
        actor_id,
        idempotency_key,
        request_hash,
        now,
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };

    let import = lock_import(transaction, guild_id, import_id).await?;
    if import.actor_id != *actor_id {
        return Err(AttemptError::PreviewActorMismatch);
    }
    if import.status != "PREVIEWED" {
        return Err(AttemptError::PreviewAlreadyApplied);
    }
    if import.expires_at <= now {
        return Err(AttemptError::PreviewExpired);
    }
    let expected_digest = parse_digest(&command.expected_digest)?;
    if expected_digest != import.archive_digest {
        return Err(AttemptError::PreviewDigestMismatch);
    }
    if import.conflict_count > 0 {
        return Err(AttemptError::PreviewConflicts);
    }
    let archive_payload = import
        .archive_payload
        .ok_or(AttemptError::InvariantViolation)?;
    let archive: MemberSeasonMmrSeedArchive =
        serde_json::from_value(archive_payload).map_err(|_| AttemptError::InvariantViolation)?;
    validate_archive(&archive)?;
    if archive_digest(&archive)? != import.archive_digest {
        return Err(AttemptError::PreviewDigestMismatch);
    }

    let state = ensure_and_lock_members_state(transaction, guild_id, actor_id, now).await?;
    if command.expected_target_revision != import.target_members_revision
        || state.revision != import.target_members_revision
    {
        return Err(AttemptError::RevisionConflict {
            expected: command.expected_target_revision,
            current: state.revision,
        });
    }
    let season = lock_active_season(transaction, guild_id).await?;
    if season.id != import.target_season_id {
        return Err(AttemptError::TargetSeasonChanged);
    }
    if has_active_workflow(transaction, guild_id).await? {
        return Err(AttemptError::ActiveWorkflowConflict);
    }
    let analysis = analyze_archive(&mut *transaction, guild_id, season.id, &archive).await?;
    if !analysis.conflicts.is_empty() {
        return Err(AttemptError::PreviewConflicts);
    }
    if analysis.summary.changes == 0 {
        return Err(AttemptError::NoChanges);
    }
    let stored_summary: ImportSummary =
        serde_json::from_value(import.summary).map_err(|_| AttemptError::InvariantViolation)?;
    if analysis.summary != stored_summary {
        return Err(AttemptError::RevisionConflict {
            expected: import.target_members_revision,
            current: state.revision,
        });
    }

    let desired = desired_mmr(&archive);
    let keys = desired
        .keys()
        .map(|(user_id, lane)| {
            Ok(MmrLaneKey {
                user_id: UserId::parse(user_id.clone())
                    .map_err(|_| AttemptError::InvariantViolation)?,
                lane: *lane,
            })
        })
        .collect::<Result<Vec<_>, AttemptError>>()?;
    let locked = mmr::ensure_and_lock_mmr_rows(
        transaction,
        guild_id,
        SeasonId::from_uuid(season.id),
        keys,
        now,
    )
    .await?;
    let current_mmr_digest = target_mmr_digest_from_locked(season.id, &locked)?;
    if current_mmr_digest != import.target_mmr_digest {
        return Err(AttemptError::MmrStateChanged);
    }
    let mut mmr_changes = Vec::new();
    for current in locked {
        let desired_mmr = desired
            .get(&(current.key.user_id.as_str().to_owned(), current.key.lane))
            .copied()
            .ok_or(AttemptError::InvariantViolation)?;
        let delta = checked_mmr_delta(current.mmr, desired_mmr)?;
        if delta.abs() <= MMR_EPSILON {
            continue;
        }
        let lane_revision = current
            .revision
            .checked_add(1)
            .ok_or(AttemptError::InvariantViolation)?;
        mmr_changes.push(PendingMmrChange {
            ledger_id: Uuid::now_v7(),
            user_id: current.key.user_id,
            lane: current.key.lane,
            lane_revision,
            mmr_before: current.mmr,
            mmr_after: desired_mmr,
            delta,
        });
    }
    if mmr_changes.len() != analysis.summary.changes {
        return Err(AttemptError::InvariantViolation);
    }
    persist_mmr_changes(
        transaction,
        guild_id,
        season.id,
        import_id,
        actor_id,
        request_id,
        now,
        &mmr_changes,
    )
    .await?;
    let target_revision =
        increment_members_revision(transaction, guild_id, actor_id, state.revision, now).await?;
    let marked = sqlx::query(
        "UPDATE guild_data_imports
         SET status = 'APPLIED', archive_payload = NULL, applied_at = $1
         WHERE guild_id = $2 AND id = $3 AND status = 'PREVIEWED'",
    )
    .bind(now)
    .bind(guild_id)
    .bind(import_id)
    .execute(&mut **transaction)
    .await?;
    if marked.rows_affected() != 1 {
        return Err(AttemptError::InvariantViolation);
    }
    insert_audit_attempt(
        transaction,
        guild_id,
        actor_id,
        "DATA_IMPORT_APPLIED",
        "DATA_IMPORT",
        &import_id.to_string(),
        request_id,
        json!({
            "digest": command.expected_digest,
            "targetRevision": state.revision,
        }),
        json!({
            "changes": mmr_changes.len(),
            "targetRevision": target_revision,
        }),
        now,
    )
    .await?;
    let result = ImportApplyResult {
        import_id,
        status: "APPLIED".to_owned(),
        applied_at: now,
        summary: analysis.summary,
        target_revision,
    };
    complete_idempotency(transaction, &scope, idempotency_record_id, &result, now).await?;
    Ok(result)
}

fn validate_apply_command(
    command: &ApplyImportCommand,
) -> Result<(), GuildDataTransferRepositoryError> {
    if command.expected_target_revision < 0 || parse_digest(&command.expected_digest).is_err() {
        Err(GuildDataTransferRepositoryError::InvalidApplyCommand)
    } else {
        Ok(())
    }
}

fn validate_archive(
    archive: &MemberSeasonMmrSeedArchive,
) -> Result<(), GuildDataTransferRepositoryError> {
    if archive.format != ARCHIVE_FORMAT
        || archive.kind != ARCHIVE_KIND
        || archive.schema_version != ARCHIVE_SCHEMA_VERSION
        || archive.members.len() > MAX_ARCHIVE_MEMBERS
        || !valid_text(&archive.source.guild_name)
        || !valid_text(&archive.source.season.name)
    {
        return Err(GuildDataTransferRepositoryError::InvalidArchive);
    }
    let mut members = HashSet::new();
    for member in &archive.members {
        if !members.insert(member.discord_id.as_str()) || !valid_text(&member.display_name) {
            return Err(GuildDataTransferRepositoryError::InvalidArchive);
        }
        if member.lanes.len() != CANONICAL_LANES.len() {
            return Err(GuildDataTransferRepositoryError::InvalidArchive);
        }
        let mut lanes = HashSet::new();
        for lane in &member.lanes {
            if !lane.mmr.is_finite() || !lanes.insert(lane.lane) {
                return Err(GuildDataTransferRepositoryError::InvalidArchive);
            }
        }
        if !CANONICAL_LANES.iter().all(|lane| lanes.contains(lane)) {
            return Err(GuildDataTransferRepositoryError::InvalidArchive);
        }
    }
    Ok(())
}

fn valid_text(value: &str) -> bool {
    let length = value.trim().chars().count();
    (1..=MAX_TEXT_CHARS).contains(&length)
}

fn archive_digest(
    archive: &MemberSeasonMmrSeedArchive,
) -> Result<Vec<u8>, GuildDataTransferRepositoryError> {
    let bytes = serde_json::to_vec(archive)?;
    Ok(digest::digest(&digest::SHA256, &bytes).as_ref().to_vec())
}

fn digest_hex(digest: &[u8]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn parse_digest(value: &str) -> Result<Vec<u8>, GuildDataTransferRepositoryError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(GuildDataTransferRepositoryError::InvalidApplyCommand);
    }
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| GuildDataTransferRepositoryError::InvalidApplyCommand)
}

async fn authorize_operator(
    connection: &mut PgConnection,
    guild_id: &GuildId,
    actor_id: &UserId,
) -> Result<(), GuildDataTransferRepositoryError> {
    let operator = sqlx::query_scalar::<_, bool>(
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
    if operator.is_some() {
        return Ok(());
    }
    let member = sqlx::query_scalar::<_, bool>(
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
    if member.is_some() {
        Err(GuildDataTransferRepositoryError::OperatorRequired)
    } else {
        Err(GuildDataTransferRepositoryError::GuildNotFound)
    }
}

async fn active_season(
    connection: &mut PgConnection,
    guild_id: &GuildId,
) -> Result<SeasonRow, GuildDataTransferRepositoryError> {
    sqlx::query_as::<_, SeasonRow>(
        "SELECT id, name
         FROM seasons
         WHERE guild_id = $1 AND status = 'ACTIVE'",
    )
    .bind(guild_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(GuildDataTransferRepositoryError::NoActiveSeason)
}

async fn lock_active_season(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
) -> Result<SeasonRow, AttemptError> {
    sqlx::query_as::<_, SeasonRow>(
        "SELECT id, name
         FROM seasons
         WHERE guild_id = $1 AND status = 'ACTIVE'
         FOR UPDATE",
    )
    .bind(guild_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::NoActiveSeason)
}

async fn guild_display_name(
    connection: &mut PgConnection,
    guild_id: &GuildId,
) -> Result<String, GuildDataTransferRepositoryError> {
    sqlx::query_scalar::<_, String>("SELECT display_name FROM guild_config WHERE guild_id = $1")
        .bind(guild_id)
        .fetch_optional(&mut *connection)
        .await?
        .filter(|name| valid_text(name))
        .ok_or(GuildDataTransferRepositoryError::InvariantViolation)
}

async fn export_rows(
    connection: &mut PgConnection,
    guild_id: &GuildId,
    season_id: Uuid,
) -> Result<Vec<ExportLaneRow>, GuildDataTransferRepositoryError> {
    sqlx::query_as::<_, ExportLaneRow>(
        "WITH canonical_lanes (lane, lane_order) AS (
             VALUES ('TOP'::text, 1), ('JUNGLE', 2), ('MID', 3), ('BOTTOM', 4), ('SUPPORT', 5)
         )
         SELECT member.user_id, member.display_name, canonical_lanes.lane AS lane,
                COALESCE(lane_mmr.mmr, 1500::double precision) AS mmr
         FROM guild_members AS member
         JOIN users AS app_user
           ON app_user.user_id = member.user_id AND app_user.identity_kind = 'DISCORD'
         CROSS JOIN canonical_lanes
         LEFT JOIN user_lane_mmr AS lane_mmr
           ON lane_mmr.guild_id = member.guild_id
          AND lane_mmr.season_id = $2
          AND lane_mmr.user_id = member.user_id
          AND lane_mmr.lane = canonical_lanes.lane
         WHERE member.guild_id = $1 AND member.status = 'ACTIVE'
         ORDER BY member.user_id, canonical_lanes.lane_order",
    )
    .bind(guild_id)
    .bind(season_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(Into::into)
}

fn build_archive_members(
    rows: Vec<ExportLaneRow>,
) -> Result<Vec<ArchiveMember>, GuildDataTransferRepositoryError> {
    let mut members = Vec::new();
    let mut current: Option<ArchiveMember> = None;
    for row in rows {
        let discord_id = DiscordUserId::parse(row.user_id)
            .map_err(|_| GuildDataTransferRepositoryError::InvariantViolation)?;
        match current.as_mut() {
            Some(member) if member.discord_id == discord_id => {
                member.lanes.push(ArchiveLaneMmr {
                    lane: row.lane,
                    mmr: row.mmr,
                });
            }
            Some(_) => {
                let previous = current
                    .take()
                    .ok_or(GuildDataTransferRepositoryError::InvariantViolation)?;
                members.push(previous);
                current = Some(ArchiveMember {
                    discord_id,
                    display_name: row.display_name,
                    lanes: vec![ArchiveLaneMmr {
                        lane: row.lane,
                        mmr: row.mmr,
                    }],
                });
            }
            None => {
                current = Some(ArchiveMember {
                    discord_id,
                    display_name: row.display_name,
                    lanes: vec![ArchiveLaneMmr {
                        lane: row.lane,
                        mmr: row.mmr,
                    }],
                });
            }
        }
    }
    if let Some(member) = current {
        members.push(member);
    }
    if members.len() > MAX_ARCHIVE_MEMBERS {
        return Err(GuildDataTransferRepositoryError::ArchiveMemberLimitExceeded);
    }
    Ok(members)
}

async fn members_revision(
    connection: &mut PgConnection,
    guild_id: &GuildId,
) -> Result<i64, GuildDataTransferRepositoryError> {
    sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(state.revision, 0)
         FROM guilds AS guild
         LEFT JOIN guild_admin_state AS state
           ON state.guild_id = guild.guild_id AND state.scope = 'MEMBERS'
         WHERE guild.guild_id = $1 AND guild.status = 'ACTIVE'",
    )
    .bind(guild_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(GuildDataTransferRepositoryError::GuildNotFound)
}

async fn analyze_archive(
    connection: &mut PgConnection,
    guild_id: &GuildId,
    season_id: Uuid,
    archive: &MemberSeasonMmrSeedArchive,
) -> Result<ArchiveAnalysis, GuildDataTransferRepositoryError> {
    let ids = archive
        .members
        .iter()
        .map(|member| member.discord_id.as_str().to_owned())
        .collect::<Vec<_>>();
    let active_ids = sqlx::query_scalar::<_, String>(
        "SELECT member.user_id
         FROM guild_members AS member
         JOIN users AS app_user
           ON app_user.user_id = member.user_id AND app_user.identity_kind = 'DISCORD'
         WHERE member.guild_id = $1
           AND member.status = 'ACTIVE'
           AND member.user_id = ANY($2::text[])",
    )
    .bind(guild_id)
    .bind(&ids)
    .fetch_all(&mut *connection)
    .await?;
    let active_ids = active_ids.into_iter().collect::<HashSet<_>>();
    let mut conflicts = Vec::new();
    for member in &archive.members {
        if !active_ids.contains(member.discord_id.as_str()) {
            conflicts.push(ImportIssue {
                reason: "member_not_active".to_owned(),
                message: "대상 길드에서 활성 Discord 멤버를 찾을 수 없습니다.".to_owned(),
                discord_id: member.discord_id.clone(),
                display_name: member.display_name.clone(),
            });
        }
    }
    let current_rows = sqlx::query_as::<_, ExistingMmrRow>(
        "SELECT user_id, lane, mmr
         FROM user_lane_mmr
         WHERE guild_id = $1 AND season_id = $2 AND user_id = ANY($3::text[])",
    )
    .bind(guild_id)
    .bind(season_id)
    .bind(&ids)
    .fetch_all(&mut *connection)
    .await?;
    let current = current_rows
        .into_iter()
        .map(|row| ((row.user_id, row.lane), row.mmr))
        .collect::<HashMap<_, _>>();
    let target_mmr_digest =
        target_mmr_digest_from_current(season_id, archive, &active_ids, &current)?;
    let mut changes = 0usize;
    for member in &archive.members {
        if !active_ids.contains(member.discord_id.as_str()) {
            continue;
        }
        for lane in &member.lanes {
            let previous = current
                .get(&(member.discord_id.as_str().to_owned(), lane.lane))
                .copied()
                .unwrap_or(1500.0);
            checked_mmr_delta(previous, lane.mmr)?;
            if is_mmr_changed(previous, lane.mmr) {
                changes += 1;
            }
        }
    }
    Ok(ArchiveAnalysis {
        summary: ImportSummary {
            archive_members: archive.members.len(),
            matched_members: archive.members.len() - conflicts.len(),
            lane_entries: archive.members.len() * CANONICAL_LANES.len(),
            changes,
        },
        conflicts,
        target_mmr_digest,
    })
}

fn desired_mmr(archive: &MemberSeasonMmrSeedArchive) -> HashMap<(String, Lane), f64> {
    archive
        .members
        .iter()
        .flat_map(|member| {
            member.lanes.iter().map(move |entry| {
                (
                    (member.discord_id.as_str().to_owned(), entry.lane),
                    entry.mmr,
                )
            })
        })
        .collect()
}

fn is_mmr_changed(left: f64, right: f64) -> bool {
    (left - right).abs() > MMR_EPSILON
}

fn checked_mmr_delta(
    current_mmr: f64,
    desired_mmr: f64,
) -> Result<f64, GuildDataTransferRepositoryError> {
    let delta = desired_mmr - current_mmr;
    if delta.is_finite() {
        Ok(delta)
    } else {
        Err(GuildDataTransferRepositoryError::InvalidMmrTransfer)
    }
}

fn target_mmr_digest_from_current(
    season_id: Uuid,
    archive: &MemberSeasonMmrSeedArchive,
    active_ids: &HashSet<String>,
    current: &HashMap<(String, Lane), f64>,
) -> Result<Vec<u8>, GuildDataTransferRepositoryError> {
    let entries = archive
        .members
        .iter()
        .filter(|member| active_ids.contains(member.discord_id.as_str()))
        .flat_map(|member| {
            CANONICAL_LANES.into_iter().map(move |lane| {
                let user_id = member.discord_id.as_str().to_owned();
                let mmr = current
                    .get(&(user_id.clone(), lane))
                    .copied()
                    .unwrap_or(1500.0);
                (user_id, lane, mmr)
            })
        })
        .collect();
    target_mmr_digest(season_id, entries)
}

fn target_mmr_digest_from_locked(
    season_id: Uuid,
    locked: &[mmr::LockedMmrRow],
) -> Result<Vec<u8>, GuildDataTransferRepositoryError> {
    let entries = locked
        .iter()
        .map(|entry| {
            (
                entry.key.user_id.as_str().to_owned(),
                entry.key.lane,
                entry.mmr,
            )
        })
        .collect();
    target_mmr_digest(season_id, entries)
}

fn target_mmr_digest(
    season_id: Uuid,
    entries: Vec<(String, Lane, f64)>,
) -> Result<Vec<u8>, GuildDataTransferRepositoryError> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct TargetMmrSnapshot {
        season_id: Uuid,
        entries: Vec<TargetMmrSnapshotEntry>,
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct TargetMmrSnapshotEntry {
        user_id: String,
        lane: Lane,
        mmr: f64,
    }

    let mut entries = entries
        .into_iter()
        .map(|(user_id, lane, mmr)| TargetMmrSnapshotEntry { user_id, lane, mmr })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        left.user_id
            .cmp(&right.user_id)
            .then_with(|| left.lane.as_str().cmp(right.lane.as_str()))
    });
    let bytes = serde_json::to_vec(&TargetMmrSnapshot { season_id, entries })?;
    Ok(digest::digest(&digest::SHA256, &bytes).as_ref().to_vec())
}

async fn ensure_and_lock_members_state(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &UserId,
    now: OffsetDateTime,
) -> Result<LockedMembersState, AttemptError> {
    sqlx::query(
        "INSERT INTO guild_admin_state (guild_id, scope, revision, updated_by, updated_at)
         VALUES ($1, 'MEMBERS', 0, $2, $3)
         ON CONFLICT (guild_id, scope) DO NOTHING",
    )
    .bind(guild_id)
    .bind(actor_id)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    sqlx::query_as::<_, LockedMembersState>(
        "SELECT revision
         FROM guild_admin_state
         WHERE guild_id = $1 AND scope = 'MEMBERS'
         FOR UPDATE",
    )
    .bind(guild_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::InvariantViolation)
}

async fn increment_members_revision(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &UserId,
    current_revision: i64,
    now: OffsetDateTime,
) -> Result<i64, AttemptError> {
    let next_revision = current_revision
        .checked_add(1)
        .ok_or(AttemptError::InvariantViolation)?;
    let updated = sqlx::query(
        "UPDATE guild_admin_state
         SET revision = $1, updated_by = $2, updated_at = $3
         WHERE guild_id = $4 AND scope = 'MEMBERS' AND revision = $5",
    )
    .bind(next_revision)
    .bind(actor_id)
    .bind(now)
    .bind(guild_id)
    .bind(current_revision)
    .execute(&mut **transaction)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(AttemptError::InvariantViolation);
    }
    Ok(next_revision)
}

#[allow(clippy::too_many_arguments)]
async fn persist_mmr_changes(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    season_id: Uuid,
    import_id: Uuid,
    actor_id: &UserId,
    request_id: &str,
    now: OffsetDateTime,
    changes: &[PendingMmrChange],
) -> Result<(), AttemptError> {
    if changes.is_empty() {
        return Ok(());
    }

    let ledger_ids = changes
        .iter()
        .map(|change| change.ledger_id)
        .collect::<Vec<_>>();
    let user_ids = changes
        .iter()
        .map(|change| change.user_id.as_str().to_owned())
        .collect::<Vec<_>>();
    let lanes = changes
        .iter()
        .map(|change| change.lane.as_str().to_owned())
        .collect::<Vec<_>>();
    let lane_revisions = changes
        .iter()
        .map(|change| change.lane_revision)
        .collect::<Vec<_>>();
    let mmr_before = changes
        .iter()
        .map(|change| change.mmr_before)
        .collect::<Vec<_>>();
    let mmr_after = changes
        .iter()
        .map(|change| change.mmr_after)
        .collect::<Vec<_>>();
    let deltas = changes
        .iter()
        .map(|change| change.delta)
        .collect::<Vec<_>>();

    let inserted = sqlx::query(
        "WITH changes AS (
             SELECT *
             FROM UNNEST(
                 $7::uuid[], $8::text[], $9::text[], $10::bigint[],
                 $11::double precision[], $12::double precision[], $13::double precision[]
             ) AS input(id, user_id, lane, lane_revision, mmr_before, mmr_after, delta)
         )
         INSERT INTO mmr_ledger_entries (
             guild_id, id, season_id, user_id, lane, lane_revision, source_type, source_id,
             game_id, opponent_user_id, mmr_before, mmr_after, delta, games_played_delta,
             wins_delta, reverses_entry_id, reversed_source_type, actor_type, actor_id, note,
             request_id, created_at
         )
         SELECT
             $1, changes.id, $2, changes.user_id, changes.lane, changes.lane_revision,
             'DATA_IMPORT_SEED', $3, NULL, NULL, changes.mmr_before, changes.mmr_after,
             changes.delta, 0, 0, NULL, NULL, 'USER', $4, NULL, $5, $6
         FROM changes",
    )
    .bind(guild_id)
    .bind(season_id)
    .bind(import_id)
    .bind(actor_id)
    .bind(request_id)
    .bind(now)
    .bind(&ledger_ids)
    .bind(&user_ids)
    .bind(&lanes)
    .bind(&lane_revisions)
    .bind(&mmr_before)
    .bind(&mmr_after)
    .bind(&deltas)
    .execute(&mut **transaction)
    .await?;
    if inserted.rows_affected() != changes.len() as u64 {
        return Err(AttemptError::InvariantViolation);
    }

    let updated = sqlx::query(
        "WITH changes AS (
             SELECT *
             FROM UNNEST($4::text[], $5::text[], $6::bigint[], $7::double precision[])
                  AS input(user_id, lane, lane_revision, mmr_after)
         )
         UPDATE user_lane_mmr AS target
         SET mmr = changes.mmr_after,
             revision = changes.lane_revision,
             updated_at = $3
         FROM changes
         WHERE target.guild_id = $1
           AND target.season_id = $2
           AND target.user_id = changes.user_id
           AND target.lane = changes.lane",
    )
    .bind(guild_id)
    .bind(season_id)
    .bind(now)
    .bind(&user_ids)
    .bind(&lanes)
    .bind(&lane_revisions)
    .bind(&mmr_after)
    .execute(&mut **transaction)
    .await?;
    if updated.rows_affected() != changes.len() as u64 {
        return Err(AttemptError::InvariantViolation);
    }

    Ok(())
}

async fn has_active_workflow(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
) -> Result<bool, AttemptError> {
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1 FROM recruitments
              WHERE guild_id = $1 AND status IN ('OPEN', 'CLOSED')
             UNION ALL
             SELECT 1 FROM series
              WHERE guild_id = $1 AND status = 'IN_PROGRESS' AND deleted_at IS NULL
             UNION ALL
             SELECT 1 FROM auction_recruitments
              WHERE guild_id = $1 AND status IN ('OPEN', 'CLOSED')
             UNION ALL
             SELECT 1 FROM auction_tournaments
              WHERE guild_id = $1
                AND status IN ('BIDDING', 'BRACKET_SETUP', 'IN_GAME')
                AND deleted_at IS NULL
         )",
    )
    .bind(guild_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(Into::into)
}

async fn lock_import(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    import_id: Uuid,
) -> Result<StoredImport, AttemptError> {
    sqlx::query_as::<_, StoredImport>(
        "SELECT actor_id, status, archive_digest, archive_payload, target_season_id,
                target_members_revision, target_mmr_digest, summary, conflict_count, expires_at
         FROM guild_data_imports
         WHERE guild_id = $1 AND id = $2
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(import_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::PreviewNotFound)
}

#[allow(clippy::too_many_arguments)]
async fn insert_audit(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &UserId,
    action: &str,
    target_type: &str,
    target_id: &str,
    request_id: &str,
    before_summary: Value,
    after_summary: Value,
    now: OffsetDateTime,
) -> Result<(), GuildDataTransferRepositoryError> {
    sqlx::query(
        "INSERT INTO admin_audit_log (
             guild_id, id, principal_type, principal_id, action, target_type, target_id,
             checkpoint_id, request_id, before_summary, after_summary, result, note, created_at
         ) VALUES ($1, $2, 'USER', $3, $4, $5, $6, NULL, $7, $8, $9, 'APPLIED', NULL, $10)",
    )
    .bind(guild_id)
    .bind(Uuid::now_v7())
    .bind(actor_id)
    .bind(action)
    .bind(target_type)
    .bind(target_id)
    .bind(request_id)
    .bind(before_summary)
    .bind(after_summary)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_audit_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &UserId,
    action: &str,
    target_type: &str,
    target_id: &str,
    request_id: &str,
    before_summary: Value,
    after_summary: Value,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    insert_audit(
        transaction,
        guild_id,
        actor_id,
        action,
        target_type,
        target_id,
        request_id,
        before_summary,
        after_summary,
        now,
    )
    .await
    .map_err(|error| match error {
        GuildDataTransferRepositoryError::Database(error) => AttemptError::Sql(error),
        _ => AttemptError::InvariantViolation,
    })
}

async fn acquire_idempotency<T: DeserializeOwned>(
    transaction: &mut DatabaseTransaction,
    scope: &TenantScope,
    record_id: IdempotencyRecordId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_hash: &[u8],
    now: OffsetDateTime,
) -> Result<IdempotencyDecision<T>, AttemptError> {
    let command = AcquireIdempotency {
        record_id,
        scope,
        principal_type: IdempotencyPrincipalType::User,
        principal_id: actor_id.as_str(),
        method: APPLY_METHOD,
        route_template: APPLY_ROUTE,
        idempotency_key,
        request_hash,
        created_at: now,
        expires_at: now + IDEMPOTENCY_TTL,
    };
    match idempotency::acquire(transaction, &command).await? {
        AcquireIdempotencyOutcome::Acquired { record_id } => {
            Ok(IdempotencyDecision::Acquired(record_id))
        }
        AcquireIdempotencyOutcome::Replay { response, .. } => {
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
    result: &ImportApplyResult,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let outcome = idempotency::complete(
        transaction,
        CompleteIdempotency {
            scope,
            record_id,
            response: IdempotentResponse {
                status_code: 200,
                body: Some(serde_json::to_value(result)?),
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

fn request_hash(
    guild_id: &GuildId,
    import_id: Uuid,
    command: &ApplyImportCommand,
) -> Result<Vec<u8>, GuildDataTransferRepositoryError> {
    let canonical = serde_json::to_vec(&(guild_id, import_id, command))?;
    Ok(digest::digest(&digest::SHA256, &canonical)
        .as_ref()
        .to_vec())
}

fn attempt_sql_error(error: &AttemptError) -> Option<&sqlx::Error> {
    match error {
        AttemptError::Sql(error) => Some(error),
        AttemptError::Mmr(MmrRepositoryError::Sql(error)) => Some(error),
        AttemptError::Idempotency(IdempotencyRepositoryError::Sql(error)) => Some(error),
        AttemptError::Repository(GuildDataTransferRepositoryError::Database(error)) => Some(error),
        _ => None,
    }
}

fn map_transaction_error(
    error: TransactionRetryError<AttemptError>,
) -> GuildDataTransferRepositoryError {
    match error {
        TransactionRetryError::DeadlockRetriesExhausted { .. } => {
            GuildDataTransferRepositoryError::DatabaseBusy
        }
        TransactionRetryError::NonRetryable { failure, .. } => match failure {
            TransactionAttemptError::Begin(error) | TransactionAttemptError::Commit(error) => {
                GuildDataTransferRepositoryError::Database(error)
            }
            TransactionAttemptError::Operation(error) => error.into_public(),
        },
    }
}

#[derive(Debug, FromRow)]
struct SeasonRow {
    id: Uuid,
    name: String,
}

#[derive(Debug, FromRow)]
struct ExportLaneRow {
    user_id: String,
    display_name: String,
    lane: Lane,
    mmr: f64,
}

#[derive(Debug, FromRow)]
struct ExistingMmrRow {
    user_id: String,
    lane: Lane,
    mmr: f64,
}

#[derive(Debug, FromRow)]
struct LockedMembersState {
    revision: i64,
}

#[derive(Debug, FromRow)]
struct StoredImport {
    actor_id: UserId,
    status: String,
    archive_digest: Vec<u8>,
    archive_payload: Option<Value>,
    target_season_id: Uuid,
    target_members_revision: i64,
    target_mmr_digest: Vec<u8>,
    summary: Value,
    conflict_count: i32,
    expires_at: OffsetDateTime,
}

struct ArchiveAnalysis {
    summary: ImportSummary,
    conflicts: Vec<ImportIssue>,
    target_mmr_digest: Vec<u8>,
}

#[derive(Clone, Copy)]
struct ApplyPrepared {
    idempotency_record_id: IdempotencyRecordId,
}

struct PendingMmrChange {
    ledger_id: Uuid,
    user_id: UserId,
    lane: Lane,
    lane_revision: i64,
    mmr_before: f64,
    mmr_after: f64,
    delta: f64,
}

enum IdempotencyDecision<T> {
    Acquired(IdempotencyRecordId),
    Replay(T),
}

#[derive(Debug, Error)]
enum AttemptError {
    #[error("PostgreSQL data transfer operation failed")]
    Sql(#[from] sqlx::Error),
    #[error("MMR operation failed")]
    Mmr(#[from] MmrRepositoryError),
    #[error("idempotency operation failed")]
    Idempotency(#[from] IdempotencyRepositoryError),
    #[error("data transfer serialization failed")]
    Serialization(#[from] serde_json::Error),
    #[error("data transfer repository operation failed")]
    Repository(#[from] GuildDataTransferRepositoryError),
    #[error("guild has no active season")]
    NoActiveSeason,
    #[error("preview was not found")]
    PreviewNotFound,
    #[error("preview belongs to another actor")]
    PreviewActorMismatch,
    #[error("preview has expired")]
    PreviewExpired,
    #[error("preview was already applied")]
    PreviewAlreadyApplied,
    #[error("preview digest does not match")]
    PreviewDigestMismatch,
    #[error("preview has conflicts")]
    PreviewConflicts,
    #[error("target revision changed")]
    RevisionConflict { expected: i64, current: i64 },
    #[error("target active season changed")]
    TargetSeasonChanged,
    #[error("target MMR state changed")]
    MmrStateChanged,
    #[error("active workflow prevents an MMR seed import")]
    ActiveWorkflowConflict,
    #[error("archive has no MMR changes")]
    NoChanges,
    #[error("idempotency key was reused with another request")]
    IdempotencyConflict,
    #[error("idempotent request is already in progress")]
    IdempotencyInProgress,
    #[error("stored idempotent response is corrupt")]
    CorruptIdempotentResponse,
    #[error("idempotency completion was lost")]
    IdempotencyCompletionLost,
    #[error("stored data transfer invariant failed")]
    InvariantViolation,
}

impl AttemptError {
    fn into_public(self) -> GuildDataTransferRepositoryError {
        match self {
            Self::Sql(error) => GuildDataTransferRepositoryError::Database(error),
            Self::Mmr(MmrRepositoryError::Sql(error))
            | Self::Idempotency(IdempotencyRepositoryError::Sql(error)) => {
                GuildDataTransferRepositoryError::Database(error)
            }
            Self::Serialization(error) => GuildDataTransferRepositoryError::Serialization(error),
            Self::Repository(error) => error,
            Self::NoActiveSeason => GuildDataTransferRepositoryError::NoActiveSeason,
            Self::PreviewNotFound => GuildDataTransferRepositoryError::PreviewNotFound,
            Self::PreviewActorMismatch => GuildDataTransferRepositoryError::PreviewActorMismatch,
            Self::PreviewExpired => GuildDataTransferRepositoryError::PreviewExpired,
            Self::PreviewAlreadyApplied => GuildDataTransferRepositoryError::PreviewAlreadyApplied,
            Self::PreviewDigestMismatch => GuildDataTransferRepositoryError::PreviewDigestMismatch,
            Self::PreviewConflicts => GuildDataTransferRepositoryError::PreviewConflicts,
            Self::RevisionConflict { expected, current } => {
                GuildDataTransferRepositoryError::RevisionConflict { expected, current }
            }
            Self::TargetSeasonChanged => GuildDataTransferRepositoryError::TargetSeasonChanged,
            Self::MmrStateChanged => GuildDataTransferRepositoryError::MmrStateChanged,
            Self::ActiveWorkflowConflict => {
                GuildDataTransferRepositoryError::ActiveWorkflowConflict
            }
            Self::NoChanges => GuildDataTransferRepositoryError::NoChanges,
            Self::IdempotencyConflict => GuildDataTransferRepositoryError::IdempotencyConflict,
            Self::IdempotencyInProgress => GuildDataTransferRepositoryError::IdempotencyInProgress,
            Self::Mmr(_)
            | Self::Idempotency(_)
            | Self::CorruptIdempotentResponse
            | Self::IdempotencyCompletionLost
            | Self::InvariantViolation => GuildDataTransferRepositoryError::InvariantViolation,
        }
    }
}

#[derive(Debug, Error)]
pub enum GuildDataTransferRepositoryError {
    #[error("PostgreSQL data transfer operation failed")]
    Database(#[from] sqlx::Error),
    #[error("data transfer serialization failed")]
    Serialization(#[from] serde_json::Error),
    #[error("guild was not found or is inaccessible")]
    GuildNotFound,
    #[error("operator permission is required")]
    OperatorRequired,
    #[error("guild has no active season")]
    NoActiveSeason,
    #[error("archive is invalid")]
    InvalidArchive,
    #[error("archive exceeds the supported member limit")]
    ArchiveMemberLimitExceeded,
    #[error("apply command is invalid")]
    InvalidApplyCommand,
    #[error("preview was not found")]
    PreviewNotFound,
    #[error("preview belongs to another operator")]
    PreviewActorMismatch,
    #[error("preview has expired")]
    PreviewExpired,
    #[error("preview was already applied")]
    PreviewAlreadyApplied,
    #[error("preview digest does not match")]
    PreviewDigestMismatch,
    #[error("preview has conflicts")]
    PreviewConflicts,
    #[error("target revision changed")]
    RevisionConflict { expected: i64, current: i64 },
    #[error("target active season changed")]
    TargetSeasonChanged,
    #[error("target MMR state changed")]
    MmrStateChanged,
    #[error("active workflow prevents an MMR seed import")]
    ActiveWorkflowConflict,
    #[error("archive has no MMR changes")]
    NoChanges,
    #[error("archive MMR cannot be applied to the target MMR state")]
    InvalidMmrTransfer,
    #[error("idempotency key was reused with another request")]
    IdempotencyConflict,
    #[error("idempotent request is already in progress")]
    IdempotencyInProgress,
    #[error("database remained busy after bounded retries")]
    DatabaseBusy,
    #[error("stored data transfer invariant failed")]
    InvariantViolation,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn archive() -> MemberSeasonMmrSeedArchive {
        MemberSeasonMmrSeedArchive {
            format: ARCHIVE_FORMAT.to_owned(),
            kind: ARCHIVE_KIND.to_owned(),
            schema_version: ARCHIVE_SCHEMA_VERSION,
            exported_at: OffsetDateTime::UNIX_EPOCH,
            source: ArchiveSource {
                guild_id: GuildId::parse("123456789012345678").expect("guild"),
                guild_name: "Source guild".to_owned(),
                season: ArchiveSeason {
                    id: Uuid::now_v7(),
                    name: "Season 1".to_owned(),
                },
            },
            members: vec![ArchiveMember {
                discord_id: DiscordUserId::parse("234567890123456789").expect("user"),
                display_name: "Longcat".to_owned(),
                lanes: CANONICAL_LANES
                    .into_iter()
                    .map(|lane| ArchiveLaneMmr { lane, mmr: 1500.0 })
                    .collect(),
            }],
        }
    }

    #[test]
    fn plain_archive_digest_changes_when_content_changes() {
        let mut value = archive();
        let original = archive_digest(&value).expect("archive digest");
        value.members[0].lanes[0].mmr = 1600.0;
        let edited = archive_digest(&value).expect("edited archive digest");
        assert_ne!(original, edited);
    }

    #[test]
    fn archive_validation_requires_each_canonical_lane_once() {
        let mut value = archive();
        value.members[0].lanes[4].lane = Lane::Top;
        assert!(matches!(
            validate_archive(&value),
            Err(GuildDataTransferRepositoryError::InvalidArchive)
        ));
    }

    #[test]
    fn digest_parser_requires_a_sha256_hex_value() {
        assert!(parse_digest(&"0".repeat(64)).is_ok());
        assert!(parse_digest("not-a-digest").is_err());
    }

    #[test]
    fn target_mmr_snapshot_detects_a_change_after_preview() {
        let value = archive();
        let user_id = value.members[0].discord_id.as_str().to_owned();
        let active_ids = HashSet::from([user_id.clone()]);
        let mut current = value.members[0]
            .lanes
            .iter()
            .map(|entry| ((user_id.clone(), entry.lane), entry.mmr))
            .collect::<HashMap<_, _>>();
        let original =
            target_mmr_digest_from_current(value.source.season.id, &value, &active_ids, &current)
                .expect("snapshot digest");
        current.insert((user_id, Lane::Mid), 1600.0);
        let changed =
            target_mmr_digest_from_current(value.source.season.id, &value, &active_ids, &current)
                .expect("changed snapshot digest");

        assert_ne!(original, changed);
    }

    #[test]
    fn rejects_a_finite_mmr_pair_with_an_infinite_ledger_delta() {
        assert!(matches!(
            checked_mmr_delta(-f64::MAX, f64::MAX),
            Err(GuildDataTransferRepositoryError::InvalidMmrTransfer)
        ));
    }
}
