use std::collections::{BTreeMap, HashSet};

use ring::digest;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{AssertSqlSafe, FromRow, PgConnection, PgPool};
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
use crate::domain::{GuildId, IdempotencyRecordId, UserId};

const ARCHIVE_FORMAT: &str = "MONKEYBOT_DATA_TRANSFER";
const ARCHIVE_KIND: &str = "GUILD_FULL_SNAPSHOT";
const ARCHIVE_SCHEMA_VERSION: u16 = 1;
const MAX_ARCHIVE_ROWS: usize = 250_000;
const MAX_TEXT_CHARS: usize = 100;
const MAX_CHAMPION_ID_BYTES: usize = 64;
const PREVIEW_TTL: Duration = Duration::minutes(15);
const IDEMPOTENCY_TTL: Duration = Duration::hours(24);
const APPLY_METHOD: &str = "POST";
const APPLY_ROUTE: &str = "/api/guilds/{guildId}/full-data-imports/{importId}/apply";

const CROSS_GUILD_DISCORD_LINKS: [(&str, &[&str]); 4] = [
    ("recruitments", &["channel_id", "message_id"]),
    ("series", &["end_card_channel_id", "end_card_message_id"]),
    ("auction_recruitments", &["channel_id", "message_id"]),
    (
        "auction_tournaments",
        &["end_card_channel_id", "end_card_message_id"],
    ),
];

const CHAMPION_ID_TABLE_NAMES: [&str; 5] = [
    "user_champion_preferences",
    "game_input_picks",
    "game_input_bans",
    "game_participants",
    "game_bans",
];

const ACTIVE_SEASON_DOMAIN_TABLE_NAMES: [&str; 8] = [
    "recruitments",
    "series",
    "auction_recruitments",
    "auction_tournaments",
    "auction_matches",
    "games",
    "user_lane_mmr",
    "mmr_ledger_entries",
];

pub const ARCHIVE_TABLE_NAMES: [&str; 41] = [
    "users",
    "user_discord_identities",
    "user_privacy_settings",
    "user_champion_preferences",
    "riot_accounts",
    "dummy_user_profiles",
    "guild_members",
    "user_guild_preferences",
    "guild_counters",
    "seasons",
    "recruitments",
    "recruitment_participants",
    "recruitment_preferred_lanes",
    "recruitment_entry_drafts",
    "recruitment_entry_assignments",
    "series",
    "series_participants",
    "auction_recruitments",
    "auction_recruitment_participants",
    "auction_setup_drafts",
    "auction_setup_teams",
    "auction_tournaments",
    "auction_teams",
    "auction_bid_candidates",
    "auction_lots",
    "auction_actions",
    "auction_team_members",
    "auction_point_ledger",
    "auction_bracket_slots",
    "auction_matches",
    "auction_match_lineups",
    "game_inputs",
    "game_input_picks",
    "game_input_bans",
    "games",
    "game_participants",
    "game_participant_stats",
    "game_bans",
    "user_lane_mmr",
    "mmr_ledger_entries",
    "admin_audit_log",
];

const GUILD_SCOPED_TABLE_NAMES: [&str; 32] = [
    "guild_members",
    "guild_counters",
    "seasons",
    "recruitments",
    "recruitment_participants",
    "recruitment_preferred_lanes",
    "recruitment_entry_drafts",
    "recruitment_entry_assignments",
    "series",
    "series_participants",
    "auction_recruitments",
    "auction_recruitment_participants",
    "auction_setup_drafts",
    "auction_setup_teams",
    "auction_tournaments",
    "auction_teams",
    "auction_bid_candidates",
    "auction_lots",
    "auction_actions",
    "auction_team_members",
    "auction_point_ledger",
    "auction_bracket_slots",
    "auction_matches",
    "auction_match_lineups",
    "game_inputs",
    "game_input_picks",
    "game_input_bans",
    "games",
    "game_participants",
    "game_participant_stats",
    "game_bans",
    "user_lane_mmr",
];

const DOMAIN_TABLE_NAMES: [&str; 31] = [
    "seasons",
    "recruitments",
    "recruitment_participants",
    "recruitment_preferred_lanes",
    "recruitment_entry_drafts",
    "recruitment_entry_assignments",
    "series",
    "series_participants",
    "auction_recruitments",
    "auction_recruitment_participants",
    "auction_setup_drafts",
    "auction_setup_teams",
    "auction_tournaments",
    "auction_teams",
    "auction_bid_candidates",
    "auction_lots",
    "auction_actions",
    "auction_team_members",
    "auction_point_ledger",
    "auction_bracket_slots",
    "auction_matches",
    "auction_match_lineups",
    "game_inputs",
    "game_input_picks",
    "game_input_bans",
    "games",
    "game_participants",
    "game_participant_stats",
    "game_bans",
    "user_lane_mmr",
    "mmr_ledger_entries",
];

const IMPORT_ORDER_BEFORE_TOURNAMENT_TEAMS: [&str; 21] = [
    "users",
    "user_discord_identities",
    "user_privacy_settings",
    "user_champion_preferences",
    "riot_accounts",
    "dummy_user_profiles",
    "guild_members",
    "user_guild_preferences",
    "guild_counters",
    "seasons",
    "recruitments",
    "recruitment_participants",
    "recruitment_preferred_lanes",
    "recruitment_entry_drafts",
    "recruitment_entry_assignments",
    "series",
    "series_participants",
    "auction_recruitments",
    "auction_recruitment_participants",
    "auction_setup_drafts",
    "auction_setup_teams",
];

const IMPORT_ORDER_AFTER_TOURNAMENT_TEAMS: [&str; 18] = [
    "auction_bid_candidates",
    "auction_lots",
    "auction_actions",
    "auction_team_members",
    "auction_point_ledger",
    "auction_bracket_slots",
    "auction_matches",
    "auction_match_lineups",
    "game_inputs",
    "game_input_picks",
    "game_input_bans",
    "games",
    "game_participants",
    "game_participant_stats",
    "game_bans",
    "user_lane_mmr",
    "mmr_ledger_entries",
    "admin_audit_log",
];

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GuildFullSnapshotArchive {
    pub format: String,
    pub kind: String,
    pub schema_version: u16,
    #[serde(with = "time::serde::rfc3339")]
    pub exported_at: OffsetDateTime,
    pub source: FullArchiveSource,
    pub tables: BTreeMap<String, Vec<Value>>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FullArchiveSource {
    pub guild_id: GuildId,
    pub guild_name: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FullDataExport {
    pub archive: GuildFullSnapshotArchive,
    pub file_name: String,
    pub summary: FullImportSummary,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplyFullImportCommand {
    pub expected_digest: String,
    pub expected_target_digest: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FullImportIssue {
    pub reason: String,
    pub message: String,
    pub table: Option<String>,
    pub row_count: usize,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FullImportSummary {
    pub total_rows: usize,
    pub users: usize,
    pub members: usize,
    pub seasons: usize,
    pub recruitments: usize,
    pub series: usize,
    pub games: usize,
    pub auction_tournaments: usize,
    pub auction_matches: usize,
    pub mmr_ledger_entries: usize,
    pub audit_entries: usize,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FullImportPreview {
    pub import_id: Uuid,
    pub digest: String,
    pub target_digest: String,
    #[serde(with = "time::serde::rfc3339")]
    pub expires_at: OffsetDateTime,
    pub source: FullArchiveSource,
    pub summary: FullImportSummary,
    pub conflicts: Vec<FullImportIssue>,
    pub remaps_source_guild: bool,
    pub uses_target_active_season: bool,
    pub rebases_display_numbers: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FullImportApplyResult {
    pub import_id: Uuid,
    pub status: String,
    #[serde(with = "time::serde::rfc3339")]
    pub applied_at: OffsetDateTime,
    pub summary: FullImportSummary,
    pub remapped_source_guild: bool,
    pub used_target_active_season: bool,
    pub rebased_display_numbers: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct MappedImportArchive {
    archive: GuildFullSnapshotArchive,
    remaps_source_guild: bool,
    uses_target_active_season: bool,
    rebases_display_numbers: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct DisplayNumberBases {
    recruitment: i64,
    series: i64,
    auction_recruitment: i64,
    auction_tournament: i64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct TargetImportState {
    domain_rows: i64,
    active_domain_rows: i64,
    target_active_season_id: Option<Uuid>,
    display_number_bases: DisplayNumberBases,
}

impl TargetImportState {
    fn is_importable(self) -> bool {
        self.domain_rows == 0
            || (self.target_active_season_id.is_some() && self.active_domain_rows == 0)
    }
}

pub async fn export(
    pool: &PgPool,
    guild_id: &GuildId,
    actor_id: &UserId,
    request_id: &str,
) -> Result<FullDataExport, GuildFullDataTransferRepositoryError> {
    let now = OffsetDateTime::now_utc();
    let mut transaction = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *transaction)
        .await?;
    authorize_operator(&mut transaction, guild_id, actor_id).await?;

    let source = FullArchiveSource {
        guild_id: guild_id.clone(),
        guild_name: guild_display_name(&mut transaction, guild_id).await?,
    };
    let tables = export_tables(&mut transaction, guild_id).await?;
    let archive = GuildFullSnapshotArchive {
        format: ARCHIVE_FORMAT.to_owned(),
        kind: ARCHIVE_KIND.to_owned(),
        schema_version: ARCHIVE_SCHEMA_VERSION,
        exported_at: now,
        source,
        tables,
    };
    validate_archive(&archive)?;
    let summary = summarize(&archive);
    insert_audit(
        &mut transaction,
        guild_id,
        actor_id,
        "FULL_DATA_TRANSFER_EXPORTED",
        "GUILD_FULL_SNAPSHOT",
        guild_id.as_str(),
        request_id,
        json!({ "schemaVersion": ARCHIVE_SCHEMA_VERSION }),
        serde_json::to_value(&summary)?,
        now,
    )
    .await?;
    transaction.commit().await?;

    Ok(FullDataExport {
        file_name: format!(
            "monkeybot-full-{}-v{}.json",
            guild_id.as_str(),
            ARCHIVE_SCHEMA_VERSION
        ),
        archive,
        summary,
    })
}

pub async fn preview(
    pool: &PgPool,
    guild_id: &GuildId,
    actor_id: &UserId,
    archive_value: Value,
    request_id: &str,
) -> Result<FullImportPreview, GuildFullDataTransferRepositoryError> {
    let archive: GuildFullSnapshotArchive = serde_json::from_value(archive_value)
        .map_err(|_| GuildFullDataTransferRepositoryError::InvalidArchive)?;
    validate_archive(&archive)?;
    let archive_digest = archive_digest(&archive)?;
    let summary = summarize(&archive);
    let remaps_source_guild = archive.source.guild_id != *guild_id;
    let now = OffsetDateTime::now_utc();
    let expires_at = now + PREVIEW_TTL;

    let mut transaction = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *transaction)
        .await?;
    authorize_operator(&mut transaction, guild_id, actor_id).await?;
    lock_guild(&mut transaction, guild_id).await?;
    let target_state = target_import_state(&mut transaction, guild_id).await?;
    let target_digest = target_state_digest(&mut transaction, guild_id, target_state).await?;
    let mut uses_target_active_season = false;
    let mut rebases_display_numbers = false;
    let conflicts = if target_state.is_importable() {
        let mapped = map_archive_for_target(
            &archive,
            guild_id,
            target_state.target_active_season_id,
            target_state.display_number_bases,
        )?;
        uses_target_active_season = mapped.uses_target_active_season;
        rebases_display_numbers = mapped.rebases_display_numbers;
        validate_against_database(&mut transaction, &mapped.archive).await?;
        Vec::new()
    } else {
        let (reason, message, row_count) = if target_state.target_active_season_id.is_none() {
            (
                "target_active_season_missing",
                "대상 길드에 사용할 활성 시즌이 없습니다. 빈 활성 시즌을 만든 뒤 다시 검증해 주세요.",
                target_state.domain_rows,
            )
        } else {
            (
                "target_active_season_not_empty",
                "대상 길드의 현재 활성 시즌에 이미 전적·MMR 또는 진행 데이터가 있어 전체 스냅샷을 적용할 수 없습니다.",
                target_state.active_domain_rows,
            )
        };
        vec![FullImportIssue {
            reason: reason.to_owned(),
            message: message.to_owned(),
            table: None,
            row_count: usize::try_from(row_count)
                .map_err(|_| GuildFullDataTransferRepositoryError::InvariantViolation)?,
        }]
    };
    let import_id = Uuid::now_v7();
    let conflict_count = i32::try_from(conflicts.len())
        .map_err(|_| GuildFullDataTransferRepositoryError::InvariantViolation)?;
    sqlx::query(
        "INSERT INTO guild_full_data_imports (
             guild_id, id, actor_id, status, archive_digest, archive_payload,
             source_guild_id, target_state_digest, summary, conflicts, conflict_count,
             request_id, created_at, expires_at, applied_at
         ) VALUES (
             $1, $2, $3, 'PREVIEWED', $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, NULL
         )",
    )
    .bind(guild_id)
    .bind(import_id)
    .bind(actor_id)
    .bind(&archive_digest)
    .bind(serde_json::to_value(&archive)?)
    .bind(&archive.source.guild_id)
    .bind(&target_digest)
    .bind(serde_json::to_value(&summary)?)
    .bind(serde_json::to_value(&conflicts)?)
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
        "FULL_DATA_IMPORT_PREVIEWED",
        "FULL_DATA_IMPORT",
        &import_id.to_string(),
        request_id,
        json!({
            "digest": digest_hex(&archive_digest),
            "sourceGuildId": archive.source.guild_id,
            "targetGuildId": guild_id,
        }),
        json!({
            "totalRows": summary.total_rows,
            "conflictCount": conflicts.len(),
            "remapsSourceGuild": remaps_source_guild,
            "usesTargetActiveSeason": uses_target_active_season,
            "rebasesDisplayNumbers": rebases_display_numbers,
        }),
        now,
    )
    .await?;
    transaction.commit().await?;

    Ok(FullImportPreview {
        import_id,
        digest: digest_hex(&archive_digest),
        target_digest: digest_hex(&target_digest),
        expires_at,
        source: archive.source,
        summary,
        conflicts,
        remaps_source_guild,
        uses_target_active_season,
        rebases_display_numbers,
    })
}

pub async fn apply(
    pool: &PgPool,
    guild_id: &GuildId,
    actor_id: &UserId,
    import_id: Uuid,
    idempotency_key: &str,
    request_id: &str,
    command: ApplyFullImportCommand,
) -> Result<FullImportApplyResult, GuildFullDataTransferRepositoryError> {
    let expected_digest = parse_digest(&command.expected_digest)?;
    let expected_target_digest = parse_digest(&command.expected_target_digest)?;
    let request_hash = request_hash(guild_id, import_id, &command)?;
    let now = OffsetDateTime::now_utc();
    let mut transaction = pool.begin().await?;
    authorize_operator(&mut transaction, guild_id, actor_id).await?;
    lock_guild(&mut transaction, guild_id).await?;

    let scope = TenantScope::guild(guild_id.clone());
    let acquired = idempotency::acquire(
        &mut transaction,
        &AcquireIdempotency {
            record_id: IdempotencyRecordId::new(),
            scope: &scope,
            principal_type: IdempotencyPrincipalType::User,
            principal_id: actor_id.as_str(),
            method: APPLY_METHOD,
            route_template: APPLY_ROUTE,
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
                .ok_or(GuildFullDataTransferRepositoryError::InvariantViolation)
                .and_then(|value| {
                    serde_json::from_value(value)
                        .map_err(|_| GuildFullDataTransferRepositoryError::InvariantViolation)
                })?;
            transaction.rollback().await?;
            return Ok(result);
        }
        AcquireIdempotencyOutcome::Conflict { .. } => {
            return Err(GuildFullDataTransferRepositoryError::IdempotencyConflict);
        }
        AcquireIdempotencyOutcome::InProgress => {
            return Err(GuildFullDataTransferRepositoryError::IdempotencyInProgress);
        }
    };

    let stored = lock_import(&mut transaction, guild_id, import_id).await?;
    if stored.actor_id != *actor_id {
        return Err(GuildFullDataTransferRepositoryError::PreviewActorMismatch);
    }
    if stored.status != "PREVIEWED" {
        return Err(GuildFullDataTransferRepositoryError::PreviewAlreadyApplied);
    }
    if stored.expires_at <= now {
        return Err(GuildFullDataTransferRepositoryError::PreviewExpired);
    }
    if stored.archive_digest != expected_digest {
        return Err(GuildFullDataTransferRepositoryError::PreviewDigestMismatch);
    }
    if stored.target_state_digest != expected_target_digest {
        return Err(GuildFullDataTransferRepositoryError::TargetStateChanged);
    }
    if stored.conflict_count != 0 {
        return Err(GuildFullDataTransferRepositoryError::PreviewConflicts);
    }
    let archive: GuildFullSnapshotArchive = serde_json::from_value(
        stored
            .archive_payload
            .ok_or(GuildFullDataTransferRepositoryError::InvariantViolation)?,
    )
    .map_err(|_| GuildFullDataTransferRepositoryError::InvariantViolation)?;
    validate_archive(&archive)?;
    if archive.source.guild_id != stored.source_guild_id
        || archive_digest(&archive)? != stored.archive_digest
    {
        return Err(GuildFullDataTransferRepositoryError::PreviewDigestMismatch);
    }
    let target_state = target_import_state(&mut transaction, guild_id).await?;
    let current_target_digest =
        target_state_digest(&mut transaction, guild_id, target_state).await?;
    if current_target_digest != stored.target_state_digest || !target_state.is_importable() {
        return Err(GuildFullDataTransferRepositoryError::TargetStateChanged);
    }

    let mapped = map_archive_for_target(
        &archive,
        guild_id,
        target_state.target_active_season_id,
        target_state.display_number_bases,
    )?;
    import_archive_rows(&mut transaction, &mapped.archive).await?;
    let summary: FullImportSummary = serde_json::from_value(stored.summary)
        .map_err(|_| GuildFullDataTransferRepositoryError::InvariantViolation)?;
    let marked = sqlx::query(
        "UPDATE guild_full_data_imports
         SET status = 'APPLIED', archive_payload = NULL, applied_at = $1
         WHERE guild_id = $2 AND id = $3 AND status = 'PREVIEWED'",
    )
    .bind(now)
    .bind(guild_id)
    .bind(import_id)
    .execute(&mut *transaction)
    .await?;
    if marked.rows_affected() != 1 {
        return Err(GuildFullDataTransferRepositoryError::InvariantViolation);
    }
    insert_audit(
        &mut transaction,
        guild_id,
        actor_id,
        "FULL_DATA_IMPORT_APPLIED",
        "FULL_DATA_IMPORT",
        &import_id.to_string(),
        request_id,
        json!({
            "digest": command.expected_digest,
            "sourceGuildId": archive.source.guild_id,
            "targetGuildId": guild_id,
        }),
        json!({
            "summary": summary,
            "remappedSourceGuild": mapped.remaps_source_guild,
            "usedTargetActiveSeason": mapped.uses_target_active_season,
            "rebasedDisplayNumbers": mapped.rebases_display_numbers,
        }),
        now,
    )
    .await?;
    let result = FullImportApplyResult {
        import_id,
        status: "APPLIED".to_owned(),
        applied_at: now,
        summary,
        remapped_source_guild: mapped.remaps_source_guild,
        used_target_active_season: mapped.uses_target_active_season,
        rebased_display_numbers: mapped.rebases_display_numbers,
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
        return Err(GuildFullDataTransferRepositoryError::InvariantViolation);
    }
    transaction.commit().await?;
    Ok(result)
}

pub async fn cleanup_expired_previews(
    pool: &PgPool,
    now: OffsetDateTime,
) -> Result<u64, GuildFullDataTransferRepositoryError> {
    let result = sqlx::query(
        "DELETE FROM guild_full_data_imports
         WHERE status = 'PREVIEWED' AND expires_at <= $1",
    )
    .bind(now)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

fn validate_archive(
    archive: &GuildFullSnapshotArchive,
) -> Result<(), GuildFullDataTransferRepositoryError> {
    if archive.format != ARCHIVE_FORMAT
        || archive.kind != ARCHIVE_KIND
        || archive.schema_version != ARCHIVE_SCHEMA_VERSION
        || !valid_text(&archive.source.guild_name)
    {
        return Err(GuildFullDataTransferRepositoryError::InvalidArchive);
    }
    let expected = ARCHIVE_TABLE_NAMES.into_iter().collect::<HashSet<_>>();
    let actual = archive
        .tables
        .keys()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    if actual != expected {
        return Err(GuildFullDataTransferRepositoryError::InvalidArchive);
    }
    let total_rows = archive
        .tables
        .values()
        .try_fold(0usize, |total, rows| total.checked_add(rows.len()))
        .ok_or(GuildFullDataTransferRepositoryError::ArchiveRowLimitExceeded)?;
    if total_rows > MAX_ARCHIVE_ROWS {
        return Err(GuildFullDataTransferRepositoryError::ArchiveRowLimitExceeded);
    }
    for (table, rows) in &archive.tables {
        let guild_scoped = GUILD_SCOPED_TABLE_NAMES.contains(&table.as_str())
            || table == "mmr_ledger_entries"
            || table == "admin_audit_log";
        for row in rows {
            let object = row
                .as_object()
                .ok_or(GuildFullDataTransferRepositoryError::InvalidArchive)?;
            if guild_scoped
                && object.get("guild_id").and_then(Value::as_str)
                    != Some(archive.source.guild_id.as_str())
            {
                return Err(GuildFullDataTransferRepositoryError::InvalidArchive);
            }
            if table == "user_guild_preferences"
                && object.get("default_guild_id").and_then(Value::as_str)
                    != Some(archive.source.guild_id.as_str())
            {
                return Err(GuildFullDataTransferRepositoryError::InvalidArchive);
            }
            if CHAMPION_ID_TABLE_NAMES.contains(&table.as_str())
                && !valid_archive_champion_id(table, object.get("champion_id"))
            {
                return Err(
                    GuildFullDataTransferRepositoryError::ArchiveConstraintViolation {
                        table: table.to_owned(),
                    },
                );
            }
        }
    }
    Ok(())
}

fn valid_archive_champion_id(table: &str, value: Option<&Value>) -> bool {
    match value {
        Some(Value::String(value)) => {
            !value.is_empty()
                && value.len() <= MAX_CHAMPION_ID_BYTES
                && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
        }
        Some(Value::Null) => table == "game_participants",
        _ => false,
    }
}

fn valid_text(value: &str) -> bool {
    let length = value.trim().chars().count();
    (1..=MAX_TEXT_CHARS).contains(&length)
}

fn summarize(archive: &GuildFullSnapshotArchive) -> FullImportSummary {
    let count = |name: &str| archive.tables.get(name).map_or(0, Vec::len);
    FullImportSummary {
        total_rows: archive.tables.values().map(Vec::len).sum(),
        users: count("users"),
        members: count("guild_members"),
        seasons: count("seasons"),
        recruitments: count("recruitments") + count("auction_recruitments"),
        series: count("series"),
        games: count("games"),
        auction_tournaments: count("auction_tournaments"),
        auction_matches: count("auction_matches"),
        mmr_ledger_entries: count("mmr_ledger_entries"),
        audit_entries: count("admin_audit_log"),
    }
}

async fn export_tables(
    connection: &mut PgConnection,
    guild_id: &GuildId,
) -> Result<BTreeMap<String, Vec<Value>>, GuildFullDataTransferRepositoryError> {
    let mut tables = BTreeMap::new();
    for table in [
        "users",
        "user_discord_identities",
        "user_privacy_settings",
        "user_champion_preferences",
        "dummy_user_profiles",
    ] {
        tables.insert(
            table.to_owned(),
            export_member_global_table(connection, guild_id, table).await?,
        );
    }
    tables.insert(
        "riot_accounts".to_owned(),
        export_riot_accounts(connection, guild_id).await?,
    );
    tables.insert(
        "user_guild_preferences".to_owned(),
        export_user_guild_preferences(connection, guild_id).await?,
    );
    for table in GUILD_SCOPED_TABLE_NAMES {
        tables.insert(
            table.to_owned(),
            export_guild_table(connection, guild_id, table).await?,
        );
    }
    tables.insert(
        "mmr_ledger_entries".to_owned(),
        export_guild_table(connection, guild_id, "mmr_ledger_entries").await?,
    );
    tables.insert(
        "admin_audit_log".to_owned(),
        export_admin_audit(connection, guild_id).await?,
    );
    Ok(tables)
}

async fn export_member_global_table(
    connection: &mut PgConnection,
    guild_id: &GuildId,
    table: &str,
) -> Result<Vec<Value>, GuildFullDataTransferRepositoryError> {
    let query = format!(
        "WITH selected_users AS (
             SELECT user_id FROM guild_members WHERE guild_id = $1
             UNION
             SELECT profile.created_by
             FROM dummy_user_profiles AS profile
             JOIN guild_members AS member ON member.user_id = profile.user_id
             WHERE member.guild_id = $1
             UNION
             SELECT profile.updated_by
             FROM dummy_user_profiles AS profile
             JOIN guild_members AS member ON member.user_id = profile.user_id
             WHERE member.guild_id = $1
         )
         SELECT COALESCE(
             jsonb_agg(to_jsonb(row_data) ORDER BY to_jsonb(row_data)::text),
             '[]'::jsonb
         )
         FROM (
             SELECT source.*
             FROM {table} AS source
             JOIN selected_users ON selected_users.user_id = source.user_id
         ) AS row_data"
    );
    json_array_query(connection, &query, guild_id).await
}

async fn export_riot_accounts(
    connection: &mut PgConnection,
    guild_id: &GuildId,
) -> Result<Vec<Value>, GuildFullDataTransferRepositoryError> {
    let query = "WITH selected_users AS (
             SELECT user_id FROM guild_members WHERE guild_id = $1
         )
         SELECT COALESCE(
             jsonb_agg(to_jsonb(row_data) ORDER BY row_data.user_id, row_data.puuid),
             '[]'::jsonb
         )
         FROM (
             SELECT account.puuid, account.user_id, account.platform, account.routing_region,
                    account.game_name, account.tag_line, account.normalized_game_name,
                    account.normalized_tag_line, account.profile_icon_id, account.is_main,
                    account.main_position, account.main_position_updated_at,
                    account.last_synced_at, account.created_at, account.updated_at, account.origin
             FROM riot_accounts AS account
             JOIN selected_users ON selected_users.user_id = account.user_id
         ) AS row_data";
    json_array_query(connection, query, guild_id).await
}

async fn export_user_guild_preferences(
    connection: &mut PgConnection,
    guild_id: &GuildId,
) -> Result<Vec<Value>, GuildFullDataTransferRepositoryError> {
    let query = "SELECT COALESCE(
             jsonb_agg(to_jsonb(row_data) ORDER BY row_data.user_id),
             '[]'::jsonb
         )
         FROM (
             SELECT preference.*
             FROM user_guild_preferences AS preference
             JOIN guild_members AS member ON member.user_id = preference.user_id
             WHERE member.guild_id = $1 AND preference.default_guild_id = $1
         ) AS row_data";
    json_array_query(connection, query, guild_id).await
}

async fn export_guild_table(
    connection: &mut PgConnection,
    guild_id: &GuildId,
    table: &str,
) -> Result<Vec<Value>, GuildFullDataTransferRepositoryError> {
    let query = format!(
        "SELECT COALESCE(
             jsonb_agg(to_jsonb(row_data) ORDER BY to_jsonb(row_data)::text),
             '[]'::jsonb
         )
         FROM (SELECT * FROM {table} WHERE guild_id = $1) AS row_data"
    );
    json_array_query(connection, &query, guild_id).await
}

async fn export_admin_audit(
    connection: &mut PgConnection,
    guild_id: &GuildId,
) -> Result<Vec<Value>, GuildFullDataTransferRepositoryError> {
    let query = "SELECT COALESCE(
             jsonb_agg(to_jsonb(row_data) ORDER BY row_data.created_at, row_data.id),
             '[]'::jsonb
         )
         FROM (
             SELECT guild_id, id, principal_type, principal_id, action, target_type, target_id,
                    NULL::uuid AS checkpoint_id, request_id, before_summary, after_summary,
                    result, note, created_at
             FROM admin_audit_log
             WHERE guild_id = $1
         ) AS row_data";
    json_array_query(connection, query, guild_id).await
}

async fn json_array_query(
    connection: &mut PgConnection,
    query: &str,
    guild_id: &GuildId,
) -> Result<Vec<Value>, GuildFullDataTransferRepositoryError> {
    // `query` is assembled only from the closed table allowlist above; archive values remain bound.
    let value = sqlx::query_scalar::<_, Value>(AssertSqlSafe(query))
        .bind(guild_id)
        .fetch_one(&mut *connection)
        .await?;
    serde_json::from_value(value).map_err(Into::into)
}

async fn validate_against_database(
    connection: &mut PgConnection,
    archive: &GuildFullSnapshotArchive,
) -> Result<(), GuildFullDataTransferRepositoryError> {
    sqlx::query("SAVEPOINT full_data_import_validation")
        .execute(&mut *connection)
        .await?;
    let result = import_archive_rows(connection, archive).await;
    let rollback = sqlx::query("ROLLBACK TO SAVEPOINT full_data_import_validation")
        .execute(&mut *connection)
        .await;
    rollback?;
    result
}

fn map_archive_for_target(
    archive: &GuildFullSnapshotArchive,
    target_guild_id: &GuildId,
    target_active_season_id: Option<Uuid>,
    display_number_bases: DisplayNumberBases,
) -> Result<MappedImportArchive, GuildFullDataTransferRepositoryError> {
    let remaps_source_guild = archive.source.guild_id != *target_guild_id;
    let mut mapped = archive.clone();

    if remaps_source_guild {
        let source_guild_id = archive.source.guild_id.as_str();
        mapped.source.guild_id = target_guild_id.clone();
        for table_rows in mapped.tables.values_mut() {
            for row in table_rows {
                replace_exact_string(row, source_guild_id, target_guild_id.as_str());
            }
        }
        clear_cross_guild_discord_links(&mut mapped)?;
    }

    let uses_target_active_season =
        map_active_season_in_place(&mut mapped, target_active_season_id)?;
    let rebases_display_numbers =
        rebase_display_numbers_in_place(&mut mapped, display_number_bases)?;
    validate_archive(&mapped)?;

    Ok(MappedImportArchive {
        archive: mapped,
        remaps_source_guild,
        uses_target_active_season,
        rebases_display_numbers,
    })
}

fn map_active_season_in_place(
    archive: &mut GuildFullSnapshotArchive,
    target_active_season_id: Option<Uuid>,
) -> Result<bool, GuildFullDataTransferRepositoryError> {
    let Some(target_active_season_id) = target_active_season_id else {
        return Ok(false);
    };
    let mut source_active_season_id = None;
    for season in rows(archive, "seasons") {
        if season.get("status").and_then(Value::as_str) != Some("ACTIVE") {
            continue;
        }
        if source_active_season_id.is_some() {
            return Err(GuildFullDataTransferRepositoryError::InvalidArchive);
        }
        let id = season
            .get("id")
            .and_then(Value::as_str)
            .ok_or(GuildFullDataTransferRepositoryError::InvalidArchive)?;
        Uuid::parse_str(id).map_err(|_| GuildFullDataTransferRepositoryError::InvalidArchive)?;
        source_active_season_id = Some(id.to_owned());
    }
    let Some(source_active_season_id) = source_active_season_id else {
        return Ok(false);
    };

    let target_active_season_id = target_active_season_id.to_string();
    archive
        .tables
        .get_mut("seasons")
        .ok_or(GuildFullDataTransferRepositoryError::InvalidArchive)?
        .retain(|season| {
            season.get("id").and_then(Value::as_str) != Some(source_active_season_id.as_str())
        });
    for table_rows in archive.tables.values_mut() {
        for row in table_rows {
            replace_exact_string(row, &source_active_season_id, &target_active_season_id);
        }
    }
    Ok(true)
}

fn clear_cross_guild_discord_links(
    archive: &mut GuildFullSnapshotArchive,
) -> Result<(), GuildFullDataTransferRepositoryError> {
    for (table, fields) in CROSS_GUILD_DISCORD_LINKS {
        let table_rows = archive
            .tables
            .get_mut(table)
            .ok_or(GuildFullDataTransferRepositoryError::InvalidArchive)?;
        for row in table_rows {
            let object = row
                .as_object_mut()
                .ok_or(GuildFullDataTransferRepositoryError::InvalidArchive)?;
            for field in fields {
                object.insert((*field).to_owned(), Value::Null);
            }
        }
    }
    Ok(())
}

fn rebase_display_numbers_in_place(
    archive: &mut GuildFullSnapshotArchive,
    bases: DisplayNumberBases,
) -> Result<bool, GuildFullDataTransferRepositoryError> {
    let namespaces = [
        (
            "recruitments",
            "RECRUITMENT",
            "일반 내전 모집 #",
            bases.recruitment,
        ),
        ("series", "SERIES", "일반 내전 #", bases.series),
        (
            "auction_recruitments",
            "AUCTION_RECRUITMENT",
            "경매 내전 모집 #",
            bases.auction_recruitment,
        ),
        (
            "auction_tournaments",
            "AUCTION_TOURNAMENT",
            "경매 내전 #",
            bases.auction_tournament,
        ),
    ];
    let mut rebased = false;

    for (table, counter_key, title_prefix, base) in namespaces {
        if base <= 0 {
            continue;
        }
        let maximum = {
            let table_rows = archive
                .tables
                .get_mut(table)
                .ok_or(GuildFullDataTransferRepositoryError::InvalidArchive)?;
            let mut maximum = None;
            for row in table_rows {
                let object = row
                    .as_object_mut()
                    .ok_or(GuildFullDataTransferRepositoryError::InvalidArchive)?;
                let source_number = object
                    .get("display_number")
                    .and_then(Value::as_i64)
                    .filter(|number| *number > 0)
                    .ok_or(GuildFullDataTransferRepositoryError::InvalidArchive)?;
                let target_number = source_number
                    .checked_add(base)
                    .ok_or(GuildFullDataTransferRepositoryError::InvalidArchive)?;
                let source_title = format!("{title_prefix}{source_number}");
                if object.get("title").and_then(Value::as_str) == Some(source_title.as_str()) {
                    object.insert(
                        "title".to_owned(),
                        Value::String(format!("{title_prefix}{target_number}")),
                    );
                }
                object.insert("display_number".to_owned(), Value::from(target_number));
                maximum =
                    Some(maximum.map_or(target_number, |value: i64| value.max(target_number)));
                rebased = true;
            }
            maximum
        };

        let Some(maximum) = maximum else {
            continue;
        };
        let minimum_next = maximum
            .checked_add(1)
            .ok_or(GuildFullDataTransferRepositoryError::InvalidArchive)?;
        let target_guild_id = archive.source.guild_id.clone();
        let counter_rows = archive
            .tables
            .get_mut("guild_counters")
            .ok_or(GuildFullDataTransferRepositoryError::InvalidArchive)?;
        let matching = counter_rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| {
                (row.get("counter_key").and_then(Value::as_str) == Some(counter_key))
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        match matching.as_slice() {
            [] => counter_rows.push(json!({
                "guild_id": target_guild_id,
                "counter_key": counter_key,
                "next_value": minimum_next,
            })),
            [index] => {
                let row = &mut counter_rows[*index];
                let object = row
                    .as_object_mut()
                    .ok_or(GuildFullDataTransferRepositoryError::InvalidArchive)?;
                let source_next = object
                    .get("next_value")
                    .and_then(Value::as_i64)
                    .filter(|next| *next > 0)
                    .ok_or(GuildFullDataTransferRepositoryError::InvalidArchive)?;
                let rebased_next = source_next
                    .checked_add(base)
                    .ok_or(GuildFullDataTransferRepositoryError::InvalidArchive)?;
                object.insert(
                    "next_value".to_owned(),
                    Value::from(rebased_next.max(minimum_next)),
                );
            }
            _ => return Err(GuildFullDataTransferRepositoryError::InvalidArchive),
        }
    }

    Ok(rebased)
}

fn replace_exact_string(value: &mut Value, source: &str, target: &str) {
    match value {
        Value::String(text) if text == source => target.clone_into(text),
        Value::Array(values) => {
            for value in values {
                replace_exact_string(value, source, target);
            }
        }
        Value::Object(object) => {
            for value in object.values_mut() {
                replace_exact_string(value, source, target);
            }
        }
        _ => {}
    }
}

async fn import_archive_rows(
    connection: &mut PgConnection,
    archive: &GuildFullSnapshotArchive,
) -> Result<(), GuildFullDataTransferRepositoryError> {
    for table in IMPORT_ORDER_BEFORE_TOURNAMENT_TEAMS {
        insert_table(connection, table, rows(archive, table)).await?;
    }

    let tournament_rows = rows(archive, "auction_tournaments");
    let initial_tournaments = tournament_rows
        .iter()
        .cloned()
        .map(|mut row| {
            let object = row
                .as_object_mut()
                .ok_or(GuildFullDataTransferRepositoryError::InvalidArchive)?;
            object.insert("status".to_owned(), Value::String("BIDDING".to_owned()));
            object.insert("champion_team_id".to_owned(), Value::Null);
            object.insert("ended_at".to_owned(), Value::Null);
            Ok(row)
        })
        .collect::<Result<Vec<_>, GuildFullDataTransferRepositoryError>>()?;
    insert_table(connection, "auction_tournaments", &initial_tournaments).await?;
    insert_table(connection, "auction_teams", rows(archive, "auction_teams")).await?;
    restore_tournament_terminal_state(connection, tournament_rows).await?;

    for table in IMPORT_ORDER_AFTER_TOURNAMENT_TEAMS {
        insert_table(connection, table, rows(archive, table)).await?;
    }
    Ok(())
}

fn rows<'a>(archive: &'a GuildFullSnapshotArchive, table: &str) -> &'a [Value] {
    archive.tables.get(table).map_or(&[], Vec::as_slice)
}

async fn insert_table(
    connection: &mut PgConnection,
    table: &str,
    rows: &[Value],
) -> Result<(), GuildFullDataTransferRepositoryError> {
    if rows.is_empty() {
        return Ok(());
    }
    let payload = Value::Array(rows.to_vec());
    let query = match table {
        "users" => String::from(
            "INSERT INTO users SELECT * FROM jsonb_populate_recordset(NULL::users, $1::jsonb)
             ON CONFLICT (user_id) DO NOTHING",
        ),
        "user_discord_identities" => String::from(
            "INSERT INTO user_discord_identities
             SELECT * FROM jsonb_populate_recordset(NULL::user_discord_identities, $1::jsonb)
             ON CONFLICT (user_id) DO NOTHING",
        ),
        "user_privacy_settings" => String::from(
            "INSERT INTO user_privacy_settings
             SELECT * FROM jsonb_populate_recordset(NULL::user_privacy_settings, $1::jsonb)
             ON CONFLICT (user_id) DO NOTHING",
        ),
        "user_champion_preferences" => String::from(
            "WITH source AS (
                 SELECT *
                 FROM jsonb_populate_recordset(NULL::user_champion_preferences, $1::jsonb)
             )
             INSERT INTO user_champion_preferences
             SELECT source.*
             FROM source
             WHERE NOT EXISTS (
                 SELECT 1
                 FROM user_champion_preferences AS existing
                 WHERE existing.user_id = source.user_id
                   AND existing.lane = source.lane
             )
             ON CONFLICT (user_id, lane, position) DO NOTHING",
        ),
        "riot_accounts" => "WITH source AS (
                 SELECT puuid, user_id, platform, routing_region, game_name, tag_line,
                        normalized_game_name, normalized_tag_line, profile_icon_id, is_main,
                        main_position, main_position_updated_at, last_synced_at, created_at,
                        updated_at, origin
                 FROM jsonb_populate_recordset(NULL::riot_accounts, $1::jsonb)
             )
             INSERT INTO riot_accounts (
                 puuid, user_id, platform, routing_region, game_name, tag_line,
                 normalized_game_name, normalized_tag_line, profile_icon_id, is_main,
                 main_position, main_position_updated_at, last_synced_at, created_at,
                 updated_at, origin
             )
             SELECT puuid, user_id, platform, routing_region, game_name, tag_line,
                    normalized_game_name, normalized_tag_line, profile_icon_id, is_main,
                    main_position, main_position_updated_at, last_synced_at, created_at,
                    updated_at, origin
             FROM source
             WHERE NOT EXISTS (
                 SELECT 1
                 FROM riot_accounts AS existing
                 WHERE existing.user_id = source.user_id
             )
               AND NOT EXISTS (
                 SELECT 1
                 FROM riot_accounts AS existing
                 WHERE existing.puuid = source.puuid
             )
               AND CASE
                 WHEN source.origin = 'RIOT' THEN NOT EXISTS (
                     SELECT 1
                     FROM riot_accounts AS existing
                     WHERE existing.origin = 'RIOT'
                       AND existing.routing_region = source.routing_region
                       AND existing.normalized_game_name = source.normalized_game_name
                       AND existing.normalized_tag_line = source.normalized_tag_line
                 )
                 ELSE true
             END
             ON CONFLICT (puuid) DO NOTHING"
            .to_owned(),
        "dummy_user_profiles" => String::from(
            "INSERT INTO dummy_user_profiles
             SELECT * FROM jsonb_populate_recordset(NULL::dummy_user_profiles, $1::jsonb)
             ON CONFLICT (user_id) DO NOTHING",
        ),
        "guild_members" => String::from(
            "INSERT INTO guild_members
             SELECT * FROM jsonb_populate_recordset(NULL::guild_members, $1::jsonb)
             ON CONFLICT (guild_id, user_id) DO NOTHING",
        ),
        "user_guild_preferences" => String::from(
            "INSERT INTO user_guild_preferences
             SELECT * FROM jsonb_populate_recordset(NULL::user_guild_preferences, $1::jsonb)
             ON CONFLICT (user_id) DO NOTHING",
        ),
        "guild_counters" => "INSERT INTO guild_counters
             SELECT * FROM jsonb_populate_recordset(NULL::guild_counters, $1::jsonb)
             ON CONFLICT (guild_id, counter_key) DO UPDATE
             SET next_value = GREATEST(guild_counters.next_value, EXCLUDED.next_value)"
            .to_owned(),
        "admin_audit_log" => String::from(
            "INSERT INTO admin_audit_log
             SELECT * FROM jsonb_populate_recordset(NULL::admin_audit_log, $1::jsonb)
             ON CONFLICT (guild_id, id) DO NOTHING",
        ),
        _ if ARCHIVE_TABLE_NAMES.contains(&table) => format!(
            "INSERT INTO {table}
             SELECT * FROM jsonb_populate_recordset(NULL::{table}, $1::jsonb)"
        ),
        _ => return Err(GuildFullDataTransferRepositoryError::InvariantViolation),
    };
    // `table` is accepted only from `ARCHIVE_TABLE_NAMES`; row data is never interpolated.
    sqlx::query(AssertSqlSafe(query.as_str()))
        .bind(payload)
        .execute(&mut *connection)
        .await
        .map_err(|source| archive_database_error(table, source))?;
    Ok(())
}

async fn restore_tournament_terminal_state(
    connection: &mut PgConnection,
    rows: &[Value],
) -> Result<(), GuildFullDataTransferRepositoryError> {
    if rows.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "UPDATE auction_tournaments AS target
         SET status = source.status,
             champion_team_id = source.champion_team_id,
             ended_at = source.ended_at
         FROM jsonb_populate_recordset(NULL::auction_tournaments, $1::jsonb) AS source
         WHERE target.guild_id = source.guild_id AND target.id = source.id",
    )
    .bind(Value::Array(rows.to_vec()))
    .execute(&mut *connection)
    .await
    .map_err(|source| archive_database_error("auction_tournaments", source))?;
    Ok(())
}

fn archive_database_error(
    table: &str,
    source: sqlx::Error,
) -> GuildFullDataTransferRepositoryError {
    if source.as_database_error().is_some() {
        GuildFullDataTransferRepositoryError::ArchiveConstraintViolation {
            table: table.to_owned(),
        }
    } else {
        GuildFullDataTransferRepositoryError::Database(source)
    }
}

async fn target_domain_rows(
    connection: &mut PgConnection,
    guild_id: &GuildId,
) -> Result<i64, GuildFullDataTransferRepositoryError> {
    let union = DOMAIN_TABLE_NAMES
        .into_iter()
        .map(|table| {
            format!("SELECT count(*)::bigint AS row_count FROM {table} WHERE guild_id = $1")
        })
        .collect::<Vec<_>>()
        .join(" UNION ALL ");
    let query = format!("SELECT COALESCE(sum(row_count), 0)::bigint FROM ({union}) AS counts");
    // The UNION is generated exclusively from `DOMAIN_TABLE_NAMES`.
    Ok(sqlx::query_scalar(AssertSqlSafe(query.as_str()))
        .bind(guild_id)
        .fetch_one(&mut *connection)
        .await?)
}

async fn target_import_state(
    connection: &mut PgConnection,
    guild_id: &GuildId,
) -> Result<TargetImportState, GuildFullDataTransferRepositoryError> {
    let domain_rows = target_domain_rows(connection, guild_id).await?;
    let target_active_season_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT season.id
         FROM seasons AS season
         WHERE season.guild_id = $1
           AND season.status = 'ACTIVE'
           AND season.ended_at IS NULL",
    )
    .bind(guild_id)
    .fetch_optional(&mut *connection)
    .await?;
    let active_domain_rows = if let Some(active_season_id) = target_active_season_id {
        let union = ACTIVE_SEASON_DOMAIN_TABLE_NAMES
            .into_iter()
            .map(|table| {
                format!(
                    "SELECT count(*)::bigint AS row_count FROM {table}
                     WHERE guild_id = $1 AND season_id = $2"
                )
            })
            .collect::<Vec<_>>()
            .join(" UNION ALL ");
        let query = format!("SELECT COALESCE(sum(row_count), 0)::bigint FROM ({union}) AS counts");
        // The UNION is generated exclusively from `ACTIVE_SEASON_DOMAIN_TABLE_NAMES`.
        sqlx::query_scalar(AssertSqlSafe(query.as_str()))
            .bind(guild_id)
            .bind(active_season_id)
            .fetch_one(&mut *connection)
            .await?
    } else {
        0
    };
    let (recruitment, series, auction_recruitment, auction_tournament) =
        sqlx::query_as::<_, (i64, i64, i64, i64)>(
            "SELECT
                 GREATEST(
                     COALESCE((SELECT max(display_number) FROM recruitments WHERE guild_id = $1), 0),
                     COALESCE((SELECT max(next_value - 1) FROM guild_counters
                               WHERE guild_id = $1 AND counter_key = 'RECRUITMENT'), 0)
                 )::bigint,
                 GREATEST(
                     COALESCE((SELECT max(display_number) FROM series WHERE guild_id = $1), 0),
                     COALESCE((SELECT max(next_value - 1) FROM guild_counters
                               WHERE guild_id = $1 AND counter_key = 'SERIES'), 0)
                 )::bigint,
                 GREATEST(
                     COALESCE((SELECT max(display_number) FROM auction_recruitments WHERE guild_id = $1), 0),
                     COALESCE((SELECT max(next_value - 1) FROM guild_counters
                               WHERE guild_id = $1 AND counter_key = 'AUCTION_RECRUITMENT'), 0)
                 )::bigint,
                 GREATEST(
                     COALESCE((SELECT max(display_number) FROM auction_tournaments WHERE guild_id = $1), 0),
                     COALESCE((SELECT max(next_value - 1) FROM guild_counters
                               WHERE guild_id = $1 AND counter_key = 'AUCTION_TOURNAMENT'), 0)
                 )::bigint",
        )
        .bind(guild_id)
        .fetch_one(&mut *connection)
        .await?;
    Ok(TargetImportState {
        domain_rows,
        active_domain_rows,
        target_active_season_id,
        display_number_bases: DisplayNumberBases {
            recruitment,
            series,
            auction_recruitment,
            auction_tournament,
        },
    })
}

async fn target_state_digest(
    connection: &mut PgConnection,
    guild_id: &GuildId,
    target_state: TargetImportState,
) -> Result<Vec<u8>, GuildFullDataTransferRepositoryError> {
    let members = sqlx::query_scalar::<_, Value>(
        "SELECT COALESCE(
             jsonb_agg(to_jsonb(row_data) ORDER BY row_data.user_id),
             '[]'::jsonb
         )
         FROM (
             SELECT user_id, status, main_lane, registered_at, inactivated_at
             FROM guild_members WHERE guild_id = $1
         ) AS row_data",
    )
    .bind(guild_id)
    .fetch_one(&mut *connection)
    .await?;
    let counters = sqlx::query_scalar::<_, Value>(
        "SELECT COALESCE(
             jsonb_agg(to_jsonb(row_data) ORDER BY row_data.counter_key),
             '[]'::jsonb
         )
         FROM (
             SELECT counter_key, next_value FROM guild_counters WHERE guild_id = $1
         ) AS row_data",
    )
    .bind(guild_id)
    .fetch_one(&mut *connection)
    .await?;
    let admin_state = sqlx::query_scalar::<_, Value>(
        "SELECT COALESCE(
             jsonb_agg(to_jsonb(row_data) ORDER BY row_data.scope),
             '[]'::jsonb
         )
         FROM (
             SELECT scope, revision FROM guild_admin_state WHERE guild_id = $1
         ) AS row_data",
    )
    .bind(guild_id)
    .fetch_one(&mut *connection)
    .await?;
    let seasons = sqlx::query_scalar::<_, Value>(
        "SELECT COALESCE(
             jsonb_agg(to_jsonb(row_data) ORDER BY row_data.id),
             '[]'::jsonb
         )
         FROM (
             SELECT id, name, status, started_at, ended_at, created_by, created_at
             FROM seasons WHERE guild_id = $1
         ) AS row_data",
    )
    .bind(guild_id)
    .fetch_one(&mut *connection)
    .await?;
    let bytes = serde_json::to_vec(&json!({
        "members": members,
        "counters": counters,
        "adminState": admin_state,
        "seasons": seasons,
        "domainRows": target_state.domain_rows,
        "activeDomainRows": target_state.active_domain_rows,
        "targetActiveSeasonId": target_state.target_active_season_id,
        "displayNumberBases": target_state.display_number_bases,
    }))?;
    Ok(digest::digest(&digest::SHA256, &bytes).as_ref().to_vec())
}

async fn authorize_operator(
    connection: &mut PgConnection,
    guild_id: &GuildId,
    actor_id: &UserId,
) -> Result<(), GuildFullDataTransferRepositoryError> {
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
        Err(GuildFullDataTransferRepositoryError::OperatorRequired)
    } else {
        Err(GuildFullDataTransferRepositoryError::GuildNotFound)
    }
}

async fn lock_guild(
    connection: &mut PgConnection,
    guild_id: &GuildId,
) -> Result<(), GuildFullDataTransferRepositoryError> {
    let found = sqlx::query_scalar::<_, bool>(
        "SELECT true FROM guilds WHERE guild_id = $1 AND status = 'ACTIVE' FOR UPDATE",
    )
    .bind(guild_id)
    .fetch_optional(&mut *connection)
    .await?;
    if found.is_some() {
        Ok(())
    } else {
        Err(GuildFullDataTransferRepositoryError::GuildNotFound)
    }
}

async fn guild_display_name(
    connection: &mut PgConnection,
    guild_id: &GuildId,
) -> Result<String, GuildFullDataTransferRepositoryError> {
    sqlx::query_scalar::<_, String>("SELECT display_name FROM guild_config WHERE guild_id = $1")
        .bind(guild_id)
        .fetch_optional(&mut *connection)
        .await?
        .filter(|name| valid_text(name))
        .ok_or(GuildFullDataTransferRepositoryError::InvariantViolation)
}

#[allow(clippy::too_many_arguments)]
async fn insert_audit(
    transaction: &mut PgConnection,
    guild_id: &GuildId,
    actor_id: &UserId,
    action: &str,
    target_type: &str,
    target_id: &str,
    request_id: &str,
    before_summary: Value,
    after_summary: Value,
    now: OffsetDateTime,
) -> Result<(), GuildFullDataTransferRepositoryError> {
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
    .execute(&mut *transaction)
    .await?;
    Ok(())
}

async fn lock_import(
    connection: &mut PgConnection,
    guild_id: &GuildId,
    import_id: Uuid,
) -> Result<StoredFullImport, GuildFullDataTransferRepositoryError> {
    sqlx::query_as::<_, StoredFullImport>(
        "SELECT actor_id, status, archive_digest, archive_payload, source_guild_id,
                target_state_digest, summary, conflict_count, expires_at
         FROM guild_full_data_imports
         WHERE guild_id = $1 AND id = $2
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(import_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(GuildFullDataTransferRepositoryError::PreviewNotFound)
}

fn archive_digest(
    archive: &GuildFullSnapshotArchive,
) -> Result<Vec<u8>, GuildFullDataTransferRepositoryError> {
    let bytes = serde_json::to_vec(archive)?;
    Ok(digest::digest(&digest::SHA256, &bytes).as_ref().to_vec())
}

fn request_hash(
    guild_id: &GuildId,
    import_id: Uuid,
    command: &ApplyFullImportCommand,
) -> Result<Vec<u8>, GuildFullDataTransferRepositoryError> {
    let bytes = serde_json::to_vec(&(guild_id, import_id, command))?;
    Ok(digest::digest(&digest::SHA256, &bytes).as_ref().to_vec())
}

fn digest_hex(value: &[u8]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn parse_digest(value: &str) -> Result<Vec<u8>, GuildFullDataTransferRepositoryError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(GuildFullDataTransferRepositoryError::InvalidApplyCommand);
    }
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| GuildFullDataTransferRepositoryError::InvalidApplyCommand)
}

#[derive(Debug, FromRow)]
struct StoredFullImport {
    actor_id: UserId,
    status: String,
    archive_digest: Vec<u8>,
    archive_payload: Option<Value>,
    source_guild_id: GuildId,
    target_state_digest: Vec<u8>,
    summary: Value,
    conflict_count: i32,
    expires_at: OffsetDateTime,
}

#[derive(Debug, Error)]
pub enum GuildFullDataTransferRepositoryError {
    #[error("PostgreSQL full data transfer operation failed")]
    Database(#[from] sqlx::Error),
    #[error("full data transfer serialization failed")]
    Serialization(#[from] serde_json::Error),
    #[error("idempotency operation failed")]
    Idempotency(#[from] IdempotencyRepositoryError),
    #[error("guild was not found or is inaccessible")]
    GuildNotFound,
    #[error("operator permission is required")]
    OperatorRequired,
    #[error("archive is invalid")]
    InvalidArchive,
    #[error("archive exceeds the supported row limit")]
    ArchiveRowLimitExceeded,
    #[error("archive violates a current database constraint in {table}")]
    ArchiveConstraintViolation { table: String },
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
    #[error("preview contains conflicts")]
    PreviewConflicts,
    #[error("target state changed after preview")]
    TargetStateChanged,
    #[error("idempotency key was reused with another request")]
    IdempotencyConflict,
    #[error("idempotent request is already in progress")]
    IdempotencyInProgress,
    #[error("stored full data transfer invariant failed")]
    InvariantViolation,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn archive() -> GuildFullSnapshotArchive {
        GuildFullSnapshotArchive {
            format: ARCHIVE_FORMAT.to_owned(),
            kind: ARCHIVE_KIND.to_owned(),
            schema_version: ARCHIVE_SCHEMA_VERSION,
            exported_at: OffsetDateTime::UNIX_EPOCH,
            source: FullArchiveSource {
                guild_id: GuildId::parse("123456789012345678").expect("guild"),
                guild_name: "Test guild".to_owned(),
            },
            tables: ARCHIVE_TABLE_NAMES
                .into_iter()
                .map(|name| (name.to_owned(), Vec::new()))
                .collect(),
        }
    }

    #[test]
    fn archive_requires_the_exact_v1_table_set() {
        let mut value = archive();
        assert!(validate_archive(&value).is_ok());
        value.tables.remove("games");
        assert!(matches!(
            validate_archive(&value),
            Err(GuildFullDataTransferRepositoryError::InvalidArchive)
        ));
    }

    #[test]
    fn archive_rejects_a_row_for_another_guild() {
        let mut value = archive();
        value.tables.get_mut("seasons").expect("table").push(json!({
            "guild_id": "999999999999999999"
        }));
        assert!(matches!(
            validate_archive(&value),
            Err(GuildFullDataTransferRepositoryError::InvalidArchive)
        ));
    }

    #[test]
    fn archive_rejects_mode_specific_champion_ids_before_preview() {
        for table in CHAMPION_ID_TABLE_NAMES {
            let mut value = archive();
            let mut row = json!({ "champion_id": "Jade_Ahri" });
            if GUILD_SCOPED_TABLE_NAMES.contains(&table) {
                row.as_object_mut().expect("row").insert(
                    "guild_id".to_owned(),
                    Value::String(value.source.guild_id.as_str().to_owned()),
                );
            }
            value.tables.get_mut(table).expect("table").push(row);

            assert!(matches!(
                validate_archive(&value),
                Err(GuildFullDataTransferRepositoryError::ArchiveConstraintViolation {
                    table: rejected_table
                }) if rejected_table == table
            ));
        }
    }

    #[test]
    fn archive_allows_an_unselected_game_participant_champion() {
        let mut value = archive();
        value
            .tables
            .get_mut("game_participants")
            .expect("table")
            .push(json!({
                "guild_id": value.source.guild_id.as_str(),
                "champion_id": null
            }));

        assert!(validate_archive(&value).is_ok());
    }

    #[test]
    fn digest_changes_with_historical_rows() {
        let mut value = archive();
        let original = archive_digest(&value).expect("digest");
        value.tables.get_mut("games").expect("table").push(json!({
            "guild_id": value.source.guild_id.as_str(),
            "id": Uuid::nil(),
        }));
        assert_ne!(original, archive_digest(&value).expect("changed digest"));
    }

    #[test]
    fn active_source_season_is_mapped_to_the_existing_target_season() {
        let source_season =
            Uuid::parse_str("01900000-0000-7000-8000-000000000001").expect("source season");
        let target_season =
            Uuid::parse_str("01900000-0000-7000-8000-000000000002").expect("target season");
        let mut value = archive();
        let guild_id = value.source.guild_id.as_str().to_owned();
        value
            .tables
            .get_mut("seasons")
            .expect("seasons")
            .push(json!({
                "guild_id": guild_id,
                "id": source_season,
                "status": "ACTIVE"
            }));
        value
            .tables
            .get_mut("games")
            .expect("games")
            .push(json!({ "guild_id": guild_id, "season_id": source_season }));
        value
            .tables
            .get_mut("admin_audit_log")
            .expect("audit")
            .push(json!({
                "guild_id": guild_id,
                "target_id": source_season,
                "after_summary": { "seasonId": source_season }
            }));

        let mapped = map_archive_for_target(
            &value,
            &value.source.guild_id,
            Some(target_season),
            DisplayNumberBases::default(),
        )
        .expect("mapped archive");

        assert!(mapped.uses_target_active_season);
        assert!(!mapped.remaps_source_guild);
        assert!(mapped.archive.tables["seasons"].is_empty());
        assert_eq!(
            mapped.archive.tables["games"][0]["season_id"],
            target_season.to_string()
        );
        assert_eq!(
            mapped.archive.tables["admin_audit_log"][0]["after_summary"]["seasonId"],
            target_season.to_string()
        );
        assert_eq!(
            value.tables["seasons"].len(),
            1,
            "source archive is immutable"
        );
    }

    #[test]
    fn display_numbers_are_appended_after_closed_target_history() {
        let mut value = archive();
        let guild_id = value.source.guild_id.as_str().to_owned();
        value
            .tables
            .get_mut("recruitments")
            .expect("recruitments")
            .push(json!({
                "guild_id": guild_id,
                "display_number": 1,
                "title": "일반 내전 모집 #1"
            }));
        value.tables.get_mut("series").expect("series").push(json!({
            "guild_id": guild_id,
            "display_number": 4,
            "title": "사용자 지정 제목"
        }));

        let mapped = map_archive_for_target(
            &value,
            &value.source.guild_id,
            None,
            DisplayNumberBases {
                recruitment: 12,
                series: 8,
                ..DisplayNumberBases::default()
            },
        )
        .expect("mapped archive");

        assert!(mapped.rebases_display_numbers);
        assert_eq!(
            mapped.archive.tables["recruitments"][0]["display_number"],
            13
        );
        assert_eq!(
            mapped.archive.tables["recruitments"][0]["title"],
            "일반 내전 모집 #13"
        );
        assert_eq!(mapped.archive.tables["series"][0]["display_number"], 12);
        assert_eq!(
            mapped.archive.tables["series"][0]["title"],
            "사용자 지정 제목"
        );
        assert!(
            mapped.archive.tables["guild_counters"]
                .iter()
                .any(|row| { row["counter_key"] == "RECRUITMENT" && row["next_value"] == 14 })
        );
        assert!(
            mapped.archive.tables["guild_counters"]
                .iter()
                .any(|row| { row["counter_key"] == "SERIES" && row["next_value"] == 13 })
        );
    }

    #[test]
    fn cross_guild_archive_is_remapped_and_source_discord_links_are_cleared() {
        let mut value = archive();
        let source_guild_id = value.source.guild_id.as_str().to_owned();
        let target_guild_id = GuildId::parse("987654321098765432").expect("target guild");

        value
            .tables
            .get_mut("user_guild_preferences")
            .expect("preferences")
            .push(json!({ "default_guild_id": source_guild_id }));
        for (table, fields) in CROSS_GUILD_DISCORD_LINKS {
            let mut row = json!({
                "guild_id": source_guild_id,
                "nested": { "guildId": source_guild_id }
            });
            let object = row.as_object_mut().expect("row");
            for field in fields {
                object.insert(
                    (*field).to_owned(),
                    Value::String("444444444444444444".to_owned()),
                );
            }
            value.tables.get_mut(table).expect("table").push(row);
        }
        value
            .tables
            .get_mut("admin_audit_log")
            .expect("audit")
            .push(json!({
                "guild_id": source_guild_id,
                "target_id": source_guild_id,
                "after_summary": { "guildId": source_guild_id }
            }));

        validate_archive(&value).expect("source archive");
        let mapped = map_archive_for_target(
            &value,
            &target_guild_id,
            None,
            DisplayNumberBases::default(),
        )
        .expect("mapped archive");

        assert!(mapped.remaps_source_guild);
        assert!(!mapped.uses_target_active_season);
        assert_eq!(mapped.archive.source.guild_id, target_guild_id);
        assert_eq!(
            mapped.archive.tables["user_guild_preferences"][0]["default_guild_id"],
            target_guild_id.as_str()
        );
        assert_eq!(
            mapped.archive.tables["admin_audit_log"][0]["after_summary"]["guildId"],
            target_guild_id.as_str()
        );
        for (table, fields) in CROSS_GUILD_DISCORD_LINKS {
            assert_eq!(
                mapped.archive.tables[table][0]["guild_id"],
                target_guild_id.as_str()
            );
            assert_eq!(
                mapped.archive.tables[table][0]["nested"]["guildId"],
                target_guild_id.as_str()
            );
            for field in fields {
                assert!(mapped.archive.tables[table][0][field].is_null());
                assert!(!value.tables[table][0][field].is_null());
            }
        }
        validate_archive(&mapped.archive).expect("mapped archive is valid");
    }
}
