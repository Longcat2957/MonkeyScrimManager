use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sqlx::{FromRow, PgPool};
use thiserror::Error;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use super::{
    TenantScope,
    idempotency::{
        self, AcquireIdempotency, AcquireIdempotencyOutcome, CompleteIdempotency,
        CompleteIdempotencyOutcome, IdempotencyPrincipalType, IdempotencyRepositoryError,
        IdempotentResponse, PreflightIdempotency, PreflightIdempotencyOutcome,
    },
    riot::{
        self, CanonicalRiotAccount, RiotAccountLinkState, RiotAccountRecord, RiotRepositoryError,
    },
};
use crate::{
    auth::AuthService,
    domain::{
        DiscordChannelId, DiscordInteractionId, DiscordMessageId, DiscordRoleId, DiscordUserId,
        GuildId, GuildMemberStatus, GuildStatus, IdempotencyRecordId, Lane, OutboxEventId,
        RecruitmentId, RecruitmentStatus, SeasonId, UserId,
    },
    infrastructure::postgres::{
        DatabaseTransaction, DeadlockRetryPolicy, TransactionAttemptError, TransactionRetryError,
        run_with_deadlock_retry,
    },
    state::RequestId,
};

const BOT_PRINCIPAL_ID: &str = "monkeybot-discord-bot";
const IDEMPOTENCY_TTL: Duration = Duration::hours(24);
const RECRUITMENT_TARGET_COUNT: i16 = 10;
const MAX_PREFERRED_LANES: usize = 5;
const SETUP_METHOD: &str = "POST";
const SETUP_ROUTE: &str = "/internal/guilds/{guildId}/setup";
const REGISTRATION_METHOD: &str = "POST";
const REGISTRATION_ROUTE: &str = "/internal/guilds/{guildId}/registrations";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BotActor {
    pub id: DiscordUserId,
    pub global_display_name: String,
    pub display_name: String,
    pub avatar_hash: Option<String>,
    pub role_ids: Vec<DiscordRoleId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupGuildCommand {
    pub actor: BotActor,
    pub guild_name: String,
    pub guild_icon_hash: Option<String>,
    pub operator_role_id: DiscordRoleId,
    pub season_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BotSeasonSnapshot {
    pub id: SeasonId,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupGuildResult {
    pub guild_id: GuildId,
    pub display_name: String,
    pub operator_role_id: DiscordRoleId,
    pub current_season: BotSeasonSnapshot,
    pub config_revision: i64,
    pub dashboard_href: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterGuildMemberInput {
    pub actor: BotActor,
    pub requested_game_name: String,
    pub requested_tag_line: String,
    pub requested_platform: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegisterGuildMemberCommand {
    pub input: RegisterGuildMemberInput,
    pub account: CanonicalRiotAccount,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RegistrationState {
    Created,
    Existing,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisteredGuildMemberSnapshot {
    pub id: DiscordUserId,
    pub display_name: String,
    pub status: GuildMemberStatus,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisteredRiotAccountSnapshot {
    pub puuid: String,
    pub game_name: String,
    pub tag_line: String,
    pub platform: String,
    pub is_main: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterGuildMemberResult {
    pub member: RegisteredGuildMemberSnapshot,
    pub riot_account: RegisteredRiotAccountSnapshot,
    pub registration: RegistrationState,
    pub account_link: RegistrationState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BotRecruitmentCommand {
    Create {
        actor: BotActor,
    },
    BindMessage {
        actor: BotActor,
        recruitment_id: RecruitmentId,
        channel_id: DiscordChannelId,
        message_id: DiscordMessageId,
    },
    Join {
        actor: BotActor,
        recruitment_id: RecruitmentId,
        participant: BotActor,
    },
    Leave {
        actor: BotActor,
        recruitment_id: RecruitmentId,
        user_id: UserId,
    },
    SetPreferredLanes {
        actor: BotActor,
        recruitment_id: RecruitmentId,
        user_id: UserId,
        lanes: Vec<Lane>,
    },
    Close {
        actor: BotActor,
        recruitment_id: RecruitmentId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecruitmentSummarySnapshot {
    pub id: RecruitmentId,
    pub display_number: i64,
    pub title: String,
    pub status: RecruitmentStatus,
    pub target_count: i16,
    pub revision: i64,
    pub channel_id: Option<DiscordChannelId>,
    pub message_id: Option<DiscordMessageId>,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecruitmentParticipantSnapshot {
    pub id: UserId,
    pub display_name: String,
    pub preferred_lanes: Vec<Lane>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecruitmentLinksSnapshot {
    pub entry: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecruitmentSnapshot {
    pub recruitment: RecruitmentSummarySnapshot,
    pub participants: Vec<RecruitmentParticipantSnapshot>,
    pub links: RecruitmentLinksSnapshot,
}

pub async fn setup_guild(
    pool: &PgPool,
    guild_id: &GuildId,
    idempotency_key: &DiscordInteractionId,
    request_id: &RequestId,
    command: SetupGuildCommand,
) -> Result<SetupGuildResult, BotWorkflowError> {
    validate_setup_command(&command)?;

    let request_hash = request_hash(guild_id, &command)?;
    let guild_id = guild_id.clone();
    let idempotency_key = idempotency_key.clone();
    let request_id = request_id.0.clone();
    let prepared = SetupPrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let command = command.clone();
            let request_hash = request_hash.clone();

            Box::pin(async move {
                setup_guild_attempt(
                    transaction,
                    &guild_id,
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

pub async fn register_guild_member(
    pool: &PgPool,
    guild_id: &GuildId,
    idempotency_key: &DiscordInteractionId,
    command: RegisterGuildMemberCommand,
) -> Result<RegisterGuildMemberResult, BotWorkflowError> {
    validate_registration_input(&command.input)?;

    let request_hash = request_hash(guild_id, &command.input)?;
    let guild_id = guild_id.clone();
    let idempotency_key = idempotency_key.clone();
    let prepared = RegistrationPrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let idempotency_key = idempotency_key.clone();
            let command = command.clone();
            let request_hash = request_hash.clone();

            Box::pin(async move {
                register_guild_member_attempt(
                    transaction,
                    &guild_id,
                    &idempotency_key,
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

pub async fn preflight_guild_registration(
    pool: &PgPool,
    guild_id: &GuildId,
    idempotency_key: &DiscordInteractionId,
    input: &RegisterGuildMemberInput,
) -> Result<Option<RegisterGuildMemberResult>, BotWorkflowError> {
    validate_registration_input(input)?;
    let request_hash = request_hash(guild_id, input)?;
    let scope = TenantScope::guild(guild_id.clone());
    let outcome = idempotency::preflight(
        pool,
        &PreflightIdempotency {
            scope: &scope,
            principal_type: IdempotencyPrincipalType::Bot,
            principal_id: BOT_PRINCIPAL_ID,
            method: REGISTRATION_METHOD,
            route_template: REGISTRATION_ROUTE,
            idempotency_key: idempotency_key.as_str(),
            request_hash: &request_hash,
            inspected_at: OffsetDateTime::now_utc(),
        },
    )
    .await
    .map_err(map_preflight_error)?;

    match outcome {
        PreflightIdempotencyOutcome::Missing => {
            let actor_user_id = UserId::from(&input.actor.id);
            require_registration_eligibility(pool, guild_id, &actor_user_id).await?;
            Ok(None)
        }
        PreflightIdempotencyOutcome::Replay { response } => {
            if response.status_code != 200 {
                return Err(BotWorkflowError::InvariantViolation);
            }
            let body = response.body.ok_or(BotWorkflowError::InvariantViolation)?;
            Ok(Some(serde_json::from_value(body)?))
        }
        PreflightIdempotencyOutcome::Conflict => Err(BotWorkflowError::IdempotencyConflict),
        PreflightIdempotencyOutcome::InProgress => Err(BotWorkflowError::IdempotencyInProgress),
    }
}

async fn require_registration_eligibility(
    pool: &PgPool,
    guild_id: &GuildId,
    user_id: &UserId,
) -> Result<(), BotWorkflowError> {
    let eligibility = sqlx::query_as::<_, (GuildStatus, bool, Option<GuildMemberStatus>)>(
        "SELECT guild.status,
                config.guild_id IS NOT NULL AS configured,
                member.status
         FROM guilds AS guild
         LEFT JOIN guild_config AS config ON config.guild_id = guild.guild_id
         LEFT JOIN guild_members AS member
           ON member.guild_id = guild.guild_id AND member.user_id = $2
         WHERE guild.guild_id = $1",
    )
    .bind(guild_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(BotWorkflowError::Database)?
    .ok_or(BotWorkflowError::GuildNotFound)?;
    if eligibility.0 != GuildStatus::Active {
        return Err(BotWorkflowError::GuildInactive(eligibility.0));
    }
    if !eligibility.1 {
        return Err(BotWorkflowError::GuildNotConfigured);
    }
    if eligibility.2 == Some(GuildMemberStatus::Inactive) {
        return Err(BotWorkflowError::MemberInactive);
    }
    Ok(())
}

pub async fn execute_recruitment_command(
    pool: &PgPool,
    guild_id: &GuildId,
    idempotency_key: &DiscordInteractionId,
    request_id: &RequestId,
    command: BotRecruitmentCommand,
) -> Result<RecruitmentSnapshot, BotWorkflowError> {
    validate_recruitment_command(&command)?;

    let request_hash = request_hash(guild_id, &command)?;
    let guild_id = guild_id.clone();
    let idempotency_key = idempotency_key.clone();
    let request_id = request_id.0.clone();
    let prepared = RecruitmentPrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let command = command.clone();
            let request_hash = request_hash.clone();

            Box::pin(async move {
                execute_recruitment_attempt(
                    transaction,
                    &guild_id,
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

async fn setup_guild_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    idempotency_key: &DiscordInteractionId,
    request_id: &str,
    request_hash: &[u8],
    command: &SetupGuildCommand,
    prepared: SetupPrepared,
) -> Result<SetupGuildResult, AttemptError> {
    let scope = TenantScope::global();
    let record_id = match acquire_idempotency::<SetupGuildResult>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            method: SETUP_METHOD,
            route_template: SETUP_ROUTE,
            key: idempotency_key.as_str(),
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };

    let existing_status = lock_guild_for_setup(transaction, guild_id).await?;
    if let Some(status) = existing_status {
        require_guild_setup_available(transaction, guild_id, status).await?;
    }

    upsert_user(transaction, &command.actor, prepared.now).await?;
    if existing_status.is_some() {
        activate_guild(transaction, guild_id, command, prepared.now).await?;
    } else {
        let inserted = sqlx::query(
            "INSERT INTO guilds (
                 guild_id, discord_name, icon_hash, status, installed_by,
                 installed_at, activated_at, left_at, updated_at
             ) VALUES ($1, $2, $3, 'ACTIVE', $4, $5, $5, NULL, $5)
             ON CONFLICT (guild_id) DO NOTHING",
        )
        .bind(guild_id)
        .bind(command.guild_name.trim())
        .bind(command.guild_icon_hash.as_deref())
        .bind(&command.actor.id)
        .bind(prepared.now)
        .execute(&mut **transaction)
        .await?;

        if inserted.rows_affected() == 0 {
            let status = lock_guild_for_setup(transaction, guild_id)
                .await?
                .ok_or(AttemptError::GuildNotFound)?;
            require_guild_setup_available(transaction, guild_id, status).await?;
            activate_guild(transaction, guild_id, command, prepared.now).await?;
        }
    }

    sync_guild_actor(transaction, guild_id, &command.actor, prepared.now).await?;

    let config_revision = sqlx::query_scalar::<_, i64>(
        "INSERT INTO guild_config (
             guild_id, display_name, operator_role_id, recruitment_channel_id,
             entry_channel_id, log_channel_id, is_externally_public, revision, updated_by, updated_at
         ) VALUES ($1, $2, $3, NULL, NULL, NULL, false, 0, $4, $5)
         RETURNING revision",
    )
    .bind(guild_id)
    .bind(command.guild_name.trim())
    .bind(&command.operator_role_id)
    .bind(&command.actor.id)
    .bind(prepared.now)
    .fetch_one(&mut **transaction)
    .await?;

    let current_season = match sqlx::query_as::<_, SeasonRow>(
        "SELECT id, name
         FROM seasons
         WHERE guild_id = $1 AND status = 'ACTIVE'
         FOR UPDATE",
    )
    .bind(guild_id)
    .fetch_optional(&mut **transaction)
    .await?
    {
        Some(row) => row.into_snapshot(),
        None => {
            sqlx::query(
                "INSERT INTO seasons (
                     guild_id, id, name, status, started_at, ended_at, created_by, created_at
                 ) VALUES ($1, $2, $3, 'ACTIVE', $4, NULL, $5, $4)",
            )
            .bind(guild_id)
            .bind(prepared.season_id.as_uuid())
            .bind(command.season_name.trim())
            .bind(prepared.now)
            .bind(&command.actor.id)
            .execute(&mut **transaction)
            .await?;

            BotSeasonSnapshot {
                id: prepared.season_id,
                name: command.season_name.trim().to_owned(),
            }
        }
    };

    for counter_key in [
        "RECRUITMENT",
        "SERIES",
        "AUCTION_RECRUITMENT",
        "AUCTION_TOURNAMENT",
    ] {
        sqlx::query(
            "INSERT INTO guild_counters (guild_id, counter_key, next_value)
             VALUES ($1, $2, 1)
             ON CONFLICT (guild_id, counter_key) DO NOTHING",
        )
        .bind(guild_id)
        .bind(counter_key)
        .execute(&mut **transaction)
        .await?;
    }

    let result = SetupGuildResult {
        guild_id: guild_id.clone(),
        display_name: command.guild_name.trim().to_owned(),
        operator_role_id: command.operator_role_id.clone(),
        current_season,
        config_revision,
        dashboard_href: format!("/g/{guild_id}"),
    };

    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id: &command.actor.id,
            action: "GUILD_SETUP",
            target_type: "GUILD",
            target_id: guild_id.as_str(),
            request_id,
            before_summary: None,
            after_summary: Some(serde_json::to_value(&result)?),
            now: prepared.now,
        },
    )
    .await?;
    complete_idempotency(transaction, &scope, record_id, 200, &result, prepared.now).await?;

    Ok(result)
}

async fn lock_guild_for_setup(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
) -> Result<Option<GuildStatus>, AttemptError> {
    Ok(sqlx::query_scalar::<_, GuildStatus>(
        "SELECT status FROM guilds WHERE guild_id = $1 FOR UPDATE",
    )
    .bind(guild_id)
    .fetch_optional(&mut **transaction)
    .await?)
}

async fn require_guild_setup_available(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    status: GuildStatus,
) -> Result<(), AttemptError> {
    let has_config = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM guild_config WHERE guild_id = $1)",
    )
    .bind(guild_id)
    .fetch_one(&mut **transaction)
    .await?;

    if status == GuildStatus::Active || has_config {
        return Err(AttemptError::GuildAlreadyConfigured);
    }
    match status {
        GuildStatus::Installed | GuildStatus::SetupRequired => Ok(()),
        GuildStatus::Disabled | GuildStatus::Left => {
            Err(AttemptError::GuildSetupUnavailable(status))
        }
        GuildStatus::Active => unreachable!("ACTIVE guilds are rejected above"),
    }
}

async fn activate_guild(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    command: &SetupGuildCommand,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    sqlx::query(
        "UPDATE guilds
         SET discord_name = $2,
             icon_hash = $3,
             status = 'ACTIVE',
             installed_by = $4,
             activated_at = COALESCE(activated_at, $5),
             left_at = NULL,
             updated_at = $5
         WHERE guild_id = $1",
    )
    .bind(guild_id)
    .bind(command.guild_name.trim())
    .bind(command.guild_icon_hash.as_deref())
    .bind(&command.actor.id)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn register_guild_member_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    idempotency_key: &DiscordInteractionId,
    request_hash: &[u8],
    command: &RegisterGuildMemberCommand,
    prepared: RegistrationPrepared,
) -> Result<RegisterGuildMemberResult, AttemptError> {
    let _operator_role_id = lock_active_guild(transaction, guild_id).await?;
    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<RegisterGuildMemberResult>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            method: REGISTRATION_METHOD,
            route_template: REGISTRATION_ROUTE,
            key: idempotency_key.as_str(),
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };

    let existing_status = sqlx::query_scalar::<_, GuildMemberStatus>(
        "SELECT status
         FROM guild_members
         WHERE guild_id = $1 AND user_id = $2
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(&command.input.actor.id)
    .fetch_optional(&mut **transaction)
    .await?;
    if existing_status == Some(GuildMemberStatus::Inactive) {
        return Err(AttemptError::MemberInactive);
    }
    let registration = if existing_status.is_some() {
        RegistrationState::Existing
    } else {
        RegistrationState::Created
    };

    sync_guild_actor(transaction, guild_id, &command.input.actor, prepared.now).await?;
    let actor_user_id = UserId::from(&command.input.actor.id);
    let linked =
        riot::link_account_in_transaction(transaction, &actor_user_id, &command.account).await?;
    let account = linked
        .accounts
        .into_iter()
        .find(|account| account.puuid == command.account.puuid)
        .ok_or(AttemptError::InvariantViolation)?;
    let result = RegisterGuildMemberResult {
        member: RegisteredGuildMemberSnapshot {
            id: command.input.actor.id.clone(),
            display_name: command.input.actor.display_name.clone(),
            status: GuildMemberStatus::Active,
        },
        riot_account: registered_riot_account(account),
        registration,
        account_link: match linked.state {
            RiotAccountLinkState::Created => RegistrationState::Created,
            RiotAccountLinkState::Existing => RegistrationState::Existing,
        },
    };

    complete_idempotency(transaction, &scope, record_id, 200, &result, prepared.now).await?;
    Ok(result)
}

async fn execute_recruitment_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    idempotency_key: &DiscordInteractionId,
    request_id: &str,
    request_hash: &[u8],
    command: &BotRecruitmentCommand,
    prepared: RecruitmentPrepared,
) -> Result<RecruitmentSnapshot, AttemptError> {
    let metadata = command.metadata();
    let operator_role_id = lock_active_guild(transaction, guild_id).await?;
    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<RecruitmentSnapshot>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            method: metadata.method,
            route_template: metadata.route_template,
            key: idempotency_key.as_str(),
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };

    let actor = command.actor();
    sync_guild_actor(transaction, guild_id, actor, prepared.now).await?;
    require_active_member(transaction, guild_id, &actor.id).await?;

    let (before, snapshot) = match command {
        BotRecruitmentCommand::Create { .. } => {
            require_operator(actor, &operator_role_id)?;
            let snapshot = create_recruitment(
                transaction,
                guild_id,
                actor,
                prepared.recruitment_id,
                prepared.now,
            )
            .await?;
            (None, snapshot)
        }
        BotRecruitmentCommand::BindMessage {
            recruitment_id,
            channel_id,
            message_id,
            ..
        } => {
            require_operator(actor, &operator_role_id)?;
            let locked = lock_recruitment(transaction, guild_id, *recruitment_id).await?;
            ensure_bindable(locked.status)?;
            let before = recruitment_snapshot(transaction, guild_id, *recruitment_id).await?;
            sqlx::query(
                "UPDATE recruitments
                 SET channel_id = $1,
                     message_id = $2,
                     revision = revision + 1,
                     updated_by = $3,
                     updated_at = $4
                 WHERE guild_id = $5 AND id = $6",
            )
            .bind(channel_id)
            .bind(message_id)
            .bind(&actor.id)
            .bind(prepared.now)
            .bind(guild_id)
            .bind(recruitment_id.as_uuid())
            .execute(&mut **transaction)
            .await?;
            let snapshot = recruitment_snapshot(transaction, guild_id, *recruitment_id).await?;
            (Some(before), snapshot)
        }
        BotRecruitmentCommand::Join {
            recruitment_id,
            participant,
            ..
        } => {
            let participant_user_id = UserId::from(&participant.id);
            require_self_or_operator(actor, &participant_user_id, &operator_role_id)?;
            let locked = lock_recruitment(transaction, guild_id, *recruitment_id).await?;
            ensure_open(locked.status)?;
            lock_entry_draft(transaction, guild_id, *recruitment_id).await?;
            if actor.id != participant.id {
                sync_guild_actor(transaction, guild_id, participant, prepared.now).await?;
                require_active_member(transaction, guild_id, &participant.id).await?;
            }
            let before = recruitment_snapshot(transaction, guild_id, *recruitment_id).await?;
            join_recruitment(
                transaction,
                guild_id,
                *recruitment_id,
                participant,
                locked.target_count,
                prepared.now,
            )
            .await?;
            bump_recruitment_and_draft(
                transaction,
                guild_id,
                *recruitment_id,
                &actor.id,
                prepared.now,
            )
            .await?;
            let snapshot = recruitment_snapshot(transaction, guild_id, *recruitment_id).await?;
            (Some(before), snapshot)
        }
        BotRecruitmentCommand::Leave {
            recruitment_id,
            user_id,
            ..
        } => {
            require_self_or_operator(actor, user_id, &operator_role_id)?;
            let locked = lock_recruitment(transaction, guild_id, *recruitment_id).await?;
            ensure_open(locked.status)?;
            lock_entry_draft(transaction, guild_id, *recruitment_id).await?;
            let before = recruitment_snapshot(transaction, guild_id, *recruitment_id).await?;
            leave_recruitment(
                transaction,
                guild_id,
                *recruitment_id,
                user_id,
                prepared.now,
            )
            .await?;
            bump_recruitment_and_draft(
                transaction,
                guild_id,
                *recruitment_id,
                &actor.id,
                prepared.now,
            )
            .await?;
            let snapshot = recruitment_snapshot(transaction, guild_id, *recruitment_id).await?;
            (Some(before), snapshot)
        }
        BotRecruitmentCommand::SetPreferredLanes {
            recruitment_id,
            user_id,
            lanes,
            ..
        } => {
            require_self_or_operator(actor, user_id, &operator_role_id)?;
            let locked = lock_recruitment(transaction, guild_id, *recruitment_id).await?;
            ensure_open(locked.status)?;
            lock_entry_draft(transaction, guild_id, *recruitment_id).await?;
            let before = recruitment_snapshot(transaction, guild_id, *recruitment_id).await?;
            set_preferred_lanes(transaction, guild_id, *recruitment_id, user_id, lanes).await?;
            bump_recruitment_and_draft(
                transaction,
                guild_id,
                *recruitment_id,
                &actor.id,
                prepared.now,
            )
            .await?;
            let snapshot = recruitment_snapshot(transaction, guild_id, *recruitment_id).await?;
            (Some(before), snapshot)
        }
        BotRecruitmentCommand::Close { recruitment_id, .. } => {
            require_operator(actor, &operator_role_id)?;
            let locked = lock_recruitment(transaction, guild_id, *recruitment_id).await?;
            ensure_open(locked.status)?;
            let before = recruitment_snapshot(transaction, guild_id, *recruitment_id).await?;
            let joined_count =
                joined_participant_count(transaction, guild_id, *recruitment_id).await?;
            if joined_count != i64::from(locked.target_count) {
                return Err(AttemptError::CloseRequiresFullRoster {
                    joined: joined_count,
                    target: locked.target_count,
                });
            }
            sqlx::query(
                "UPDATE recruitments
                 SET status = 'CLOSED',
                     revision = revision + 1,
                     updated_by = $1,
                     updated_at = $2,
                     closed_at = $2
                 WHERE guild_id = $3 AND id = $4",
            )
            .bind(&actor.id)
            .bind(prepared.now)
            .bind(guild_id)
            .bind(recruitment_id.as_uuid())
            .execute(&mut **transaction)
            .await?;
            let snapshot = recruitment_snapshot(transaction, guild_id, *recruitment_id).await?;
            (Some(before), snapshot)
        }
    };

    let recruitment_id = snapshot.recruitment.id.to_string();
    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id: &actor.id,
            action: metadata.audit_action,
            target_type: "RECRUITMENT",
            target_id: &recruitment_id,
            request_id,
            before_summary: before.as_ref().map(serde_json::to_value).transpose()?,
            after_summary: Some(serde_json::to_value(&snapshot)?),
            now: prepared.now,
        },
    )
    .await?;
    insert_outbox(
        transaction,
        guild_id,
        prepared.outbox_event_id,
        format!("guild:{guild_id}:recruitment:{}", snapshot.recruitment.id),
        "RECRUITMENT",
        &recruitment_id,
        snapshot.recruitment.revision,
        metadata.event_type,
        json!({
            "type": "invalidate",
            "resourceId": snapshot.recruitment.id,
            "revision": snapshot.recruitment.revision,
        }),
        prepared.now,
    )
    .await?;
    complete_idempotency(
        transaction,
        &scope,
        record_id,
        metadata.success_status,
        &snapshot,
        prepared.now,
    )
    .await?;

    Ok(snapshot)
}

async fn create_recruitment(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor: &BotActor,
    recruitment_id: RecruitmentId,
    now: OffsetDateTime,
) -> Result<RecruitmentSnapshot, AttemptError> {
    let season_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT id
         FROM seasons
         WHERE guild_id = $1 AND status = 'ACTIVE'",
    )
    .bind(guild_id)
    .fetch_optional(&mut **transaction)
    .await?
    .map(SeasonId::from_uuid)
    .ok_or(AttemptError::NoActiveSeason)?;

    let display_number = sqlx::query_scalar::<_, i64>(
        "UPDATE guild_counters
         SET next_value = next_value + 1
         WHERE guild_id = $1 AND counter_key = 'RECRUITMENT'
         RETURNING next_value - 1",
    )
    .bind(guild_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::MissingRecruitmentCounter)?;
    let title = format!("일반 내전 모집 #{display_number}");

    sqlx::query(
        "INSERT INTO recruitments (
             guild_id, id, season_id, display_number, title, target_count, status,
             revision, channel_id, message_id, created_by, created_at, updated_by, updated_at
         ) VALUES ($1, $2, $3, $4, $5, $6, 'OPEN', 0, NULL, NULL, $7, $8, $7, $8)",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .bind(season_id.as_uuid())
    .bind(display_number)
    .bind(title)
    .bind(RECRUITMENT_TARGET_COUNT)
    .bind(&actor.id)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO recruitment_entry_drafts (
             guild_id, recruitment_id, revision, updated_by, updated_at
         ) VALUES ($1, $2, 0, $3, $4)",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .bind(&actor.id)
    .bind(now)
    .execute(&mut **transaction)
    .await?;

    recruitment_snapshot(transaction, guild_id, recruitment_id).await
}

async fn join_recruitment(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    participant: &BotActor,
    target_count: i16,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let existing_status = sqlx::query_scalar::<_, String>(
        "SELECT status
         FROM recruitment_participants
         WHERE guild_id = $1 AND recruitment_id = $2 AND user_id = $3
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .bind(&participant.id)
    .fetch_optional(&mut **transaction)
    .await?;
    if existing_status.as_deref() == Some("JOINED") {
        return Err(AttemptError::ParticipantAlreadyJoined);
    }

    let joined_count = joined_participant_count(transaction, guild_id, recruitment_id).await?;
    if joined_count >= i64::from(target_count) {
        return Err(AttemptError::RecruitmentFull);
    }

    sqlx::query(
        "INSERT INTO recruitment_participants (
             guild_id, recruitment_id, user_id, status, joined_at, withdrawn_at
         ) VALUES ($1, $2, $3, 'JOINED', $4, NULL)
         ON CONFLICT (guild_id, recruitment_id, user_id) DO UPDATE
         SET status = 'JOINED', joined_at = EXCLUDED.joined_at, withdrawn_at = NULL",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .bind(&participant.id)
    .bind(now)
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

async fn leave_recruitment(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    user_id: &UserId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let updated = sqlx::query(
        "UPDATE recruitment_participants
         SET status = 'WITHDRAWN', withdrawn_at = $1
         WHERE guild_id = $2
           AND recruitment_id = $3
           AND user_id = $4
           AND status = 'JOINED'",
    )
    .bind(now)
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .bind(user_id)
    .execute(&mut **transaction)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(AttemptError::ParticipantNotJoined);
    }

    sqlx::query(
        "DELETE FROM recruitment_entry_assignments
         WHERE guild_id = $1 AND recruitment_id = $2 AND user_id = $3",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .bind(user_id)
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

async fn set_preferred_lanes(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    user_id: &UserId,
    lanes: &[Lane],
) -> Result<(), AttemptError> {
    let participant_status = sqlx::query_scalar::<_, String>(
        "SELECT status
         FROM recruitment_participants
         WHERE guild_id = $1 AND recruitment_id = $2 AND user_id = $3
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .bind(user_id)
    .fetch_optional(&mut **transaction)
    .await?;
    if participant_status.as_deref() != Some("JOINED") {
        return Err(AttemptError::ParticipantNotJoined);
    }

    sqlx::query(
        "DELETE FROM recruitment_preferred_lanes
         WHERE guild_id = $1 AND recruitment_id = $2 AND user_id = $3",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .bind(user_id)
    .execute(&mut **transaction)
    .await?;

    for (index, lane) in lanes.iter().enumerate() {
        let priority = i16::try_from(index + 1).map_err(|_| AttemptError::InvalidPreferredLanes)?;
        sqlx::query(
            "INSERT INTO recruitment_preferred_lanes (
                 guild_id, recruitment_id, user_id, priority, lane
             ) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(guild_id)
        .bind(recruitment_id.as_uuid())
        .bind(user_id)
        .bind(priority)
        .bind(*lane)
        .execute(&mut **transaction)
        .await?;
    }

    Ok(())
}

async fn recruitment_snapshot(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
) -> Result<RecruitmentSnapshot, AttemptError> {
    let recruitment = sqlx::query_as::<_, RecruitmentProjectionRow>(
        "SELECT id, display_number, title, status, target_count, revision,
                channel_id, message_id, updated_at
         FROM recruitments
         WHERE guild_id = $1 AND id = $2",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::RecruitmentNotFound)?
    .into_snapshot();

    let participant_rows = sqlx::query_as::<_, RecruitmentParticipantRow>(
        "SELECT participant.user_id AS id, member.display_name
         FROM recruitment_participants AS participant
         JOIN guild_members AS member
           ON member.guild_id = participant.guild_id
          AND member.user_id = participant.user_id
         WHERE participant.guild_id = $1
           AND participant.recruitment_id = $2
           AND participant.status = 'JOINED'
         ORDER BY participant.joined_at, participant.user_id",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    let lane_rows = sqlx::query_as::<_, RecruitmentLaneRow>(
        "SELECT preferred.user_id, preferred.lane
         FROM recruitment_preferred_lanes AS preferred
         JOIN recruitment_participants AS participant
           ON participant.guild_id = preferred.guild_id
          AND participant.recruitment_id = preferred.recruitment_id
          AND participant.user_id = preferred.user_id
          AND participant.status = 'JOINED'
         WHERE preferred.guild_id = $1 AND preferred.recruitment_id = $2
         ORDER BY preferred.user_id, preferred.priority",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    let mut lanes_by_user: HashMap<UserId, Vec<Lane>> = HashMap::new();
    for row in lane_rows {
        lanes_by_user.entry(row.user_id).or_default().push(row.lane);
    }
    let participants = participant_rows
        .into_iter()
        .map(|row| RecruitmentParticipantSnapshot {
            preferred_lanes: lanes_by_user.remove(&row.id).unwrap_or_default(),
            id: row.id,
            display_name: row.display_name,
        })
        .collect();

    Ok(RecruitmentSnapshot {
        recruitment,
        participants,
        links: RecruitmentLinksSnapshot {
            entry: format!("/g/{guild_id}/recruitments/{recruitment_id}/entry"),
        },
    })
}

fn registered_riot_account(account: RiotAccountRecord) -> RegisteredRiotAccountSnapshot {
    RegisteredRiotAccountSnapshot {
        puuid: account.puuid,
        game_name: account.game_name,
        tag_line: account.tag_line,
        platform: account.platform,
        is_main: account.is_main,
    }
}

async fn upsert_user(
    transaction: &mut DatabaseTransaction,
    actor: &BotActor,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let user_id = UserId::from(&actor.id);
    let persisted_user_id = sqlx::query_scalar::<_, String>(
        "INSERT INTO users (
             user_id, identity_kind, display_name, avatar_hash,
             created_at, updated_at, deleted_at
         ) VALUES ($1, 'DISCORD', $2, $3, $4, $4, NULL)
         ON CONFLICT (user_id) DO UPDATE
         SET display_name = EXCLUDED.display_name,
             avatar_hash = EXCLUDED.avatar_hash,
             updated_at = EXCLUDED.updated_at,
             deleted_at = NULL
         WHERE users.identity_kind = 'DISCORD'
         RETURNING user_id",
    )
    .bind(&user_id)
    .bind(actor.global_display_name.trim())
    .bind(actor.avatar_hash.as_deref())
    .bind(now)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::IdentityConflict)?;

    let identity_user_id = sqlx::query_scalar::<_, String>(
        "INSERT INTO user_discord_identities (
             user_id, discord_id, created_at, updated_at
         ) VALUES ($1, $2, $3, $3)
         ON CONFLICT (discord_id) DO UPDATE
         SET updated_at = EXCLUDED.updated_at
         WHERE user_discord_identities.user_id = EXCLUDED.user_id
         RETURNING user_id",
    )
    .bind(&user_id)
    .bind(&actor.id)
    .bind(now)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::IdentityConflict)?;
    if identity_user_id != persisted_user_id {
        return Err(AttemptError::IdentityConflict);
    }

    Ok(())
}

async fn sync_guild_actor(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor: &BotActor,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    upsert_user(transaction, actor, now).await?;
    sqlx::query(
        "INSERT INTO guild_members (
             guild_id, user_id, display_name, status, main_lane, first_seen_at,
             registered_at, discord_joined_at, last_active_at, last_synced_at, inactivated_at
         ) VALUES ($1, $2, $3, 'ACTIVE', NULL, $4, $4, NULL, $4, $4, NULL)
         ON CONFLICT (guild_id, user_id) DO UPDATE
         SET display_name = EXCLUDED.display_name,
             registered_at = COALESCE(guild_members.registered_at, EXCLUDED.registered_at),
             last_active_at = EXCLUDED.last_active_at,
             last_synced_at = EXCLUDED.last_synced_at",
    )
    .bind(guild_id)
    .bind(&actor.id)
    .bind(actor.display_name.trim())
    .bind(now)
    .execute(&mut **transaction)
    .await?;

    sqlx::query("DELETE FROM guild_member_discord_roles WHERE guild_id = $1 AND user_id = $2")
        .bind(guild_id)
        .bind(&actor.id)
        .execute(&mut **transaction)
        .await?;

    let mut inserted_roles = HashSet::new();
    for role_id in &actor.role_ids {
        if !inserted_roles.insert(role_id.as_str()) {
            continue;
        }
        sqlx::query(
            "INSERT INTO guild_member_discord_roles (
                 guild_id, user_id, role_id, last_synced_at
             ) VALUES ($1, $2, $3, $4)",
        )
        .bind(guild_id)
        .bind(&actor.id)
        .bind(role_id)
        .bind(now)
        .execute(&mut **transaction)
        .await?;
    }

    Ok(())
}

async fn require_active_member(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    user_id: &DiscordUserId,
) -> Result<(), AttemptError> {
    let status = sqlx::query_scalar::<_, GuildMemberStatus>(
        "SELECT status
         FROM guild_members
         WHERE guild_id = $1 AND user_id = $2
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(user_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::MemberInactive)?;
    if status == GuildMemberStatus::Active {
        Ok(())
    } else {
        Err(AttemptError::MemberInactive)
    }
}

async fn lock_active_guild(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
) -> Result<DiscordRoleId, AttemptError> {
    let status = sqlx::query_scalar::<_, GuildStatus>(
        "SELECT status
         FROM guilds
         WHERE guild_id = $1
         FOR UPDATE",
    )
    .bind(guild_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::GuildNotFound)?;
    if status != GuildStatus::Active {
        return Err(AttemptError::GuildInactive(status));
    }
    sqlx::query_scalar::<_, DiscordRoleId>(
        "SELECT operator_role_id
         FROM guild_config
         WHERE guild_id = $1
         FOR UPDATE",
    )
    .bind(guild_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::GuildNotConfigured)
}

async fn lock_recruitment(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
) -> Result<RecruitmentLockRow, AttemptError> {
    sqlx::query_as::<_, RecruitmentLockRow>(
        "SELECT status, target_count
         FROM recruitments
         WHERE guild_id = $1 AND id = $2
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::RecruitmentNotFound)
}

async fn lock_entry_draft(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
) -> Result<(), AttemptError> {
    let exists = sqlx::query_scalar::<_, i64>(
        "SELECT revision
         FROM recruitment_entry_drafts
         WHERE guild_id = $1 AND recruitment_id = $2
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?;
    if exists.is_none() {
        return Err(AttemptError::MissingEntryDraft);
    }
    Ok(())
}

async fn bump_recruitment_and_draft(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    actor_id: &DiscordUserId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let recruitment = sqlx::query(
        "UPDATE recruitments
         SET revision = revision + 1, updated_by = $1, updated_at = $2
         WHERE guild_id = $3 AND id = $4",
    )
    .bind(actor_id)
    .bind(now)
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .execute(&mut **transaction)
    .await?;
    let draft = sqlx::query(
        "UPDATE recruitment_entry_drafts
         SET revision = revision + 1, updated_by = $1, updated_at = $2
         WHERE guild_id = $3 AND recruitment_id = $4",
    )
    .bind(actor_id)
    .bind(now)
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .execute(&mut **transaction)
    .await?;
    if recruitment.rows_affected() != 1 || draft.rows_affected() != 1 {
        return Err(AttemptError::MissingEntryDraft);
    }
    Ok(())
}

async fn joined_participant_count(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
) -> Result<i64, AttemptError> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT count(*)
         FROM recruitment_participants
         WHERE guild_id = $1 AND recruitment_id = $2 AND status = 'JOINED'",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_one(&mut **transaction)
    .await?)
}

async fn insert_audit(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    record: AuditInsert<'_>,
) -> Result<(), AttemptError> {
    sqlx::query(
        "INSERT INTO admin_audit_log (
             guild_id, id, principal_type, principal_id, action, target_type, target_id,
             checkpoint_id, request_id, before_summary, after_summary, result, note, created_at
         ) VALUES ($1, $2, 'USER', $3, $4, $5, $6, NULL, $7, $8, $9, 'APPLIED', NULL, $10)",
    )
    .bind(guild_id)
    .bind(record.id)
    .bind(record.actor_id)
    .bind(record.action)
    .bind(record.target_type)
    .bind(record.target_id)
    .bind(record.request_id)
    .bind(record.before_summary)
    .bind(record.after_summary)
    .bind(record.now)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_outbox(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    event_id: OutboxEventId,
    topic: String,
    aggregate_type: &str,
    aggregate_id: &str,
    aggregate_revision: i64,
    event_type: &str,
    payload: Value,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    sqlx::query(
        "INSERT INTO outbox_events (
             id, guild_id, topic, aggregate_type, aggregate_id, aggregate_revision,
             event_type, payload, occurred_at, available_at
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $9)",
    )
    .bind(event_id.as_uuid())
    .bind(guild_id)
    .bind(topic)
    .bind(aggregate_type)
    .bind(aggregate_id)
    .bind(aggregate_revision)
    .bind(event_type)
    .bind(payload)
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
        principal_type: IdempotencyPrincipalType::Bot,
        principal_id: BOT_PRINCIPAL_ID,
        method: request.method,
        route_template: request.route_template,
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

async fn complete_idempotency<T: Serialize>(
    transaction: &mut DatabaseTransaction,
    scope: &TenantScope,
    record_id: IdempotencyRecordId,
    status_code: u16,
    result: &T,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let outcome = idempotency::complete(
        transaction,
        CompleteIdempotency {
            scope,
            record_id,
            response: IdempotentResponse {
                status_code,
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

fn validate_setup_command(command: &SetupGuildCommand) -> Result<(), BotWorkflowError> {
    validate_actor(&command.actor)?;
    if command.guild_name.trim().is_empty() {
        return Err(BotWorkflowError::InvalidCommand("guild name is empty"));
    }
    if command.season_name.trim().is_empty() {
        return Err(BotWorkflowError::InvalidCommand("season name is empty"));
    }
    Ok(())
}

fn validate_registration_input(input: &RegisterGuildMemberInput) -> Result<(), BotWorkflowError> {
    validate_actor(&input.actor)?;
    if input.requested_game_name.trim().is_empty()
        || input.requested_tag_line.trim().is_empty()
        || input.requested_platform.trim().is_empty()
    {
        return Err(BotWorkflowError::InvalidCommand(
            "Riot account request is empty",
        ));
    }
    Ok(())
}

fn validate_recruitment_command(command: &BotRecruitmentCommand) -> Result<(), BotWorkflowError> {
    validate_actor(command.actor())?;
    if let BotRecruitmentCommand::Join { participant, .. } = command {
        validate_actor(participant)?;
    }
    if let BotRecruitmentCommand::SetPreferredLanes { lanes, .. } = command {
        validate_lanes(lanes)?;
    }
    Ok(())
}

fn validate_actor(actor: &BotActor) -> Result<(), BotWorkflowError> {
    if actor.global_display_name.trim().is_empty() || actor.display_name.trim().is_empty() {
        return Err(BotWorkflowError::InvalidCommand(
            "actor display name is empty",
        ));
    }
    Ok(())
}

fn validate_lanes(lanes: &[Lane]) -> Result<(), BotWorkflowError> {
    if lanes.is_empty() || lanes.len() > MAX_PREFERRED_LANES {
        return Err(BotWorkflowError::InvalidPreferredLanes);
    }
    let unique: HashSet<Lane> = lanes.iter().copied().collect();
    if unique.len() != lanes.len() {
        return Err(BotWorkflowError::InvalidPreferredLanes);
    }
    Ok(())
}

fn require_operator(
    actor: &BotActor,
    operator_role_id: &DiscordRoleId,
) -> Result<(), AttemptError> {
    if actor.role_ids.iter().any(|role| role == operator_role_id) {
        Ok(())
    } else {
        Err(AttemptError::OperatorRequired)
    }
}

fn require_self_or_operator(
    actor: &BotActor,
    target_user_id: &UserId,
    operator_role_id: &DiscordRoleId,
) -> Result<(), AttemptError> {
    if UserId::from(&actor.id) == *target_user_id {
        Ok(())
    } else {
        require_operator(actor, operator_role_id)
    }
}

fn ensure_open(status: RecruitmentStatus) -> Result<(), AttemptError> {
    if status == RecruitmentStatus::Open {
        Ok(())
    } else {
        Err(AttemptError::RecruitmentNotOpen(status))
    }
}

fn ensure_bindable(status: RecruitmentStatus) -> Result<(), AttemptError> {
    match status {
        RecruitmentStatus::Open | RecruitmentStatus::Closed => Ok(()),
        RecruitmentStatus::Converted | RecruitmentStatus::Cancelled => {
            Err(AttemptError::RecruitmentNotBindable(status))
        }
    }
}

fn request_hash<T: Serialize>(
    guild_id: &GuildId,
    command: &T,
) -> Result<Vec<u8>, BotWorkflowError> {
    let canonical = serde_json::to_string(&(guild_id, command))?;
    Ok(AuthService::hash_secret(&canonical))
}

fn attempt_sql_error(error: &AttemptError) -> Option<&sqlx::Error> {
    match error {
        AttemptError::Sql(error) => Some(error),
        AttemptError::Idempotency(IdempotencyRepositoryError::Sql(error)) => Some(error),
        AttemptError::Riot(RiotRepositoryError::Sql(error)) => Some(error),
        _ => None,
    }
}

fn map_preflight_error(error: IdempotencyRepositoryError) -> BotWorkflowError {
    match error {
        IdempotencyRepositoryError::Sql(error) => BotWorkflowError::Database(error),
        _ => BotWorkflowError::InvariantViolation,
    }
}

fn map_transaction_error(error: TransactionRetryError<AttemptError>) -> BotWorkflowError {
    match error {
        TransactionRetryError::DeadlockRetriesExhausted { .. } => BotWorkflowError::DatabaseBusy,
        TransactionRetryError::NonRetryable { failure, .. } => match failure {
            TransactionAttemptError::Begin(error) | TransactionAttemptError::Commit(error) => {
                BotWorkflowError::Database(error)
            }
            TransactionAttemptError::Operation(error) => error.into_public(),
        },
    }
}

struct IdempotencyAcquire<'a> {
    scope: &'a TenantScope,
    record_id: IdempotencyRecordId,
    method: &'a str,
    route_template: &'a str,
    key: &'a str,
    request_hash: &'a [u8],
    now: OffsetDateTime,
}

struct AuditInsert<'a> {
    id: Uuid,
    actor_id: &'a DiscordUserId,
    action: &'a str,
    target_type: &'a str,
    target_id: &'a str,
    request_id: &'a str,
    before_summary: Option<Value>,
    after_summary: Option<Value>,
    now: OffsetDateTime,
}

#[derive(Clone, Copy)]
struct SetupPrepared {
    now: OffsetDateTime,
    idempotency_record_id: IdempotencyRecordId,
    season_id: SeasonId,
    audit_id: Uuid,
}

#[derive(Clone, Copy)]
struct RegistrationPrepared {
    now: OffsetDateTime,
    idempotency_record_id: IdempotencyRecordId,
}

impl RegistrationPrepared {
    fn new() -> Self {
        Self {
            now: OffsetDateTime::now_utc(),
            idempotency_record_id: IdempotencyRecordId::new(),
        }
    }
}

impl SetupPrepared {
    fn new() -> Self {
        Self {
            now: OffsetDateTime::now_utc(),
            idempotency_record_id: IdempotencyRecordId::new(),
            season_id: SeasonId::new(),
            audit_id: Uuid::now_v7(),
        }
    }
}

#[derive(Clone, Copy)]
struct RecruitmentPrepared {
    now: OffsetDateTime,
    idempotency_record_id: IdempotencyRecordId,
    recruitment_id: RecruitmentId,
    audit_id: Uuid,
    outbox_event_id: OutboxEventId,
}

impl RecruitmentPrepared {
    fn new() -> Self {
        Self {
            now: OffsetDateTime::now_utc(),
            idempotency_record_id: IdempotencyRecordId::new(),
            recruitment_id: RecruitmentId::new(),
            audit_id: Uuid::now_v7(),
            outbox_event_id: OutboxEventId::new(),
        }
    }
}

#[derive(Clone, Copy)]
struct CommandMetadata {
    method: &'static str,
    route_template: &'static str,
    success_status: u16,
    audit_action: &'static str,
    event_type: &'static str,
}

impl BotRecruitmentCommand {
    fn actor(&self) -> &BotActor {
        match self {
            Self::Create { actor }
            | Self::BindMessage { actor, .. }
            | Self::Join { actor, .. }
            | Self::Leave { actor, .. }
            | Self::SetPreferredLanes { actor, .. }
            | Self::Close { actor, .. } => actor,
        }
    }

    fn metadata(&self) -> CommandMetadata {
        match self {
            Self::Create { .. } => CommandMetadata {
                method: "POST",
                route_template: "/api/guilds/{guildId}/recruitments",
                success_status: 201,
                audit_action: "RECRUITMENT_CREATE",
                event_type: "RECRUITMENT_CREATED",
            },
            Self::BindMessage { .. } => CommandMetadata {
                method: "PUT",
                route_template: "/api/guilds/{guildId}/recruitments/{id}/message",
                success_status: 200,
                audit_action: "RECRUITMENT_BIND_MESSAGE",
                event_type: "RECRUITMENT_MESSAGE_BOUND",
            },
            Self::Join { .. } => CommandMetadata {
                method: "POST",
                route_template: "/api/guilds/{guildId}/recruitments/{id}/members",
                success_status: 200,
                audit_action: "RECRUITMENT_JOIN",
                event_type: "RECRUITMENT_PARTICIPANT_JOINED",
            },
            Self::Leave { .. } => CommandMetadata {
                method: "DELETE",
                route_template: "/api/guilds/{guildId}/recruitments/{id}/members/{userId}",
                success_status: 200,
                audit_action: "RECRUITMENT_LEAVE",
                event_type: "RECRUITMENT_PARTICIPANT_LEFT",
            },
            Self::SetPreferredLanes { .. } => CommandMetadata {
                method: "PUT",
                route_template: "/api/guilds/{guildId}/recruitments/{id}/members/{userId}/preferred-lanes",
                success_status: 200,
                audit_action: "RECRUITMENT_SET_PREFERRED_LANES",
                event_type: "RECRUITMENT_PREFERRED_LANES_SET",
            },
            Self::Close { .. } => CommandMetadata {
                method: "POST",
                route_template: "/api/guilds/{guildId}/recruitments/{id}/close",
                success_status: 200,
                audit_action: "RECRUITMENT_CLOSE",
                event_type: "RECRUITMENT_CLOSED",
            },
        }
    }
}

enum IdempotencyDecision<T> {
    Acquired(IdempotencyRecordId),
    Replay(T),
}

#[derive(Debug, FromRow)]
struct SeasonRow {
    id: Uuid,
    name: String,
}

impl SeasonRow {
    fn into_snapshot(self) -> BotSeasonSnapshot {
        BotSeasonSnapshot {
            id: SeasonId::from_uuid(self.id),
            name: self.name,
        }
    }
}

#[derive(Debug, FromRow)]
struct RecruitmentLockRow {
    status: RecruitmentStatus,
    target_count: i16,
}

#[derive(Debug, FromRow)]
struct RecruitmentProjectionRow {
    id: Uuid,
    display_number: i64,
    title: String,
    status: RecruitmentStatus,
    target_count: i16,
    revision: i64,
    channel_id: Option<DiscordChannelId>,
    message_id: Option<DiscordMessageId>,
    updated_at: OffsetDateTime,
}

impl RecruitmentProjectionRow {
    fn into_snapshot(self) -> RecruitmentSummarySnapshot {
        RecruitmentSummarySnapshot {
            id: RecruitmentId::from_uuid(self.id),
            display_number: self.display_number,
            title: self.title,
            status: self.status,
            target_count: self.target_count,
            revision: self.revision,
            channel_id: self.channel_id,
            message_id: self.message_id,
            updated_at: self.updated_at,
        }
    }
}

#[derive(Debug, FromRow)]
struct RecruitmentParticipantRow {
    id: UserId,
    display_name: String,
}

#[derive(Debug, FromRow)]
struct RecruitmentLaneRow {
    user_id: UserId,
    lane: Lane,
}

#[derive(Debug, Error)]
enum AttemptError {
    #[error("PostgreSQL bot workflow operation failed")]
    Sql(#[from] sqlx::Error),
    #[error("idempotency repository operation failed")]
    Idempotency(#[from] IdempotencyRepositoryError),
    #[error("Riot account repository operation failed")]
    Riot(#[from] RiotRepositoryError),
    #[error("Discord identity conflicts with a non-Discord user")]
    IdentityConflict,
    #[error("bot workflow JSON serialization failed")]
    Serialization(#[from] serde_json::Error),
    #[error("guild was not found")]
    GuildNotFound,
    #[error("guild is not active: {0}")]
    GuildInactive(GuildStatus),
    #[error("guild setup has not created a configuration")]
    GuildNotConfigured,
    #[error("guild setup was already completed")]
    GuildAlreadyConfigured,
    #[error("guild cannot be configured while its status is {0}")]
    GuildSetupUnavailable(GuildStatus),
    #[error("the actor does not hold the configured operator role")]
    OperatorRequired,
    #[error("the guild member is inactive")]
    MemberInactive,
    #[error("the guild has no active season")]
    NoActiveSeason,
    #[error("the guild recruitment counter is missing")]
    MissingRecruitmentCounter,
    #[error("recruitment was not found in the tenant")]
    RecruitmentNotFound,
    #[error("recruitment must be OPEN, but is {0}")]
    RecruitmentNotOpen(RecruitmentStatus),
    #[error("recruitment message cannot be bound while status is {0}")]
    RecruitmentNotBindable(RecruitmentStatus),
    #[error("recruitment is full")]
    RecruitmentFull,
    #[error("participant is already joined")]
    ParticipantAlreadyJoined,
    #[error("participant is not joined")]
    ParticipantNotJoined,
    #[error("preferred lanes are invalid")]
    InvalidPreferredLanes,
    #[error("entry draft root is missing")]
    MissingEntryDraft,
    #[error("closing recruitment requires {target} joined participants, found {joined}")]
    CloseRequiresFullRoster { joined: i64, target: i16 },
    #[error("idempotency key was reused with a different request")]
    IdempotencyConflict,
    #[error("the same idempotent command is still in progress")]
    IdempotencyInProgress,
    #[error("stored idempotent response is corrupt")]
    CorruptIdempotentResponse,
    #[error("idempotency completion row was not in progress")]
    IdempotencyCompletionLost,
    #[error("bot workflow storage invariant was violated")]
    InvariantViolation,
}

impl AttemptError {
    fn into_public(self) -> BotWorkflowError {
        match self {
            Self::Sql(error) => BotWorkflowError::Database(error),
            Self::Idempotency(IdempotencyRepositoryError::Sql(error)) => {
                BotWorkflowError::Database(error)
            }
            Self::Idempotency(_) => BotWorkflowError::InvariantViolation,
            Self::Riot(RiotRepositoryError::Sql(error)) => BotWorkflowError::Database(error),
            Self::Riot(RiotRepositoryError::AlreadyLinked) => {
                BotWorkflowError::RiotAccountAlreadyLinked
            }
            Self::Riot(RiotRepositoryError::IdentityConflict) => {
                BotWorkflowError::RiotIdentityConflict
            }
            Self::Riot(
                RiotRepositoryError::UserNotFound
                | RiotRepositoryError::NotFound
                | RiotRepositoryError::InvalidCanonicalAccount
                | RiotRepositoryError::InvariantViolation,
            ) => BotWorkflowError::InvariantViolation,
            Self::Serialization(error) => BotWorkflowError::Serialization(error),
            Self::GuildNotFound => BotWorkflowError::GuildNotFound,
            Self::GuildInactive(status) => BotWorkflowError::GuildInactive(status),
            Self::GuildNotConfigured => BotWorkflowError::GuildNotConfigured,
            Self::GuildAlreadyConfigured => BotWorkflowError::GuildAlreadyConfigured,
            Self::GuildSetupUnavailable(status) => BotWorkflowError::GuildSetupUnavailable(status),
            Self::OperatorRequired => BotWorkflowError::OperatorRequired,
            Self::MemberInactive => BotWorkflowError::MemberInactive,
            Self::NoActiveSeason => BotWorkflowError::NoActiveSeason,
            Self::MissingRecruitmentCounter => BotWorkflowError::InvariantViolation,
            Self::RecruitmentNotFound => BotWorkflowError::RecruitmentNotFound,
            Self::RecruitmentNotOpen(status) => BotWorkflowError::RecruitmentNotOpen(status),
            Self::RecruitmentNotBindable(status) => {
                BotWorkflowError::RecruitmentNotBindable(status)
            }
            Self::RecruitmentFull => BotWorkflowError::RecruitmentFull,
            Self::ParticipantAlreadyJoined => BotWorkflowError::ParticipantAlreadyJoined,
            Self::ParticipantNotJoined => BotWorkflowError::ParticipantNotJoined,
            Self::InvalidPreferredLanes => BotWorkflowError::InvalidPreferredLanes,
            Self::MissingEntryDraft => BotWorkflowError::InvariantViolation,
            Self::CloseRequiresFullRoster { joined, target } => {
                BotWorkflowError::CloseRequiresFullRoster { joined, target }
            }
            Self::IdempotencyConflict => BotWorkflowError::IdempotencyConflict,
            Self::IdempotencyInProgress => BotWorkflowError::IdempotencyInProgress,
            Self::IdentityConflict => BotWorkflowError::UserIdentityConflict,
            Self::CorruptIdempotentResponse
            | Self::IdempotencyCompletionLost
            | Self::InvariantViolation => BotWorkflowError::InvariantViolation,
        }
    }
}

#[derive(Debug, Error)]
pub enum BotWorkflowError {
    #[error("bot workflow command is invalid: {0}")]
    InvalidCommand(&'static str),
    #[error("guild was not found")]
    GuildNotFound,
    #[error("guild is not active: {0}")]
    GuildInactive(GuildStatus),
    #[error("guild setup is incomplete")]
    GuildNotConfigured,
    #[error("guild setup was already completed")]
    GuildAlreadyConfigured,
    #[error("guild cannot be configured while its status is {0}")]
    GuildSetupUnavailable(GuildStatus),
    #[error("the configured operator role is required")]
    OperatorRequired,
    #[error("the guild member is inactive")]
    MemberInactive,
    #[error("the Riot account is already linked to another user")]
    RiotAccountAlreadyLinked,
    #[error("the canonical Riot ID conflicts with another account")]
    RiotIdentityConflict,
    #[error("the Discord identity conflicts with a non-Discord user")]
    UserIdentityConflict,
    #[error("the guild has no active season")]
    NoActiveSeason,
    #[error("recruitment was not found")]
    RecruitmentNotFound,
    #[error("recruitment must be OPEN, but is {0}")]
    RecruitmentNotOpen(RecruitmentStatus),
    #[error("recruitment message cannot be bound while status is {0}")]
    RecruitmentNotBindable(RecruitmentStatus),
    #[error("recruitment has reached its capacity")]
    RecruitmentFull,
    #[error("participant is already joined")]
    ParticipantAlreadyJoined,
    #[error("participant is not joined")]
    ParticipantNotJoined,
    #[error("preferred lanes must contain one to five unique canonical lanes")]
    InvalidPreferredLanes,
    #[error("closing recruitment requires {target} joined participants, found {joined}")]
    CloseRequiresFullRoster { joined: i64, target: i16 },
    #[error("idempotency key was reused with a different request")]
    IdempotencyConflict,
    #[error("the same idempotent command is still in progress")]
    IdempotencyInProgress,
    #[error("PostgreSQL bot workflow operation failed")]
    Database(#[source] sqlx::Error),
    #[error("PostgreSQL deadlock retry budget was exhausted")]
    DatabaseBusy,
    #[error("bot workflow response serialization failed")]
    Serialization(#[from] serde_json::Error),
    #[error("bot workflow storage invariant was violated")]
    InvariantViolation,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user_id(value: &str) -> DiscordUserId {
        DiscordUserId::parse(value).expect("valid user ID")
    }

    fn role_id(value: &str) -> DiscordRoleId {
        DiscordRoleId::parse(value).expect("valid role ID")
    }

    fn actor(role_ids: Vec<DiscordRoleId>) -> BotActor {
        BotActor {
            id: user_id("100000000000000001"),
            global_display_name: "Operator Global".to_owned(),
            display_name: "Operator".to_owned(),
            avatar_hash: None,
            role_ids,
        }
    }

    #[test]
    fn command_routes_are_canonical() {
        let operator = actor(vec![]);
        let recruitment_id = RecruitmentId::new();
        let commands = [
            BotRecruitmentCommand::Create {
                actor: operator.clone(),
            },
            BotRecruitmentCommand::BindMessage {
                actor: operator.clone(),
                recruitment_id,
                channel_id: DiscordChannelId::parse("400000000000000001").expect("valid channel"),
                message_id: DiscordMessageId::parse("500000000000000001").expect("valid message"),
            },
            BotRecruitmentCommand::Join {
                actor: operator.clone(),
                recruitment_id,
                participant: operator.clone(),
            },
            BotRecruitmentCommand::Leave {
                actor: operator.clone(),
                recruitment_id,
                user_id: UserId::from(&operator.id),
            },
            BotRecruitmentCommand::SetPreferredLanes {
                actor: operator.clone(),
                recruitment_id,
                user_id: UserId::from(&operator.id),
                lanes: vec![Lane::Mid],
            },
            BotRecruitmentCommand::Close {
                actor: operator,
                recruitment_id,
            },
        ];

        assert_eq!(
            commands.map(|command| command.metadata().route_template),
            [
                "/api/guilds/{guildId}/recruitments",
                "/api/guilds/{guildId}/recruitments/{id}/message",
                "/api/guilds/{guildId}/recruitments/{id}/members",
                "/api/guilds/{guildId}/recruitments/{id}/members/{userId}",
                "/api/guilds/{guildId}/recruitments/{id}/members/{userId}/preferred-lanes",
                "/api/guilds/{guildId}/recruitments/{id}/close",
            ]
        );
    }

    #[test]
    fn preferred_lanes_are_non_empty_bounded_and_unique() {
        assert!(validate_lanes(&[Lane::Top]).is_ok());
        assert!(validate_lanes(&[]).is_err());
        assert!(validate_lanes(&[Lane::Mid, Lane::Mid]).is_err());
    }

    #[test]
    fn self_service_or_operator_role_is_required() {
        let operator_role = role_id("300000000000000001");
        let target = user_id("100000000000000002");
        let self_actor = actor(vec![]);
        assert!(
            require_self_or_operator(&self_actor, &UserId::from(&self_actor.id), &operator_role)
                .is_ok()
        );
        assert!(
            require_self_or_operator(&self_actor, &UserId::from(&target), &operator_role).is_err()
        );

        let operator = actor(vec![operator_role.clone()]);
        assert!(
            require_self_or_operator(&operator, &UserId::from(&target), &operator_role).is_ok()
        );
    }
}
