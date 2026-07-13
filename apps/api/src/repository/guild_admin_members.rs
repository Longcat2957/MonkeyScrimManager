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
    auth::AuthService,
    domain::{DiscordUserId, GuildId, GuildMemberStatus, IdempotencyRecordId, Lane, SeasonId},
    infrastructure::postgres::{
        DatabaseTransaction, DeadlockRetryPolicy, TransactionAttemptError, TransactionRetryError,
        run_with_deadlock_retry,
    },
};

const IDEMPOTENCY_TTL: Duration = Duration::hours(24);
const ADJUST_MMR_METHOD: &str = "PATCH";
const ADJUST_MMR_ROUTE: &str = "/api/guilds/{guildId}/members/{memberId}/mmr";
const MAX_ADJUSTMENT_NOTE_CHARS: usize = 500;
const EXPECTED_LANE_COUNT: usize = 5;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdjustMmrCommand {
    pub lane: Lane,
    pub delta: f64,
    pub note: String,
    pub expected_revision: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetMemberStatusCommand {
    pub status: GuildMemberStatus,
    pub expected_revision: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildMembersSnapshot {
    pub revision: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    pub updated_by: Option<DiscordUserId>,
    pub members: Vec<GuildAdminMember>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildAdminMember {
    pub id: DiscordUserId,
    pub discord_name: String,
    pub riot_id: Option<String>,
    pub role: GuildAdminRole,
    pub main_lane: Option<Lane>,
    pub lane_mmr: LaneMmr,
    pub registered: bool,
    pub status: GuildMemberStatus,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_active_at: Option<OffsetDateTime>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GuildAdminRole {
    Member,
    Operator,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub struct LaneMmr {
    pub top: f64,
    pub jungle: f64,
    pub mid: f64,
    pub bottom: f64,
    pub support: f64,
}

impl Default for LaneMmr {
    fn default() -> Self {
        Self {
            top: 1500.0,
            jungle: 1500.0,
            mid: 1500.0,
            bottom: 1500.0,
            support: 1500.0,
        }
    }
}

impl LaneMmr {
    fn set(&mut self, lane: Lane, mmr: f64) {
        match lane {
            Lane::Top => self.top = mmr,
            Lane::Jungle => self.jungle = mmr,
            Lane::Mid => self.mid = mmr,
            Lane::Bottom => self.bottom = mmr,
            Lane::Support => self.support = mmr,
        }
    }
}

pub async fn get(
    pool: &PgPool,
    guild_id: &GuildId,
    actor_id: &DiscordUserId,
) -> Result<GuildMembersSnapshot, GuildAdminMembersRepositoryError> {
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

pub async fn adjust_mmr(
    pool: &PgPool,
    guild_id: &GuildId,
    member_id: &DiscordUserId,
    actor_id: &DiscordUserId,
    idempotency_key: &str,
    request_id: &str,
    command: AdjustMmrCommand,
) -> Result<GuildMembersSnapshot, GuildAdminMembersRepositoryError> {
    validate_adjustment(&command)?;
    let request_hash = request_hash(guild_id, member_id, &command)?;
    let guild_id = guild_id.clone();
    let member_id = member_id.clone();
    let actor_id = actor_id.clone();
    let idempotency_key = idempotency_key.to_owned();
    let request_id = request_id.to_owned();
    let prepared = MmrPrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let member_id = member_id.clone();
            let actor_id = actor_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let request_hash = request_hash.clone();
            let command = command.clone();

            Box::pin(async move {
                adjust_mmr_attempt(
                    transaction,
                    &guild_id,
                    &member_id,
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

pub async fn set_status(
    pool: &PgPool,
    guild_id: &GuildId,
    member_id: &DiscordUserId,
    actor_id: &DiscordUserId,
    request_id: &str,
    command: SetMemberStatusCommand,
) -> Result<GuildMembersSnapshot, GuildAdminMembersRepositoryError> {
    validate_expected_revision(command.expected_revision)?;
    let guild_id = guild_id.clone();
    let member_id = member_id.clone();
    let actor_id = actor_id.clone();
    let request_id = request_id.to_owned();
    let prepared = StatusPrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let member_id = member_id.clone();
            let actor_id = actor_id.clone();
            let request_id = request_id.clone();
            let command = command.clone();

            Box::pin(async move {
                set_status_attempt(
                    transaction,
                    &guild_id,
                    &member_id,
                    &actor_id,
                    &request_id,
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
async fn adjust_mmr_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    member_id: &DiscordUserId,
    actor_id: &DiscordUserId,
    idempotency_key: &str,
    request_id: &str,
    request_hash: &[u8],
    command: &AdjustMmrCommand,
    prepared: MmrPrepared,
) -> Result<GuildMembersSnapshot, AttemptError> {
    let now = OffsetDateTime::now_utc();
    authorize_operator(transaction, guild_id, actor_id).await?;
    let scope = TenantScope::guild(guild_id.clone());
    let idempotency_record_id = match acquire_idempotency::<GuildMembersSnapshot>(
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

    let state = ensure_and_lock_members_state(transaction, guild_id, actor_id, now).await?;
    ensure_revision(command.expected_revision, &state)?;
    ensure_member_exists(transaction, guild_id, member_id).await?;
    let season_id = lock_active_season(transaction, guild_id).await?;
    let mut locked = mmr::ensure_and_lock_mmr_rows(
        transaction,
        guild_id,
        season_id,
        [MmrLaneKey {
            user_id: member_id.clone(),
            lane: command.lane,
        }],
        now,
    )
    .await?;
    let locked = locked.pop().ok_or(AttemptError::InvariantViolation)?;
    let mmr_after = locked.mmr + command.delta;
    if !mmr_after.is_finite() {
        return Err(AttemptError::InvalidAdjustment);
    }
    let lane_revision = locked
        .revision
        .checked_add(1)
        .ok_or(AttemptError::InvariantViolation)?;

    sqlx::query(
        "INSERT INTO mmr_ledger_entries (
             guild_id, id, season_id, user_id, lane, lane_revision, source_type, source_id,
             game_id, opponent_user_id, mmr_before, mmr_after, delta, games_played_delta,
             wins_delta, reverses_entry_id, reversed_source_type, actor_type, actor_id, note,
             request_id, created_at
         ) VALUES (
             $1, $2, $3, $4, $5, $6, 'MANUAL_ADJUSTMENT', $7,
             NULL, NULL, $8, $9, $10, 0, 0, NULL, NULL, 'USER', $11, $12, $13, $14
         )",
    )
    .bind(guild_id)
    .bind(prepared.ledger_id)
    .bind(season_id.as_uuid())
    .bind(member_id)
    .bind(command.lane)
    .bind(lane_revision)
    .bind(prepared.source_id)
    .bind(locked.mmr)
    .bind(mmr_after)
    .bind(command.delta)
    .bind(actor_id)
    .bind(command.note.trim())
    .bind(request_id)
    .bind(now)
    .execute(&mut **transaction)
    .await?;

    let updated = sqlx::query(
        "UPDATE user_lane_mmr
         SET mmr = $1, revision = $2, updated_at = $3
         WHERE guild_id = $4 AND season_id = $5 AND user_id = $6 AND lane = $7",
    )
    .bind(mmr_after)
    .bind(lane_revision)
    .bind(now)
    .bind(guild_id)
    .bind(season_id.as_uuid())
    .bind(member_id)
    .bind(command.lane)
    .execute(&mut **transaction)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(AttemptError::InvariantViolation);
    }

    increment_members_revision(transaction, guild_id, actor_id, state.revision, now).await?;
    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id,
            action: "MEMBER_MMR_ADJUSTED",
            target_type: "GUILD_MEMBER_MMR",
            target_id: member_id.as_str(),
            request_id,
            before_summary: json!({
                "lane": command.lane,
                "mmr": locked.mmr,
                "laneRevision": locked.revision,
            }),
            after_summary: json!({
                "lane": command.lane,
                "mmr": mmr_after,
                "delta": command.delta,
                "laneRevision": lane_revision,
            }),
            note: Some(command.note.trim()),
            now,
        },
    )
    .await?;

    let snapshot = fetch_snapshot(transaction, guild_id).await?;
    complete_idempotency(transaction, &scope, idempotency_record_id, &snapshot, now).await?;
    Ok(snapshot)
}

async fn set_status_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    member_id: &DiscordUserId,
    actor_id: &DiscordUserId,
    request_id: &str,
    command: &SetMemberStatusCommand,
    prepared: StatusPrepared,
) -> Result<GuildMembersSnapshot, AttemptError> {
    let now = OffsetDateTime::now_utc();
    authorize_operator(transaction, guild_id, actor_id).await?;
    if member_id == actor_id && command.status == GuildMemberStatus::Inactive {
        return Err(AttemptError::SelfDeactivationForbidden);
    }
    let state = ensure_and_lock_members_state(transaction, guild_id, actor_id, now).await?;
    ensure_revision(command.expected_revision, &state)?;
    let previous_status = lock_member_status(transaction, guild_id, member_id).await?;

    let updated = sqlx::query(
        "UPDATE guild_members
         SET status = $1,
             inactivated_at = CASE WHEN $1 = 'INACTIVE' THEN $2 ELSE NULL END
         WHERE guild_id = $3 AND user_id = $4",
    )
    .bind(command.status)
    .bind(now)
    .bind(guild_id)
    .bind(member_id)
    .execute(&mut **transaction)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(AttemptError::InvariantViolation);
    }

    increment_members_revision(transaction, guild_id, actor_id, state.revision, now).await?;
    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id,
            action: "MEMBER_STATUS_CHANGED",
            target_type: "GUILD_MEMBER",
            target_id: member_id.as_str(),
            request_id,
            before_summary: json!({ "status": previous_status }),
            after_summary: json!({ "status": command.status }),
            note: None,
            now,
        },
    )
    .await?;
    fetch_snapshot(transaction, guild_id).await
}

async fn authorize_operator(
    connection: &mut PgConnection,
    guild_id: &GuildId,
    actor_id: &DiscordUserId,
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
) -> Result<GuildMembersSnapshot, AttemptError> {
    let metadata = sqlx::query_as::<_, SnapshotMetadataRow>(
        "SELECT COALESCE(admin_state.revision, 0) AS revision,
                COALESCE(admin_state.updated_at, config.updated_at, guild.updated_at) AS updated_at,
                COALESCE(admin_state.updated_by, config.updated_by) AS updated_by
         FROM guilds AS guild
         JOIN guild_config AS config ON config.guild_id = guild.guild_id
         LEFT JOIN guild_admin_state AS admin_state
           ON admin_state.guild_id = guild.guild_id AND admin_state.scope = 'MEMBERS'
         WHERE guild.guild_id = $1 AND guild.status = 'ACTIVE'",
    )
    .bind(guild_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(AttemptError::GuildNotFound)?;

    let rows = sqlx::query_as::<_, MemberLaneRow>(
        "WITH canonical_lanes (lane, lane_order) AS (
             VALUES ('TOP'::text, 1), ('JUNGLE', 2), ('MID', 3), ('BOTTOM', 4), ('SUPPORT', 5)
         ), active_season AS (
             SELECT id
             FROM seasons
             WHERE guild_id = $1 AND status = 'ACTIVE'
         )
         SELECT member.user_id,
                member.display_name,
                riot.riot_id,
                EXISTS (
                    SELECT 1
                    FROM guild_member_discord_roles AS member_role
                    WHERE member_role.guild_id = member.guild_id
                      AND member_role.user_id = member.user_id
                      AND member_role.role_id = config.operator_role_id
                ) AS is_operator,
                member.main_lane,
                member.registered_at IS NOT NULL AS registered,
                member.status,
                member.last_active_at,
                canonical_lanes.lane AS mmr_lane,
                COALESCE(lane_mmr.mmr, 1500::double precision) AS mmr
         FROM guild_members AS member
         JOIN guild_config AS config ON config.guild_id = member.guild_id
         CROSS JOIN canonical_lanes
         LEFT JOIN active_season ON true
         LEFT JOIN user_lane_mmr AS lane_mmr
           ON lane_mmr.guild_id = member.guild_id
          AND lane_mmr.season_id = active_season.id
          AND lane_mmr.user_id = member.user_id
          AND lane_mmr.lane = canonical_lanes.lane
         LEFT JOIN LATERAL (
             SELECT account.game_name || '#' || account.tag_line AS riot_id
             FROM riot_accounts AS account
             WHERE account.user_id = member.user_id AND account.is_main
             LIMIT 1
         ) AS riot ON true
         WHERE member.guild_id = $1
         ORDER BY CASE member.status WHEN 'ACTIVE' THEN 0 ELSE 1 END,
                  lower(member.display_name), member.user_id, canonical_lanes.lane_order",
    )
    .bind(guild_id)
    .fetch_all(&mut *connection)
    .await?;

    Ok(GuildMembersSnapshot {
        revision: metadata.revision,
        updated_at: metadata.updated_at,
        updated_by: metadata.updated_by,
        members: members_from_rows(rows)?,
    })
}

fn members_from_rows(rows: Vec<MemberLaneRow>) -> Result<Vec<GuildAdminMember>, AttemptError> {
    let mut members = Vec::new();
    let mut current: Option<MemberAccumulator> = None;

    for row in rows {
        let starts_next = current
            .as_ref()
            .is_some_and(|current| current.member.id != row.user_id);
        if starts_next {
            members.push(
                current
                    .take()
                    .ok_or(AttemptError::InvariantViolation)?
                    .finish()?,
            );
        }

        let accumulator = current.get_or_insert_with(|| MemberAccumulator::new(&row));
        accumulator.member.lane_mmr.set(row.mmr_lane, row.mmr);
        accumulator.lane_count += 1;
    }

    if let Some(accumulator) = current {
        members.push(accumulator.finish()?);
    }
    Ok(members)
}

async fn ensure_and_lock_members_state(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &DiscordUserId,
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
        "SELECT revision, updated_at, updated_by
         FROM guild_admin_state
         WHERE guild_id = $1 AND scope = 'MEMBERS'
         FOR UPDATE",
    )
    .bind(guild_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::InvariantViolation)
}

fn ensure_revision(expected: i64, state: &LockedMembersState) -> Result<(), AttemptError> {
    if expected == state.revision {
        Ok(())
    } else {
        Err(AttemptError::RevisionConflict {
            expected,
            current: state.revision,
            updated_at: state.updated_at,
            updated_by: state.updated_by.clone(),
        })
    }
}

async fn ensure_member_exists(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    member_id: &DiscordUserId,
) -> Result<(), AttemptError> {
    lock_member_status(transaction, guild_id, member_id)
        .await
        .map(|_| ())
}

async fn lock_member_status(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    member_id: &DiscordUserId,
) -> Result<GuildMemberStatus, AttemptError> {
    sqlx::query_scalar::<_, GuildMemberStatus>(
        "SELECT status
         FROM guild_members
         WHERE guild_id = $1 AND user_id = $2
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(member_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::MemberNotFound)
}

async fn lock_active_season(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
) -> Result<SeasonId, AttemptError> {
    sqlx::query_scalar::<_, SeasonId>(
        "SELECT id
         FROM seasons
         WHERE guild_id = $1 AND status = 'ACTIVE'
         FOR UPDATE",
    )
    .bind(guild_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::NoActiveSeason)
}

async fn increment_members_revision(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &DiscordUserId,
    current_revision: i64,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
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
    Ok(())
}

async fn insert_audit(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    audit: AuditInsert<'_>,
) -> Result<(), AttemptError> {
    sqlx::query(
        "INSERT INTO admin_audit_log (
             guild_id, id, principal_type, principal_id, action, target_type, target_id,
             checkpoint_id, request_id, before_summary, after_summary, result, note, created_at
         ) VALUES ($1, $2, 'USER', $3, $4, $5, $6, NULL, $7, $8, $9, 'APPLIED', $10, $11)",
    )
    .bind(guild_id)
    .bind(audit.id)
    .bind(audit.actor_id)
    .bind(audit.action)
    .bind(audit.target_type)
    .bind(audit.target_id)
    .bind(audit.request_id)
    .bind(audit.before_summary)
    .bind(audit.after_summary)
    .bind(audit.note)
    .bind(audit.now)
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
        method: ADJUST_MMR_METHOD,
        route_template: ADJUST_MMR_ROUTE,
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
    snapshot: &GuildMembersSnapshot,
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

fn validate_adjustment(command: &AdjustMmrCommand) -> Result<(), GuildAdminMembersRepositoryError> {
    validate_expected_revision(command.expected_revision)?;
    let note = command.note.trim();
    if !command.delta.is_finite()
        || command.delta == 0.0
        || note.is_empty()
        || note.chars().count() > MAX_ADJUSTMENT_NOTE_CHARS
    {
        return Err(GuildAdminMembersRepositoryError::InvalidAdjustment);
    }
    Ok(())
}

fn validate_expected_revision(
    expected_revision: i64,
) -> Result<(), GuildAdminMembersRepositoryError> {
    if expected_revision < 0 {
        Err(GuildAdminMembersRepositoryError::InvalidExpectedRevision)
    } else {
        Ok(())
    }
}

fn request_hash<T: Serialize>(
    guild_id: &GuildId,
    member_id: &DiscordUserId,
    command: &T,
) -> Result<Vec<u8>, GuildAdminMembersRepositoryError> {
    let canonical = serde_json::to_string(&(guild_id, member_id, command))?;
    Ok(AuthService::hash_secret(&canonical))
}

fn attempt_sql_error(error: &AttemptError) -> Option<&sqlx::Error> {
    match error {
        AttemptError::Sql(error) => Some(error),
        AttemptError::Mmr(MmrRepositoryError::Sql(error)) => Some(error),
        AttemptError::Idempotency(IdempotencyRepositoryError::Sql(error)) => Some(error),
        _ => None,
    }
}

fn map_transaction_error(
    error: TransactionRetryError<AttemptError>,
) -> GuildAdminMembersRepositoryError {
    match error {
        TransactionRetryError::DeadlockRetriesExhausted { .. } => {
            GuildAdminMembersRepositoryError::DatabaseBusy
        }
        TransactionRetryError::NonRetryable { failure, .. } => match failure {
            TransactionAttemptError::Begin(error) | TransactionAttemptError::Commit(error) => {
                GuildAdminMembersRepositoryError::Database(error)
            }
            TransactionAttemptError::Operation(error) => error.into_public(),
        },
    }
}

#[derive(Debug, FromRow)]
struct SnapshotMetadataRow {
    revision: i64,
    updated_at: OffsetDateTime,
    updated_by: Option<DiscordUserId>,
}

#[derive(Debug, FromRow)]
struct MemberLaneRow {
    user_id: DiscordUserId,
    display_name: String,
    riot_id: Option<String>,
    is_operator: bool,
    main_lane: Option<Lane>,
    registered: bool,
    status: GuildMemberStatus,
    last_active_at: Option<OffsetDateTime>,
    mmr_lane: Lane,
    mmr: f64,
}

struct MemberAccumulator {
    member: GuildAdminMember,
    lane_count: usize,
}

impl MemberAccumulator {
    fn new(row: &MemberLaneRow) -> Self {
        Self {
            member: GuildAdminMember {
                id: row.user_id.clone(),
                discord_name: row.display_name.clone(),
                riot_id: row.riot_id.clone(),
                role: if row.is_operator {
                    GuildAdminRole::Operator
                } else {
                    GuildAdminRole::Member
                },
                main_lane: row.main_lane,
                lane_mmr: LaneMmr::default(),
                registered: row.registered,
                status: row.status,
                last_active_at: row.last_active_at,
            },
            lane_count: 0,
        }
    }

    fn finish(self) -> Result<GuildAdminMember, AttemptError> {
        if self.lane_count == EXPECTED_LANE_COUNT {
            Ok(self.member)
        } else {
            Err(AttemptError::InvariantViolation)
        }
    }
}

#[derive(Debug, FromRow)]
struct LockedMembersState {
    revision: i64,
    updated_at: OffsetDateTime,
    updated_by: Option<DiscordUserId>,
}

#[derive(Clone, Copy)]
struct MmrPrepared {
    idempotency_record_id: IdempotencyRecordId,
    ledger_id: Uuid,
    source_id: Uuid,
    audit_id: Uuid,
}

impl MmrPrepared {
    fn new() -> Self {
        Self {
            idempotency_record_id: IdempotencyRecordId::new(),
            ledger_id: Uuid::now_v7(),
            source_id: Uuid::now_v7(),
            audit_id: Uuid::now_v7(),
        }
    }
}

#[derive(Clone, Copy)]
struct StatusPrepared {
    audit_id: Uuid,
}

impl StatusPrepared {
    fn new() -> Self {
        Self {
            audit_id: Uuid::now_v7(),
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

struct AuditInsert<'a> {
    id: Uuid,
    actor_id: &'a DiscordUserId,
    action: &'a str,
    target_type: &'a str,
    target_id: &'a str,
    request_id: &'a str,
    before_summary: Value,
    after_summary: Value,
    note: Option<&'a str>,
    now: OffsetDateTime,
}

#[derive(Debug, Error)]
enum AttemptError {
    #[error("PostgreSQL guild admin member operation failed")]
    Sql(#[from] sqlx::Error),
    #[error("MMR repository operation failed")]
    Mmr(#[from] MmrRepositoryError),
    #[error("idempotency repository operation failed")]
    Idempotency(#[from] IdempotencyRepositoryError),
    #[error("guild admin member serialization failed")]
    Serialization(#[from] serde_json::Error),
    #[error("guild was not found or is inaccessible")]
    GuildNotFound,
    #[error("operator permission is required")]
    OperatorRequired,
    #[error("guild member was not found")]
    MemberNotFound,
    #[error("an operator cannot deactivate their own guild membership")]
    SelfDeactivationForbidden,
    #[error("guild has no active season")]
    NoActiveSeason,
    #[error("expected revision {expected}, current revision {current}")]
    RevisionConflict {
        expected: i64,
        current: i64,
        updated_at: OffsetDateTime,
        updated_by: Option<DiscordUserId>,
    },
    #[error("MMR adjustment is invalid")]
    InvalidAdjustment,
    #[error("idempotency key was reused with another request")]
    IdempotencyConflict,
    #[error("idempotent request is already in progress")]
    IdempotencyInProgress,
    #[error("stored idempotent response is corrupt")]
    CorruptIdempotentResponse,
    #[error("idempotency completion row was not in progress")]
    IdempotencyCompletionLost,
    #[error("stored guild admin member invariant was violated")]
    InvariantViolation,
}

impl AttemptError {
    fn into_public(self) -> GuildAdminMembersRepositoryError {
        match self {
            Self::Sql(error) => GuildAdminMembersRepositoryError::Database(error),
            Self::Mmr(MmrRepositoryError::Sql(error)) => {
                GuildAdminMembersRepositoryError::Database(error)
            }
            Self::Idempotency(IdempotencyRepositoryError::Sql(error)) => {
                GuildAdminMembersRepositoryError::Database(error)
            }
            Self::Serialization(error) => GuildAdminMembersRepositoryError::Serialization(error),
            Self::GuildNotFound => GuildAdminMembersRepositoryError::GuildNotFound,
            Self::OperatorRequired => GuildAdminMembersRepositoryError::OperatorRequired,
            Self::MemberNotFound => GuildAdminMembersRepositoryError::MemberNotFound,
            Self::SelfDeactivationForbidden => {
                GuildAdminMembersRepositoryError::SelfDeactivationForbidden
            }
            Self::NoActiveSeason => GuildAdminMembersRepositoryError::NoActiveSeason,
            Self::RevisionConflict {
                expected,
                current,
                updated_at,
                updated_by,
            } => GuildAdminMembersRepositoryError::RevisionConflict {
                expected,
                current,
                updated_at,
                updated_by,
            },
            Self::InvalidAdjustment => GuildAdminMembersRepositoryError::InvalidAdjustment,
            Self::IdempotencyConflict => GuildAdminMembersRepositoryError::IdempotencyConflict,
            Self::IdempotencyInProgress => GuildAdminMembersRepositoryError::IdempotencyInProgress,
            Self::Mmr(_)
            | Self::Idempotency(_)
            | Self::CorruptIdempotentResponse
            | Self::IdempotencyCompletionLost
            | Self::InvariantViolation => GuildAdminMembersRepositoryError::InvariantViolation,
        }
    }
}

#[derive(Debug, Error)]
pub enum GuildAdminMembersRepositoryError {
    #[error("PostgreSQL guild admin member operation failed")]
    Database(#[from] sqlx::Error),
    #[error("guild admin member serialization failed")]
    Serialization(#[from] serde_json::Error),
    #[error("guild was not found or is inaccessible")]
    GuildNotFound,
    #[error("operator permission is required")]
    OperatorRequired,
    #[error("guild member was not found")]
    MemberNotFound,
    #[error("an operator cannot deactivate their own guild membership")]
    SelfDeactivationForbidden,
    #[error("guild has no active season")]
    NoActiveSeason,
    #[error("expected revision {expected}, current revision {current}")]
    RevisionConflict {
        expected: i64,
        current: i64,
        updated_at: OffsetDateTime,
        updated_by: Option<DiscordUserId>,
    },
    #[error("MMR adjustment is invalid")]
    InvalidAdjustment,
    #[error("expected revision is invalid")]
    InvalidExpectedRevision,
    #[error("idempotency key was reused with another request")]
    IdempotencyConflict,
    #[error("idempotent request is already in progress")]
    IdempotencyInProgress,
    #[error("database remained busy after bounded retries")]
    DatabaseBusy,
    #[error("stored guild admin member invariant was violated")]
    InvariantViolation,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjustment_validation_requires_a_revision_delta_and_bounded_note() {
        let valid = AdjustMmrCommand {
            lane: Lane::Mid,
            delta: 25.5,
            note: "manual correction".to_owned(),
            expected_revision: 0,
        };
        assert!(validate_adjustment(&valid).is_ok());

        for invalid in [
            AdjustMmrCommand {
                delta: 0.0,
                ..valid.clone()
            },
            AdjustMmrCommand {
                note: "   ".to_owned(),
                ..valid.clone()
            },
            AdjustMmrCommand {
                expected_revision: -1,
                ..valid.clone()
            },
        ] {
            assert!(validate_adjustment(&invalid).is_err());
        }
    }

    #[test]
    fn lane_mmr_serializes_all_five_canonical_keys() {
        let value = serde_json::to_value(LaneMmr::default()).expect("serialize lane MMR");
        assert_eq!(
            value,
            json!({
                "TOP": 1500.0,
                "JUNGLE": 1500.0,
                "MID": 1500.0,
                "BOTTOM": 1500.0,
                "SUPPORT": 1500.0,
            })
        );
    }
}
