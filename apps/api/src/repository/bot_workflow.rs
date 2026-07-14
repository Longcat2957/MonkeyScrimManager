use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

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
    mmr::{self, MmrLaneKey, MmrRepositoryError},
    notification_outbox::{
        self, EnqueueNotification, NotificationOutboxError, RECRUITMENT_MESSAGE_CREATE_TYPE,
        RECRUITMENT_MESSAGE_DELETE_TYPE, RECRUITMENT_MESSAGE_SCHEMA_VERSION,
        RECRUITMENT_MESSAGE_UPDATE_TYPE,
    },
    riot::{
        self, CanonicalRiotAccount, RiotAccountLinkState, RiotAccountRecord, RiotRepositoryError,
    },
};
use crate::{
    auth::AuthService,
    domain::{
        DiscordChannelId, DiscordInteractionId, DiscordMessageId, DiscordRoleId, DiscordUserId,
        GuildId, GuildMemberStatus, GuildStatus, IdempotencyRecordId, Lane, NotificationId,
        OutboxEventId, RecruitmentId, RecruitmentStatus, SeasonId, SeriesId, SeriesStatus, UserId,
    },
    infrastructure::postgres::{
        DatabaseTransaction, DeadlockRetryPolicy, TransactionAttemptError, TransactionRetryError,
        begin, run_with_deadlock_retry,
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
const SERIES_CREATE_METHOD: &str = "POST";
const SERIES_CREATE_ROUTE: &str = "/api/guilds/{guildId}/series";
const SERIES_REVERT_METHOD: &str = "POST";
const SERIES_REVERT_ROUTE: &str = "/api/guilds/{guildId}/series/{id}/revert";
const SERIES_DELETE_METHOD: &str = "DELETE";
const SERIES_DELETE_ROUTE: &str = "/api/guilds/{guildId}/series/{id}";
const SERIES_BEST_OF: i16 = 3;

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
    pub created_by_id: UserId,
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryDraftSnapshot {
    pub recruitment: EntryDraftRecruitmentSnapshot,
    pub draft: EntryDraftStateSnapshot,
    pub capabilities: EntryDraftCapabilities,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryDraftRecruitmentSnapshot {
    pub id: RecruitmentId,
    pub display_number: i64,
    pub title: String,
    pub status: RecruitmentStatus,
    pub target_count: i16,
    pub revision: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryDraftStateSnapshot {
    pub revision: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    pub updated_by: Option<EntryDraftEditorSnapshot>,
    pub participants: Vec<EntryDraftParticipantSnapshot>,
    pub assignments: Vec<EntryDraftAssignmentSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryDraftEditorSnapshot {
    pub id: UserId,
    pub display_name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryDraftParticipantSnapshot {
    pub id: UserId,
    pub display_name: String,
    pub preferred_lanes: Vec<Lane>,
    pub main_lane: Option<Lane>,
    pub mmr: f64,
    pub games: i32,
    pub wins: i32,
    pub riot_tier: Option<String>,
    pub profile_icon_id: Option<i32>,
    pub champion_ids: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryDraftAssignmentSnapshot {
    pub participant_id: UserId,
    pub team: String,
    pub lane: Lane,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryDraftCapabilities {
    pub can_edit: bool,
    pub can_reset: bool,
    pub can_create_series: bool,
    pub can_reopen: bool,
    pub can_cancel: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesCreationResult {
    pub series: CreatedSeriesSnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedSeriesSnapshot {
    pub id: SeriesId,
    pub href: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesPickBanSnapshot {
    pub series: SeriesPickBanSeriesSnapshot,
    pub teams: Vec<SeriesPickBanTeamSnapshot>,
    pub current_game: SeriesPickBanGameSnapshot,
    pub games: Vec<SeriesPickBanGameSnapshot>,
    pub capabilities: SeriesPickBanCapabilities,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesPickBanSeriesSnapshot {
    pub id: SeriesId,
    pub display_number: i64,
    pub title: String,
    pub best_of: i16,
    pub status: SeriesStatus,
    pub revision: i64,
    pub source_recruitment_id: RecruitmentId,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesPickBanTeamSnapshot {
    pub team: String,
    pub name: String,
    pub players: Vec<SeriesPickBanPlayerSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesPickBanPlayerSnapshot {
    pub id: UserId,
    pub display_name: String,
    pub lane: Lane,
    pub mmr: f64,
    pub riot_tier: Option<String>,
    pub preferred_champion_ids: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesPickBanGameSnapshot {
    pub set_number: i16,
    pub side_preset: String,
    pub winner: Option<String>,
    pub picks: BTreeMap<String, Option<String>>,
    pub bans: SeriesPickBanBansSnapshot,
    pub patch_version: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub struct SeriesPickBanBansSnapshot {
    pub blue: Vec<Option<String>>,
    pub red: Vec<Option<String>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesPickBanCapabilities {
    pub can_revert: bool,
    pub can_delete: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesLifecycleResult {
    pub next_href: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
pub enum EntryDraftOperation {
    MoveParticipant {
        #[serde(rename = "participantId")]
        participant_id: UserId,
        slot: Option<String>,
    },
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

    if let BotRecruitmentCommand::Create { actor } = &command {
        let request_hash = request_hash(guild_id, &command)?;
        return execute_recruitment_create(
            pool,
            guild_id,
            RecruitmentCreateActor::Bot(actor.clone()),
            IdempotencyPrincipalType::Bot,
            BOT_PRINCIPAL_ID,
            idempotency_key.as_str(),
            request_id,
            request_hash,
        )
        .await;
    }

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

pub async fn create_web_recruitment(
    pool: &PgPool,
    guild_id: &GuildId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<RecruitmentSnapshot, BotWorkflowError> {
    let request_hash = request_hash(guild_id, &WebRecruitmentCreateRequest {})?;
    execute_recruitment_create(
        pool,
        guild_id,
        RecruitmentCreateActor::Session(actor_id.clone()),
        IdempotencyPrincipalType::User,
        actor_id.as_str(),
        idempotency_key,
        request_id,
        request_hash,
    )
    .await
}

pub async fn add_web_recruitment_member(
    pool: &PgPool,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    actor_id: &UserId,
    member_id: UserId,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<RecruitmentSnapshot, BotWorkflowError> {
    execute_web_recruitment_mutation(
        pool,
        guild_id,
        actor_id,
        idempotency_key,
        request_id,
        WebRecruitmentMutation::AddMember {
            recruitment_id,
            member_id,
        },
    )
    .await
}

pub async fn remove_web_recruitment_member(
    pool: &PgPool,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    actor_id: &UserId,
    member_id: UserId,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<RecruitmentSnapshot, BotWorkflowError> {
    execute_web_recruitment_mutation(
        pool,
        guild_id,
        actor_id,
        idempotency_key,
        request_id,
        WebRecruitmentMutation::RemoveMember {
            recruitment_id,
            member_id,
        },
    )
    .await
}

pub async fn close_web_recruitment(
    pool: &PgPool,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<RecruitmentSnapshot, BotWorkflowError> {
    execute_web_recruitment_mutation(
        pool,
        guild_id,
        actor_id,
        idempotency_key,
        request_id,
        WebRecruitmentMutation::Close { recruitment_id },
    )
    .await
}

pub async fn cancel_web_recruitment(
    pool: &PgPool,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<RecruitmentSnapshot, BotWorkflowError> {
    execute_web_recruitment_mutation(
        pool,
        guild_id,
        actor_id,
        idempotency_key,
        request_id,
        WebRecruitmentMutation::Cancel { recruitment_id },
    )
    .await
}

pub async fn update_web_entry_draft(
    pool: &PgPool,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    actor_id: &UserId,
    expected_revision: i64,
    operation: EntryDraftOperation,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<EntryDraftSnapshot, BotWorkflowError> {
    execute_web_entry_draft_mutation(
        pool,
        guild_id,
        actor_id,
        idempotency_key,
        request_id,
        WebEntryDraftMutation::Apply {
            recruitment_id,
            expected_revision,
            operation,
        },
    )
    .await
}

pub async fn reset_web_entry_draft(
    pool: &PgPool,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    actor_id: &UserId,
    expected_revision: i64,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<EntryDraftSnapshot, BotWorkflowError> {
    execute_web_entry_draft_mutation(
        pool,
        guild_id,
        actor_id,
        idempotency_key,
        request_id,
        WebEntryDraftMutation::Reset {
            recruitment_id,
            expected_revision,
        },
    )
    .await
}

pub async fn reopen_web_recruitment(
    pool: &PgPool,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    actor_id: &UserId,
    expected_revision: i64,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<EntryDraftSnapshot, BotWorkflowError> {
    execute_web_entry_draft_mutation(
        pool,
        guild_id,
        actor_id,
        idempotency_key,
        request_id,
        WebEntryDraftMutation::Reopen {
            recruitment_id,
            expected_revision,
        },
    )
    .await
}

pub async fn create_web_series(
    pool: &PgPool,
    guild_id: &GuildId,
    actor_id: &UserId,
    source_recruitment_id: RecruitmentId,
    expected_revision: i64,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<SeriesCreationResult, BotWorkflowError> {
    if expected_revision < 0 {
        return Err(BotWorkflowError::InvalidCommand(
            "expected series source revision must be non-negative",
        ));
    }

    let command = WebSeriesCreateCommand {
        source_recruitment_id,
        expected_revision,
    };
    let request_hash = request_hash(guild_id, &command)?;
    let guild_id = guild_id.clone();
    let actor_id = actor_id.clone();
    let idempotency_key = idempotency_key.to_owned();
    let request_id = request_id.0.clone();
    let prepared = WebSeriesCreatePrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let actor_id = actor_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let command = command.clone();
            let request_hash = request_hash.clone();

            Box::pin(async move {
                execute_web_series_create_attempt(
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

pub async fn revert_web_series(
    pool: &PgPool,
    guild_id: &GuildId,
    series_id: SeriesId,
    actor_id: &UserId,
    expected_revision: i64,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<SeriesLifecycleResult, BotWorkflowError> {
    execute_web_series_lifecycle(
        pool,
        guild_id,
        series_id,
        actor_id,
        expected_revision,
        idempotency_key,
        request_id,
        SeriesLifecycleAction::Revert,
    )
    .await
}

pub async fn delete_web_series(
    pool: &PgPool,
    guild_id: &GuildId,
    series_id: SeriesId,
    actor_id: &UserId,
    expected_revision: i64,
    idempotency_key: &str,
    request_id: &RequestId,
) -> Result<SeriesLifecycleResult, BotWorkflowError> {
    execute_web_series_lifecycle(
        pool,
        guild_id,
        series_id,
        actor_id,
        expected_revision,
        idempotency_key,
        request_id,
        SeriesLifecycleAction::Delete,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn execute_web_series_lifecycle(
    pool: &PgPool,
    guild_id: &GuildId,
    series_id: SeriesId,
    actor_id: &UserId,
    expected_revision: i64,
    idempotency_key: &str,
    request_id: &RequestId,
    action: SeriesLifecycleAction,
) -> Result<SeriesLifecycleResult, BotWorkflowError> {
    if expected_revision < 0 {
        return Err(BotWorkflowError::InvalidCommand(
            "expected series revision must be non-negative",
        ));
    }

    let command = WebSeriesLifecycleCommand {
        series_id,
        expected_revision,
        action,
    };
    let request_hash = request_hash(guild_id, &command)?;
    let guild_id = guild_id.clone();
    let actor_id = actor_id.clone();
    let idempotency_key = idempotency_key.to_owned();
    let request_id = request_id.0.clone();
    let prepared = WebSeriesLifecyclePrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let actor_id = actor_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let command = command.clone();
            let request_hash = request_hash.clone();

            Box::pin(async move {
                execute_web_series_lifecycle_attempt(
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

pub async fn get_series_pick_ban_snapshot(
    pool: &PgPool,
    guild_id: &GuildId,
    series_id: SeriesId,
    viewer_id: &UserId,
) -> Result<SeriesPickBanSnapshot, BotWorkflowError> {
    let mut transaction = begin(pool).await.map_err(BotWorkflowError::Database)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *transaction)
        .await
        .map_err(BotWorkflowError::Database)?;
    let snapshot = series_pick_ban_snapshot(&mut transaction, guild_id, series_id, viewer_id)
        .await
        .map_err(AttemptError::into_public)?;
    transaction
        .commit()
        .await
        .map_err(BotWorkflowError::Database)?;
    Ok(snapshot)
}

pub async fn get_entry_draft_snapshot(
    pool: &PgPool,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    viewer_id: &UserId,
) -> Result<EntryDraftSnapshot, BotWorkflowError> {
    let mut transaction = begin(pool).await.map_err(BotWorkflowError::Database)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *transaction)
        .await
        .map_err(BotWorkflowError::Database)?;
    let snapshot = entry_draft_snapshot(&mut transaction, guild_id, recruitment_id, viewer_id)
        .await
        .map_err(AttemptError::into_public)?;
    if !matches!(
        snapshot.recruitment.status,
        RecruitmentStatus::Open | RecruitmentStatus::Closed
    ) {
        return Err(BotWorkflowError::RecruitmentNotFound);
    }
    transaction
        .commit()
        .await
        .map_err(BotWorkflowError::Database)?;
    Ok(snapshot)
}

pub async fn get_recruitment_snapshot(
    pool: &PgPool,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
) -> Result<RecruitmentSnapshot, BotWorkflowError> {
    let mut transaction = begin(pool).await.map_err(BotWorkflowError::Database)?;
    let snapshot = recruitment_snapshot(&mut transaction, guild_id, recruitment_id)
        .await
        .map_err(AttemptError::into_public)?;
    if !matches!(
        snapshot.recruitment.status,
        RecruitmentStatus::Open | RecruitmentStatus::Closed
    ) {
        return Err(BotWorkflowError::RecruitmentNotFound);
    }
    transaction
        .commit()
        .await
        .map_err(BotWorkflowError::Database)?;
    Ok(snapshot)
}

#[allow(clippy::too_many_arguments)]
async fn execute_recruitment_create(
    pool: &PgPool,
    guild_id: &GuildId,
    actor: RecruitmentCreateActor,
    principal_type: IdempotencyPrincipalType,
    principal_id: &str,
    idempotency_key: &str,
    request_id: &RequestId,
    request_hash: Vec<u8>,
) -> Result<RecruitmentSnapshot, BotWorkflowError> {
    let guild_id = guild_id.clone();
    let principal_id = principal_id.to_owned();
    let idempotency_key = idempotency_key.to_owned();
    let request_id = request_id.0.clone();
    let prepared = RecruitmentPrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let actor = actor.clone();
            let principal_id = principal_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let request_hash = request_hash.clone();

            Box::pin(async move {
                execute_recruitment_create_attempt(
                    transaction,
                    &guild_id,
                    &actor,
                    principal_type,
                    &principal_id,
                    &idempotency_key,
                    &request_id,
                    &request_hash,
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

async fn execute_web_recruitment_mutation(
    pool: &PgPool,
    guild_id: &GuildId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &RequestId,
    command: WebRecruitmentMutation,
) -> Result<RecruitmentSnapshot, BotWorkflowError> {
    let request_hash = request_hash(guild_id, &command)?;
    let guild_id = guild_id.clone();
    let actor_id = actor_id.clone();
    let idempotency_key = idempotency_key.to_owned();
    let request_id = request_id.0.clone();
    let prepared = WebRecruitmentMutationPrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let actor_id = actor_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let command = command.clone();
            let request_hash = request_hash.clone();

            Box::pin(async move {
                execute_web_recruitment_mutation_attempt(
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

async fn execute_web_entry_draft_mutation(
    pool: &PgPool,
    guild_id: &GuildId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &RequestId,
    command: WebEntryDraftMutation,
) -> Result<EntryDraftSnapshot, BotWorkflowError> {
    let request_hash = request_hash(guild_id, &command)?;
    let guild_id = guild_id.clone();
    let actor_id = actor_id.clone();
    let idempotency_key = idempotency_key.to_owned();
    let request_id = request_id.0.clone();
    let prepared = WebEntryDraftMutationPrepared::new();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let actor_id = actor_id.clone();
            let idempotency_key = idempotency_key.clone();
            let request_id = request_id.clone();
            let command = command.clone();
            let request_hash = request_hash.clone();

            Box::pin(async move {
                execute_web_entry_draft_mutation_attempt(
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
            principal_type: IdempotencyPrincipalType::Bot,
            principal_id: BOT_PRINCIPAL_ID,
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
            actor_id: command.actor.id.as_str(),
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
            principal_type: IdempotencyPrincipalType::Bot,
            principal_id: BOT_PRINCIPAL_ID,
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
            principal_type: IdempotencyPrincipalType::Bot,
            principal_id: BOT_PRINCIPAL_ID,
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
    let actor_user_id = UserId::from(&actor.id);
    sync_guild_actor(transaction, guild_id, actor, prepared.now).await?;
    require_active_member(transaction, guild_id, &actor.id).await?;

    let (before, snapshot) = match command {
        // Create commands are dispatched to the shared Bot/session create transaction before
        // this mutation-only command path is entered.
        BotRecruitmentCommand::Create { .. } => return Err(AttemptError::InvariantViolation),
        BotRecruitmentCommand::BindMessage {
            recruitment_id,
            channel_id,
            message_id,
            ..
        } => {
            let locked = lock_recruitment(transaction, guild_id, *recruitment_id).await?;
            require_creator_or_operator(actor, &locked.created_by, &operator_role_id)?;
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
                &participant_user_id,
                locked.target_count,
                prepared.now,
            )
            .await?;
            bump_recruitment_and_draft(
                transaction,
                guild_id,
                *recruitment_id,
                &actor_user_id,
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
                &actor_user_id,
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
                &actor_user_id,
                prepared.now,
            )
            .await?;
            let snapshot = recruitment_snapshot(transaction, guild_id, *recruitment_id).await?;
            (Some(before), snapshot)
        }
        BotRecruitmentCommand::Close { recruitment_id, .. } => {
            require_operator(actor, &operator_role_id)?;
            let (before, snapshot) = close_recruitment_for_entry(
                transaction,
                guild_id,
                *recruitment_id,
                &actor_user_id,
                prepared.now,
            )
            .await?;
            (Some(before), snapshot)
        }
    };

    let recruitment_id = snapshot.recruitment.id.to_string();
    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id: actor.id.as_str(),
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

#[allow(clippy::too_many_arguments)]
async fn execute_web_recruitment_mutation_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &str,
    request_hash: &[u8],
    command: &WebRecruitmentMutation,
    prepared: WebRecruitmentMutationPrepared,
) -> Result<RecruitmentSnapshot, AttemptError> {
    let metadata = command.metadata();
    let operator_role_id = lock_active_guild(transaction, guild_id).await?;
    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<RecruitmentSnapshot>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            principal_type: IdempotencyPrincipalType::User,
            principal_id: actor_id.as_str(),
            method: metadata.method,
            route_template: metadata.route_template,
            key: idempotency_key,
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };

    let (before, snapshot) = match command {
        WebRecruitmentMutation::AddMember {
            recruitment_id,
            member_id,
        } => {
            require_active_session_operator(transaction, guild_id, actor_id, &operator_role_id)
                .await?;
            let locked = lock_recruitment(transaction, guild_id, *recruitment_id).await?;
            ensure_open(locked.status)?;
            lock_entry_draft(transaction, guild_id, *recruitment_id).await?;
            require_active_recruitment_member(transaction, guild_id, member_id).await?;
            let before = recruitment_snapshot(transaction, guild_id, *recruitment_id).await?;
            join_recruitment(
                transaction,
                guild_id,
                *recruitment_id,
                member_id,
                locked.target_count,
                prepared.now,
            )
            .await?;
            bump_recruitment_and_draft(
                transaction,
                guild_id,
                *recruitment_id,
                actor_id,
                prepared.now,
            )
            .await?;
            let snapshot = recruitment_snapshot(transaction, guild_id, *recruitment_id).await?;
            (Some(before), snapshot)
        }
        WebRecruitmentMutation::RemoveMember {
            recruitment_id,
            member_id,
        } => {
            require_active_session_operator(transaction, guild_id, actor_id, &operator_role_id)
                .await?;
            let locked = lock_recruitment(transaction, guild_id, *recruitment_id).await?;
            ensure_open(locked.status)?;
            lock_entry_draft(transaction, guild_id, *recruitment_id).await?;
            let before = recruitment_snapshot(transaction, guild_id, *recruitment_id).await?;
            leave_recruitment(
                transaction,
                guild_id,
                *recruitment_id,
                member_id,
                prepared.now,
            )
            .await?;
            bump_recruitment_and_draft(
                transaction,
                guild_id,
                *recruitment_id,
                actor_id,
                prepared.now,
            )
            .await?;
            let snapshot = recruitment_snapshot(transaction, guild_id, *recruitment_id).await?;
            (Some(before), snapshot)
        }
        WebRecruitmentMutation::Close { recruitment_id } => {
            require_active_session_operator(transaction, guild_id, actor_id, &operator_role_id)
                .await?;
            let (before, snapshot) = close_recruitment_for_entry(
                transaction,
                guild_id,
                *recruitment_id,
                actor_id,
                prepared.now,
            )
            .await?;
            (Some(before), snapshot)
        }
        WebRecruitmentMutation::Cancel { recruitment_id } => {
            let locked = lock_recruitment(transaction, guild_id, *recruitment_id).await?;
            require_active_creator_or_operator(
                transaction,
                guild_id,
                actor_id,
                &locked.created_by,
                &operator_role_id,
            )
            .await?;
            ensure_cancellable(locked.status)?;
            let before = recruitment_snapshot(transaction, guild_id, *recruitment_id).await?;
            let cancelled = sqlx::query(
                "UPDATE recruitments
                 SET status = 'CANCELLED',
                     revision = revision + 1,
                     updated_by = $1,
                     updated_at = $2,
                     cancelled_at = $2
                 WHERE guild_id = $3 AND id = $4",
            )
            .bind(actor_id)
            .bind(prepared.now)
            .bind(guild_id)
            .bind(recruitment_id.as_uuid())
            .execute(&mut **transaction)
            .await?;
            if cancelled.rows_affected() != 1 {
                return Err(AttemptError::InvariantViolation);
            }
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
            actor_id: actor_id.as_str(),
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
    enqueue_web_recruitment_message_notification(
        transaction,
        guild_id,
        &snapshot,
        command,
        prepared.notification_id,
        prepared.now,
    )
    .await?;
    complete_idempotency(transaction, &scope, record_id, 200, &snapshot, prepared.now).await?;

    Ok(snapshot)
}

#[allow(clippy::too_many_arguments)]
async fn execute_web_entry_draft_mutation_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &str,
    request_hash: &[u8],
    command: &WebEntryDraftMutation,
    prepared: WebEntryDraftMutationPrepared,
) -> Result<EntryDraftSnapshot, AttemptError> {
    let metadata = command.metadata();
    let operator_role_id = lock_active_guild(transaction, guild_id).await?;
    require_active_session_operator(transaction, guild_id, actor_id, &operator_role_id).await?;

    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<EntryDraftSnapshot>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            principal_type: IdempotencyPrincipalType::User,
            principal_id: actor_id.as_str(),
            method: metadata.method,
            route_template: metadata.route_template,
            key: idempotency_key,
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };

    let recruitment_id = command.recruitment_id();
    let locked = lock_recruitment(transaction, guild_id, recruitment_id).await?;
    let draft_revision = lock_entry_draft_revision(transaction, guild_id, recruitment_id).await?;
    let actual_revision = match command {
        WebEntryDraftMutation::Reopen { .. } => locked.revision,
        WebEntryDraftMutation::Apply { .. } | WebEntryDraftMutation::Reset { .. } => draft_revision,
    };
    if actual_revision != command.expected_revision() {
        return Err(AttemptError::EntryDraftRevisionConflict {
            expected: command.expected_revision(),
            actual: actual_revision,
        });
    }

    let before = entry_draft_snapshot(transaction, guild_id, recruitment_id, actor_id).await?;
    let mut message_snapshot = None;
    match command {
        WebEntryDraftMutation::Apply { operation, .. } => {
            ensure_entry_editable(locked.status)?;
            apply_entry_draft_operation(transaction, guild_id, recruitment_id, operation).await?;
            bump_entry_draft(
                transaction,
                guild_id,
                recruitment_id,
                actor_id,
                prepared.now,
            )
            .await?;
        }
        WebEntryDraftMutation::Reset { .. } => {
            ensure_entry_editable(locked.status)?;
            sqlx::query(
                "DELETE FROM recruitment_entry_assignments
                 WHERE guild_id = $1 AND recruitment_id = $2",
            )
            .bind(guild_id)
            .bind(recruitment_id.as_uuid())
            .execute(&mut **transaction)
            .await?;
            bump_entry_draft(
                transaction,
                guild_id,
                recruitment_id,
                actor_id,
                prepared.now,
            )
            .await?;
        }
        WebEntryDraftMutation::Reopen { .. } => {
            ensure_reopenable(locked.status)?;
            let reopened = sqlx::query(
                "UPDATE recruitments
                 SET status = 'OPEN',
                     revision = revision + 1,
                     updated_by = $1,
                     updated_at = $2,
                     closed_at = NULL,
                     cancelled_at = NULL
                 WHERE guild_id = $3 AND id = $4",
            )
            .bind(actor_id)
            .bind(prepared.now)
            .bind(guild_id)
            .bind(recruitment_id.as_uuid())
            .execute(&mut **transaction)
            .await?;
            if reopened.rows_affected() != 1 {
                return Err(AttemptError::InvariantViolation);
            }
            bump_entry_draft(
                transaction,
                guild_id,
                recruitment_id,
                actor_id,
                prepared.now,
            )
            .await?;
            message_snapshot =
                Some(recruitment_snapshot(transaction, guild_id, recruitment_id).await?);
        }
    }

    let snapshot = entry_draft_snapshot(transaction, guild_id, recruitment_id, actor_id).await?;
    let target_id = recruitment_id.to_string();
    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id: actor_id.as_str(),
            action: metadata.audit_action,
            target_type: "RECRUITMENT_ENTRY_DRAFT",
            target_id: &target_id,
            request_id,
            before_summary: Some(serde_json::to_value(&before)?),
            after_summary: Some(serde_json::to_value(&snapshot)?),
            now: prepared.now,
        },
    )
    .await?;
    insert_outbox(
        transaction,
        guild_id,
        prepared.outbox_event_id,
        format!("guild:{guild_id}:recruitment-entry-draft:{recruitment_id}"),
        "RECRUITMENT_ENTRY_DRAFT",
        &target_id,
        snapshot.draft.revision,
        metadata.event_type,
        json!({
            "type": "invalidate",
            "resourceId": recruitment_id,
            "revision": snapshot.draft.revision,
        }),
        prepared.now,
    )
    .await?;
    if matches!(command, WebEntryDraftMutation::Reopen { .. }) {
        insert_outbox(
            transaction,
            guild_id,
            prepared.recruitment_outbox_event_id,
            format!("guild:{guild_id}:recruitment:{recruitment_id}"),
            "RECRUITMENT",
            &target_id,
            snapshot.recruitment.revision,
            metadata.event_type,
            json!({
                "type": "invalidate",
                "resourceId": recruitment_id,
                "revision": snapshot.recruitment.revision,
            }),
            prepared.now,
        )
        .await?;
    }
    if let Some(message_snapshot) = message_snapshot.as_ref() {
        enqueue_recruitment_message_update(
            transaction,
            guild_id,
            message_snapshot,
            prepared.notification_id,
            prepared.now,
        )
        .await?;
    }
    complete_idempotency(transaction, &scope, record_id, 200, &snapshot, prepared.now).await?;

    Ok(snapshot)
}

#[allow(clippy::too_many_arguments)]
async fn execute_web_series_create_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &str,
    request_hash: &[u8],
    command: &WebSeriesCreateCommand,
    prepared: WebSeriesCreatePrepared,
) -> Result<SeriesCreationResult, AttemptError> {
    let operator_role_id = lock_active_guild(transaction, guild_id).await?;
    require_active_session_operator(transaction, guild_id, actor_id, &operator_role_id).await?;

    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<SeriesCreationResult>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            principal_type: IdempotencyPrincipalType::User,
            principal_id: actor_id.as_str(),
            method: SERIES_CREATE_METHOD,
            route_template: SERIES_CREATE_ROUTE,
            key: idempotency_key,
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };

    // Preserve the common recruitment -> entry-draft lock order used by every entry mutation.
    let recruitment =
        lock_recruitment(transaction, guild_id, command.source_recruitment_id).await?;
    if recruitment.revision != command.expected_revision {
        return Err(AttemptError::RecruitmentRevisionConflict {
            expected: command.expected_revision,
            actual: recruitment.revision,
        });
    }
    ensure_series_source_ready(recruitment.status)?;
    lock_entry_draft(transaction, guild_id, command.source_recruitment_id).await?;

    let assignments = locked_full_entry_assignments(
        transaction,
        guild_id,
        command.source_recruitment_id,
        recruitment.target_count,
    )
    .await?;
    let mmr_rows = mmr::ensure_and_lock_mmr_rows(
        transaction,
        guild_id,
        SeasonId::from_uuid(recruitment.season_id),
        assignments.iter().map(|assignment| MmrLaneKey {
            user_id: assignment.user_id.clone(),
            lane: assignment.lane,
        }),
        prepared.now,
    )
    .await?;
    let mmr_by_assignment = mmr_rows
        .into_iter()
        .map(|row| ((row.key.user_id, row.key.lane), row.mmr))
        .collect::<HashMap<_, _>>();
    if mmr_by_assignment.len() != assignments.len() {
        return Err(AttemptError::InvariantViolation);
    }

    let display_number = next_series_display_number(transaction, guild_id).await?;
    let title = format!("일반 내전 #{display_number}");
    sqlx::query(
        "INSERT INTO series (
             guild_id, id, season_id, source_recruitment_id, display_number, title,
             best_of, status, winning_team, terminal_reason, revision,
             started_at, ended_at, created_by, end_card_channel_id, end_card_message_id,
             updated_by, updated_at, deleted_at
         ) VALUES (
             $1, $2, $3, $4, $5, $6,
             $7, 'IN_PROGRESS', NULL, NULL, 0,
             $8, NULL, $9, NULL, NULL,
             $9, $8, NULL
         )",
    )
    .bind(guild_id)
    .bind(prepared.series_id.as_uuid())
    .bind(recruitment.season_id)
    .bind(command.source_recruitment_id.as_uuid())
    .bind(display_number)
    .bind(&title)
    .bind(SERIES_BEST_OF)
    .bind(prepared.now)
    .bind(actor_id)
    .execute(&mut **transaction)
    .await?;

    for assignment in &assignments {
        let mmr_at_lock = mmr_by_assignment
            .get(&(assignment.user_id.clone(), assignment.lane))
            .copied()
            .ok_or(AttemptError::InvariantViolation)?;
        sqlx::query(
            "INSERT INTO series_participants (
                 guild_id, series_id, user_id, team, lane, mmr_at_lock, mmr_snapshot_origin
             ) VALUES ($1, $2, $3, $4, $5, $6, 'CAPTURED')",
        )
        .bind(guild_id)
        .bind(prepared.series_id.as_uuid())
        .bind(&assignment.user_id)
        .bind(&assignment.team)
        .bind(assignment.lane)
        .bind(mmr_at_lock)
        .execute(&mut **transaction)
        .await?;
    }

    let converted_revision = recruitment
        .revision
        .checked_add(1)
        .ok_or(AttemptError::InvariantViolation)?;
    let converted = sqlx::query(
        "UPDATE recruitments
         SET status = 'CONVERTED',
             revision = revision + 1,
             updated_by = $1,
             updated_at = $2,
             converted_at = $2
         WHERE guild_id = $3 AND id = $4",
    )
    .bind(actor_id)
    .bind(prepared.now)
    .bind(guild_id)
    .bind(command.source_recruitment_id.as_uuid())
    .execute(&mut **transaction)
    .await?;
    if converted.rows_affected() != 1 {
        return Err(AttemptError::InvariantViolation);
    }

    let result = SeriesCreationResult {
        series: CreatedSeriesSnapshot {
            id: prepared.series_id,
            href: format!("/g/{guild_id}/series/{}/pick-ban", prepared.series_id),
        },
    };
    let source_id = command.source_recruitment_id.to_string();
    let series_id = prepared.series_id.to_string();
    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id: actor_id.as_str(),
            action: "SERIES_CREATED",
            target_type: "SERIES",
            target_id: &series_id,
            request_id,
            before_summary: Some(json!({
                "sourceRecruitmentId": command.source_recruitment_id,
                "sourceRecruitmentRevision": recruitment.revision,
                "assignmentCount": assignments.len(),
            })),
            after_summary: Some(serde_json::to_value(&result)?),
            now: prepared.now,
        },
    )
    .await?;
    insert_outbox(
        transaction,
        guild_id,
        prepared.recruitment_outbox_event_id,
        format!(
            "guild:{guild_id}:recruitment:{}",
            command.source_recruitment_id
        ),
        "RECRUITMENT",
        &source_id,
        converted_revision,
        "RECRUITMENT_CONVERTED",
        json!({
            "type": "invalidate",
            "resourceId": command.source_recruitment_id,
            "revision": converted_revision,
        }),
        prepared.now,
    )
    .await?;
    insert_outbox(
        transaction,
        guild_id,
        prepared.series_outbox_event_id,
        format!("guild:{guild_id}:series:{}", prepared.series_id),
        "SERIES",
        &series_id,
        0,
        "SERIES_CREATED",
        json!({
            "type": "invalidate",
            "resourceId": prepared.series_id,
            "revision": 0,
        }),
        prepared.now,
    )
    .await?;
    complete_idempotency(transaction, &scope, record_id, 201, &result, prepared.now).await?;

    Ok(result)
}

#[allow(clippy::too_many_arguments)]
async fn execute_web_series_lifecycle_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &str,
    request_hash: &[u8],
    command: &WebSeriesLifecycleCommand,
    prepared: WebSeriesLifecyclePrepared,
) -> Result<SeriesLifecycleResult, AttemptError> {
    let operator_role_id = lock_active_guild(transaction, guild_id).await?;
    require_active_session_operator(transaction, guild_id, actor_id, &operator_role_id).await?;

    let metadata = command.action.metadata();
    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<SeriesLifecycleResult>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            principal_type: IdempotencyPrincipalType::User,
            principal_id: actor_id.as_str(),
            method: metadata.method,
            route_template: metadata.route_template,
            key: idempotency_key,
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };

    // A lifecycle mutation always follows the documented source recruitment -> series lock order.
    let source_recruitment_id =
        resolve_series_source_recruitment(transaction, guild_id, command.series_id).await?;
    let recruitment = lock_recruitment(transaction, guild_id, source_recruitment_id).await?;
    let series = lock_series(transaction, guild_id, command.series_id).await?;
    if series.source_recruitment_id != source_recruitment_id {
        return Err(AttemptError::InvariantViolation);
    }
    if series.revision != command.expected_revision {
        return Err(AttemptError::SeriesRevisionConflict {
            expected: command.expected_revision,
            actual: series.revision,
        });
    }
    ensure_series_in_progress(series.status)?;
    ensure_series_source_converted(recruitment.status)?;

    let confirmed_game_count =
        confirmed_series_game_count(transaction, guild_id, command.series_id).await?;
    if confirmed_game_count > 0 {
        return Err(AttemptError::SeriesHasConfirmedGames {
            count: confirmed_game_count,
        });
    }

    let series_revision = series
        .revision
        .checked_add(1)
        .ok_or(AttemptError::InvariantViolation)?;
    let recruitment_revision = recruitment
        .revision
        .checked_add(1)
        .ok_or(AttemptError::InvariantViolation)?;
    let cancelled = sqlx::query(
        "UPDATE series
         SET status = 'CANCELLED',
             winning_team = NULL,
             terminal_reason = 'CANCELLED',
             revision = revision + 1,
             ended_at = $1,
             updated_by = $2,
             updated_at = $1
         WHERE guild_id = $3 AND id = $4",
    )
    .bind(prepared.now)
    .bind(actor_id)
    .bind(guild_id)
    .bind(command.series_id.as_uuid())
    .execute(&mut **transaction)
    .await?;
    if cancelled.rows_affected() != 1 {
        return Err(AttemptError::InvariantViolation);
    }

    let next_href = match command.action {
        SeriesLifecycleAction::Revert => {
            let restored = sqlx::query(
                "UPDATE recruitments
                 SET status = 'CLOSED',
                     revision = revision + 1,
                     updated_by = $1,
                     updated_at = $2,
                     converted_at = NULL,
                     cancelled_at = NULL
                 WHERE guild_id = $3 AND id = $4",
            )
            .bind(actor_id)
            .bind(prepared.now)
            .bind(guild_id)
            .bind(source_recruitment_id.as_uuid())
            .execute(&mut **transaction)
            .await?;
            if restored.rows_affected() != 1 {
                return Err(AttemptError::InvariantViolation);
            }
            format!("/g/{guild_id}/recruitments/{source_recruitment_id}/entry")
        }
        SeriesLifecycleAction::Delete => {
            let deleted = sqlx::query(
                "UPDATE recruitments
                 SET status = 'CANCELLED',
                     revision = revision + 1,
                     updated_by = $1,
                     updated_at = $2,
                     converted_at = NULL,
                     cancelled_at = $2
                 WHERE guild_id = $3 AND id = $4",
            )
            .bind(actor_id)
            .bind(prepared.now)
            .bind(guild_id)
            .bind(source_recruitment_id.as_uuid())
            .execute(&mut **transaction)
            .await?;
            if deleted.rows_affected() != 1 {
                return Err(AttemptError::InvariantViolation);
            }
            format!("/g/{guild_id}/series")
        }
    };
    let result = SeriesLifecycleResult { next_href };
    let series_id = command.series_id.to_string();
    let source_id = source_recruitment_id.to_string();
    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id: actor_id.as_str(),
            action: metadata.audit_action,
            target_type: "SERIES",
            target_id: &series_id,
            request_id,
            before_summary: Some(json!({
                "seriesRevision": series.revision,
                "seriesStatus": series.status.as_str(),
                "sourceRecruitmentId": source_recruitment_id,
                "sourceRecruitmentRevision": recruitment.revision,
                "sourceRecruitmentStatus": recruitment.status.as_str(),
            })),
            after_summary: Some(json!({
                "seriesRevision": series_revision,
                "seriesStatus": "CANCELLED",
                "sourceRecruitmentId": source_recruitment_id,
                "sourceRecruitmentRevision": recruitment_revision,
                "sourceRecruitmentStatus": metadata.source_status,
                "nextHref": result.next_href,
            })),
            now: prepared.now,
        },
    )
    .await?;
    insert_outbox(
        transaction,
        guild_id,
        prepared.series_outbox_event_id,
        format!("guild:{guild_id}:series:{}", command.series_id),
        "SERIES",
        &series_id,
        series_revision,
        metadata.series_event_type,
        json!({
            "type": "invalidate",
            "resourceId": command.series_id,
            "revision": series_revision,
        }),
        prepared.now,
    )
    .await?;
    insert_outbox(
        transaction,
        guild_id,
        prepared.recruitment_outbox_event_id,
        format!("guild:{guild_id}:recruitment:{source_recruitment_id}"),
        "RECRUITMENT",
        &source_id,
        recruitment_revision,
        metadata.recruitment_event_type,
        json!({
            "type": "invalidate",
            "resourceId": source_recruitment_id,
            "revision": recruitment_revision,
        }),
        prepared.now,
    )
    .await?;
    complete_idempotency(transaction, &scope, record_id, 200, &result, prepared.now).await?;

    Ok(result)
}

#[allow(clippy::too_many_arguments)]
async fn execute_recruitment_create_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor: &RecruitmentCreateActor,
    principal_type: IdempotencyPrincipalType,
    principal_id: &str,
    idempotency_key: &str,
    request_id: &str,
    request_hash: &[u8],
    prepared: RecruitmentPrepared,
) -> Result<RecruitmentSnapshot, AttemptError> {
    let _operator_role_id = lock_active_guild(transaction, guild_id).await?;
    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<RecruitmentSnapshot>(
        transaction,
        IdempotencyAcquire {
            scope: &scope,
            record_id: prepared.idempotency_record_id,
            principal_type,
            principal_id,
            method: "POST",
            route_template: "/api/guilds/{guildId}/recruitments",
            key: idempotency_key,
            request_hash,
            now: prepared.now,
        },
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(result) => return Ok(result),
    };

    let actor_id = actor.user_id();
    if let RecruitmentCreateActor::Bot(actor) = actor {
        sync_guild_actor(transaction, guild_id, actor, prepared.now).await?;
    }
    require_active_member_for_creation(transaction, guild_id, &actor_id).await?;
    let recruitment_channel_id = match actor {
        RecruitmentCreateActor::Bot(_) => None,
        RecruitmentCreateActor::Session(_) => {
            Some(require_recruitment_channel(transaction, guild_id).await?)
        }
    };
    let snapshot = insert_recruitment(
        transaction,
        guild_id,
        &actor_id,
        prepared.recruitment_id,
        prepared.now,
    )
    .await?;

    if let Some(channel_id) = recruitment_channel_id.as_ref() {
        enqueue_recruitment_message_notification(
            transaction,
            guild_id,
            channel_id,
            &snapshot,
            RECRUITMENT_MESSAGE_CREATE_TYPE,
            format!("recruitment:{}:initial-message", snapshot.recruitment.id),
            prepared.notification_id,
            prepared.now,
        )
        .await?;
    }

    let recruitment_id = snapshot.recruitment.id.to_string();
    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: prepared.audit_id,
            actor_id: actor_id.as_str(),
            action: "RECRUITMENT_CREATE",
            target_type: "RECRUITMENT",
            target_id: &recruitment_id,
            request_id,
            before_summary: None,
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
        "RECRUITMENT_CREATED",
        json!({
            "type": "invalidate",
            "resourceId": snapshot.recruitment.id,
            "revision": snapshot.recruitment.revision,
        }),
        prepared.now,
    )
    .await?;
    complete_idempotency(transaction, &scope, record_id, 201, &snapshot, prepared.now).await?;

    Ok(snapshot)
}

async fn insert_recruitment(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &UserId,
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
    .bind(actor_id)
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
    .bind(actor_id)
    .bind(now)
    .execute(&mut **transaction)
    .await?;

    recruitment_snapshot(transaction, guild_id, recruitment_id).await
}

async fn join_recruitment(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    participant_id: &UserId,
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
    .bind(participant_id)
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
    .bind(participant_id)
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

async fn entry_draft_snapshot(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    viewer_id: &UserId,
) -> Result<EntryDraftSnapshot, AttemptError> {
    let recruitment = sqlx::query_as::<_, EntryDraftRecruitmentRow>(
        "SELECT recruitment.id,
                recruitment.display_number,
                recruitment.title,
                recruitment.status,
                recruitment.target_count,
                recruitment.revision,
                recruitment.updated_at,
                recruitment.created_by,
                draft.revision AS draft_revision,
                draft.updated_at AS draft_updated_at,
                draft.updated_by AS draft_updated_by,
                editor.display_name AS draft_updated_by_display_name
         FROM recruitments AS recruitment
         JOIN recruitment_entry_drafts AS draft
           ON draft.guild_id = recruitment.guild_id
          AND draft.recruitment_id = recruitment.id
         LEFT JOIN guild_members AS editor
           ON editor.guild_id = draft.guild_id
          AND editor.user_id = draft.updated_by
         WHERE recruitment.guild_id = $1 AND recruitment.id = $2",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::RecruitmentNotFound)?;

    let participant_rows = sqlx::query_as::<_, EntryDraftParticipantRow>(
        "SELECT participant.user_id AS id,
                member.display_name,
                member.main_lane,
                COALESCE(mmr.mmr, 1500::double precision) AS mmr,
                COALESCE(mmr.games_played, 0) AS games,
                COALESCE(mmr.wins, 0) AS wins,
                solo_rank.tier AS solo_rank_tier,
                solo_rank.division AS solo_rank_division,
                account.profile_icon_id
         FROM recruitment_participants AS participant
         JOIN guild_members AS member
           ON member.guild_id = participant.guild_id
          AND member.user_id = participant.user_id
         JOIN recruitments AS recruitment
           ON recruitment.guild_id = participant.guild_id
          AND recruitment.id = participant.recruitment_id
         LEFT JOIN recruitment_entry_assignments AS assignment
           ON assignment.guild_id = participant.guild_id
          AND assignment.recruitment_id = participant.recruitment_id
          AND assignment.user_id = participant.user_id
         LEFT JOIN LATERAL (
             SELECT state.mmr, state.games_played, state.wins
             FROM user_lane_mmr AS state
             WHERE state.guild_id = participant.guild_id
               AND state.season_id = recruitment.season_id
               AND state.user_id = participant.user_id
               AND state.lane = COALESCE(
                   assignment.lane,
                   member.main_lane,
                   (
                       SELECT preferred.lane
                       FROM recruitment_preferred_lanes AS preferred
                       WHERE preferred.guild_id = participant.guild_id
                         AND preferred.recruitment_id = participant.recruitment_id
                         AND preferred.user_id = participant.user_id
                       ORDER BY preferred.priority
                       LIMIT 1
                   )
               )
             LIMIT 1
         ) AS mmr ON TRUE
         LEFT JOIN LATERAL (
             SELECT riot.platform, riot.puuid, riot.profile_icon_id
             FROM riot_accounts AS riot
             WHERE riot.user_id = participant.user_id
               AND riot.is_main = true
               AND riot.origin = 'RIOT'
             ORDER BY riot.updated_at DESC
             LIMIT 1
         ) AS account ON TRUE
         LEFT JOIN LATERAL (
             SELECT rank_entry.value ->> 'tier' AS tier,
                    rank_entry.value ->> 'rank' AS division
             FROM riot_records_snapshots AS records
             CROSS JOIN LATERAL jsonb_array_elements(
                 CASE
                     WHEN jsonb_typeof(records.snapshot -> 'ranks') = 'array'
                         THEN records.snapshot -> 'ranks'
                     ELSE '[]'::jsonb
                 END
             ) AS rank_entry(value)
             WHERE records.platform = account.platform
               AND records.puuid = account.puuid
               AND rank_entry.value ->> 'queueType' = 'RANKED_SOLO_5x5'
             LIMIT 1
         ) AS solo_rank ON TRUE
         WHERE participant.guild_id = $1
           AND participant.recruitment_id = $2
           AND participant.status = 'JOINED'
         ORDER BY participant.joined_at, participant.user_id",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    let preferred_lane_rows = sqlx::query_as::<_, RecruitmentLaneRow>(
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
    let champion_rows = sqlx::query_as::<_, EntryDraftChampionRow>(
        "SELECT preference.user_id, preference.champion_id
         FROM user_champion_preferences AS preference
         JOIN recruitment_participants AS participant
           ON participant.user_id = preference.user_id
          AND participant.guild_id = $1
          AND participant.recruitment_id = $2
          AND participant.status = 'JOINED'
         WHERE preference.user_id IN (
             SELECT joined.user_id
             FROM recruitment_participants AS joined
             WHERE joined.guild_id = $1
               AND joined.recruitment_id = $2
               AND joined.status = 'JOINED'
         )
         ORDER BY preference.user_id,
                  CASE preference.lane
                      WHEN 'TOP' THEN 1
                      WHEN 'JUNGLE' THEN 2
                      WHEN 'MID' THEN 3
                      WHEN 'BOTTOM' THEN 4
                      WHEN 'SUPPORT' THEN 5
                  END,
                  preference.position",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    let assignment_rows = sqlx::query_as::<_, EntryDraftAssignmentRow>(
        "SELECT assignment.user_id AS participant_id, assignment.team, assignment.lane
         FROM recruitment_entry_assignments AS assignment
         JOIN recruitment_participants AS participant
           ON participant.guild_id = assignment.guild_id
          AND participant.recruitment_id = assignment.recruitment_id
          AND participant.user_id = assignment.user_id
          AND participant.status = 'JOINED'
         WHERE assignment.guild_id = $1 AND assignment.recruitment_id = $2
         ORDER BY assignment.team,
                  CASE assignment.lane
                      WHEN 'TOP' THEN 1
                      WHEN 'JUNGLE' THEN 2
                      WHEN 'MID' THEN 3
                      WHEN 'BOTTOM' THEN 4
                      WHEN 'SUPPORT' THEN 5
                  END",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;

    let mut lanes_by_user: HashMap<UserId, Vec<Lane>> = HashMap::new();
    for row in preferred_lane_rows {
        lanes_by_user.entry(row.user_id).or_default().push(row.lane);
    }
    let mut champions_by_user: HashMap<UserId, Vec<String>> = HashMap::new();
    for row in champion_rows {
        champions_by_user
            .entry(row.user_id)
            .or_default()
            .push(row.champion_id);
    }
    let participants = participant_rows
        .into_iter()
        .map(|row| EntryDraftParticipantSnapshot {
            preferred_lanes: lanes_by_user.remove(&row.id).unwrap_or_default(),
            champion_ids: champions_by_user.remove(&row.id).unwrap_or_default(),
            id: row.id,
            display_name: row.display_name,
            main_lane: row.main_lane,
            mmr: row.mmr,
            games: row.games,
            wins: row.wins,
            riot_tier: compact_riot_tier(
                row.solo_rank_tier.as_deref(),
                row.solo_rank_division.as_deref(),
            ),
            profile_icon_id: row.profile_icon_id,
        })
        .collect::<Vec<_>>();
    let is_operator = is_active_session_operator(transaction, guild_id, viewer_id).await?;
    let can_edit = recruitment.status == RecruitmentStatus::Closed && is_operator;
    let can_create_series = recruitment.status == RecruitmentStatus::Closed
        && is_operator
        && has_complete_entry_assignments(
            i64::try_from(participants.len()).map_err(|_| AttemptError::InvariantViolation)?,
            &assignment_rows,
            recruitment.target_count,
        );
    let can_cancel = matches!(
        recruitment.status,
        RecruitmentStatus::Open | RecruitmentStatus::Closed
    ) && (is_operator || recruitment.created_by == *viewer_id);

    Ok(EntryDraftSnapshot {
        recruitment: EntryDraftRecruitmentSnapshot {
            id: RecruitmentId::from_uuid(recruitment.id),
            display_number: recruitment.display_number,
            title: recruitment.title,
            status: recruitment.status,
            target_count: recruitment.target_count,
            revision: recruitment.revision,
            updated_at: recruitment.updated_at,
        },
        draft: EntryDraftStateSnapshot {
            revision: recruitment.draft_revision,
            updated_at: recruitment.draft_updated_at,
            updated_by: match (
                recruitment.draft_updated_by,
                recruitment.draft_updated_by_display_name,
            ) {
                (Some(id), Some(display_name)) => {
                    Some(EntryDraftEditorSnapshot { id, display_name })
                }
                (None, None) => None,
                _ => return Err(AttemptError::InvariantViolation),
            },
            participants,
            assignments: assignment_rows
                .into_iter()
                .map(EntryDraftAssignmentRow::into_snapshot)
                .collect::<Result<_, _>>()?,
        },
        capabilities: EntryDraftCapabilities {
            can_edit,
            can_reset: can_edit,
            can_create_series,
            can_reopen: recruitment.status == RecruitmentStatus::Closed && is_operator,
            can_cancel,
        },
    })
}

async fn series_pick_ban_snapshot(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    series_id: SeriesId,
    viewer_id: &UserId,
) -> Result<SeriesPickBanSnapshot, AttemptError> {
    let series = sqlx::query_as::<_, SeriesPickBanSeriesRow>(
        "SELECT id, display_number, title, best_of, status, revision, source_recruitment_id, updated_at
         FROM series
         WHERE guild_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(guild_id)
    .bind(series_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::SeriesNotFound)?;
    ensure_series_in_progress(series.status)?;

    let participant_rows = sqlx::query_as::<_, SeriesPickBanParticipantRow>(
        "SELECT participant.team,
                participant.user_id AS id,
                participant.lane,
                participant.mmr_at_lock AS mmr,
                member.display_name,
                solo_rank.tier AS solo_rank_tier,
                solo_rank.division AS solo_rank_division
         FROM series_participants AS participant
         JOIN guild_members AS member
           ON member.guild_id = participant.guild_id
          AND member.user_id = participant.user_id
         LEFT JOIN LATERAL (
             SELECT riot.platform, riot.puuid
             FROM riot_accounts AS riot
             WHERE riot.user_id = participant.user_id
               AND riot.is_main = true
               AND riot.origin = 'RIOT'
             ORDER BY riot.updated_at DESC
             LIMIT 1
         ) AS account ON TRUE
         LEFT JOIN LATERAL (
             SELECT rank_entry.value ->> 'tier' AS tier,
                    rank_entry.value ->> 'rank' AS division
             FROM riot_records_snapshots AS records
             CROSS JOIN LATERAL jsonb_array_elements(
                 CASE
                     WHEN jsonb_typeof(records.snapshot -> 'ranks') = 'array'
                         THEN records.snapshot -> 'ranks'
                     ELSE '[]'::jsonb
                 END
             ) AS rank_entry(value)
             WHERE records.platform = account.platform
               AND records.puuid = account.puuid
               AND rank_entry.value ->> 'queueType' = 'RANKED_SOLO_5x5'
             LIMIT 1
         ) AS solo_rank ON TRUE
         WHERE participant.guild_id = $1 AND participant.series_id = $2
         ORDER BY CASE participant.team
                      WHEN 'TEAM_1' THEN 1
                      WHEN 'TEAM_2' THEN 2
                  END,
                  CASE participant.lane
                      WHEN 'TOP' THEN 1
                      WHEN 'JUNGLE' THEN 2
                      WHEN 'MID' THEN 3
                      WHEN 'BOTTOM' THEN 4
                      WHEN 'SUPPORT' THEN 5
                  END",
    )
    .bind(guild_id)
    .bind(series_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    let champion_rows = sqlx::query_as::<_, SeriesPickBanChampionRow>(
        "SELECT preference.user_id, preference.champion_id
         FROM user_champion_preferences AS preference
         JOIN series_participants AS participant
           ON participant.user_id = preference.user_id
          AND participant.guild_id = $1
          AND participant.series_id = $2
          AND participant.lane = preference.lane
         ORDER BY preference.user_id,
                  preference.position",
    )
    .bind(guild_id)
    .bind(series_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;

    let mut champions_by_user: HashMap<UserId, Vec<String>> = HashMap::new();
    for row in champion_rows {
        champions_by_user
            .entry(row.user_id)
            .or_default()
            .push(row.champion_id);
    }
    let mut team_1 = Vec::with_capacity(5);
    let mut team_2 = Vec::with_capacity(5);
    for row in participant_rows {
        let player = SeriesPickBanPlayerSnapshot {
            preferred_champion_ids: champions_by_user.remove(&row.id).unwrap_or_default(),
            id: row.id,
            display_name: row.display_name,
            lane: row.lane,
            mmr: row.mmr.ok_or(AttemptError::InvariantViolation)?,
            riot_tier: compact_riot_tier(
                row.solo_rank_tier.as_deref(),
                row.solo_rank_division.as_deref(),
            ),
        };
        match row.team.as_str() {
            "TEAM_1" => team_1.push(player),
            "TEAM_2" => team_2.push(player),
            _ => return Err(AttemptError::InvariantViolation),
        }
    }
    ensure_complete_series_roster(&team_1, &team_2)?;
    let teams = vec![
        SeriesPickBanTeamSnapshot {
            team: "TEAM_1".to_owned(),
            name: "1팀".to_owned(),
            players: team_1,
        },
        SeriesPickBanTeamSnapshot {
            team: "TEAM_2".to_owned(),
            name: "2팀".to_owned(),
            players: team_2,
        },
    ];
    let player_ids = teams
        .iter()
        .flat_map(|team| {
            team.players
                .iter()
                .map(|player| player.id.as_str().to_owned())
        })
        .collect::<BTreeSet<_>>();

    // The first pick-ban slice exposes only the active input. Completed-game ledger projection
    // remains a later endpoint, while the confirmed count still drives its lifecycle guards.
    let confirmed_game_count =
        confirmed_series_game_count(transaction, guild_id, series_id).await?;
    let next_set_number =
        i16::try_from(confirmed_game_count + 1).map_err(|_| AttemptError::InvariantViolation)?;
    if next_set_number > series.best_of {
        return Err(AttemptError::InvariantViolation);
    }
    let draft = sqlx::query_as::<_, SeriesPickBanGameInputRow>(
        "SELECT id, set_number, side_preset, winning_side, patch_version
         FROM game_inputs
         WHERE guild_id = $1
           AND series_id = $2
           AND set_number = $3
           AND status = 'DRAFT'",
    )
    .bind(guild_id)
    .bind(series_id.as_uuid())
    .bind(next_set_number)
    .fetch_optional(&mut **transaction)
    .await?;
    let current_game = match draft {
        Some(draft) => {
            series_pick_ban_game_snapshot(transaction, guild_id, &draft, &player_ids).await?
        }
        None => empty_series_pick_ban_game(next_set_number, &player_ids),
    };
    let can_change_lifecycle = is_active_session_operator(transaction, guild_id, viewer_id).await?
        && confirmed_game_count == 0;

    Ok(SeriesPickBanSnapshot {
        series: SeriesPickBanSeriesSnapshot {
            id: SeriesId::from_uuid(series.id),
            display_number: series.display_number,
            title: series.title,
            best_of: series.best_of,
            status: series.status,
            revision: series.revision,
            source_recruitment_id: RecruitmentId::from_uuid(series.source_recruitment_id),
            updated_at: series.updated_at,
        },
        teams,
        current_game,
        games: Vec::new(),
        capabilities: SeriesPickBanCapabilities {
            can_revert: can_change_lifecycle,
            can_delete: can_change_lifecycle,
        },
    })
}

async fn series_pick_ban_game_snapshot(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    input: &SeriesPickBanGameInputRow,
    player_ids: &BTreeSet<String>,
) -> Result<SeriesPickBanGameSnapshot, AttemptError> {
    let mut snapshot = empty_series_pick_ban_game(input.set_number, player_ids);
    snapshot.side_preset = canonical_side_preset(&input.side_preset)?;
    snapshot.winner = input
        .winning_side
        .as_deref()
        .map(canonical_blue_red_side)
        .transpose()?;
    snapshot.patch_version = input.patch_version.clone();

    let picks = sqlx::query_as::<_, SeriesPickBanPickRow>(
        "SELECT user_id, champion_id
         FROM game_input_picks
         WHERE guild_id = $1 AND game_input_id = $2",
    )
    .bind(guild_id)
    .bind(input.id)
    .fetch_all(&mut **transaction)
    .await?;
    for pick in picks {
        if !player_ids.contains(pick.user_id.as_str()) {
            return Err(AttemptError::InvariantViolation);
        }
        snapshot
            .picks
            .insert(pick.user_id.to_string(), Some(pick.champion_id));
    }
    let bans = sqlx::query_as::<_, SeriesPickBanBanRow>(
        "SELECT side, slot, champion_id
         FROM game_input_bans
         WHERE guild_id = $1 AND game_input_id = $2",
    )
    .bind(guild_id)
    .bind(input.id)
    .fetch_all(&mut **transaction)
    .await?;
    for ban in bans {
        let index = usize::try_from(
            ban.slot
                .checked_sub(1)
                .ok_or(AttemptError::InvariantViolation)?,
        )
        .map_err(|_| AttemptError::InvariantViolation)?;
        let slots = match ban.side.as_str() {
            "BLUE" => &mut snapshot.bans.blue,
            "RED" => &mut snapshot.bans.red,
            _ => return Err(AttemptError::InvariantViolation),
        };
        let slot = slots
            .get_mut(index)
            .ok_or(AttemptError::InvariantViolation)?;
        if slot.replace(ban.champion_id).is_some() {
            return Err(AttemptError::InvariantViolation);
        }
    }
    Ok(snapshot)
}

fn empty_series_pick_ban_game(
    set_number: i16,
    player_ids: &BTreeSet<String>,
) -> SeriesPickBanGameSnapshot {
    SeriesPickBanGameSnapshot {
        set_number,
        side_preset: "DEFAULT".to_owned(),
        winner: None,
        picks: player_ids
            .iter()
            .map(|id| (id.clone(), None))
            .collect::<BTreeMap<_, _>>(),
        bans: SeriesPickBanBansSnapshot {
            blue: vec![None; 5],
            red: vec![None; 5],
        },
        patch_version: None,
    }
}

fn ensure_complete_series_roster(
    team_1: &[SeriesPickBanPlayerSnapshot],
    team_2: &[SeriesPickBanPlayerSnapshot],
) -> Result<(), AttemptError> {
    for team in [team_1, team_2] {
        if team.len() != 5 {
            return Err(AttemptError::InvariantViolation);
        }
        let lanes = team
            .iter()
            .map(|player| player.lane)
            .collect::<HashSet<_>>();
        if lanes.len() != 5 {
            return Err(AttemptError::InvariantViolation);
        }
    }
    Ok(())
}

fn canonical_side_preset(value: &str) -> Result<String, AttemptError> {
    match value {
        "DEFAULT" | "SWAPPED" => Ok(value.to_owned()),
        _ => Err(AttemptError::InvariantViolation),
    }
}

fn canonical_blue_red_side(value: &str) -> Result<String, AttemptError> {
    match value {
        "BLUE" | "RED" => Ok(value.to_owned()),
        _ => Err(AttemptError::InvariantViolation),
    }
}

async fn recruitment_snapshot(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
) -> Result<RecruitmentSnapshot, AttemptError> {
    let recruitment = sqlx::query_as::<_, RecruitmentProjectionRow>(
        "SELECT id, display_number, title, status, target_count, revision,
                channel_id, message_id, created_by, updated_at
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

async fn require_active_member_for_creation(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    user_id: &UserId,
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
    .await?;
    if status == Some(GuildMemberStatus::Active) {
        Ok(())
    } else {
        Err(AttemptError::MembershipRequired)
    }
}

async fn require_active_recruitment_member(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    user_id: &UserId,
) -> Result<(), AttemptError> {
    match sqlx::query_scalar::<_, GuildMemberStatus>(
        "SELECT status
         FROM guild_members
         WHERE guild_id = $1 AND user_id = $2
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(user_id)
    .fetch_optional(&mut **transaction)
    .await?
    {
        Some(GuildMemberStatus::Active) => Ok(()),
        Some(GuildMemberStatus::Inactive) => Err(AttemptError::RecruitmentMemberInactive),
        None => Err(AttemptError::RecruitmentMemberNotFound),
    }
}

async fn require_recruitment_channel(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
) -> Result<DiscordChannelId, AttemptError> {
    sqlx::query_scalar::<_, Option<DiscordChannelId>>(
        "SELECT recruitment_channel_id
         FROM guild_config
         WHERE guild_id = $1
         FOR UPDATE",
    )
    .bind(guild_id)
    .fetch_one(&mut **transaction)
    .await?
    .ok_or(AttemptError::RecruitmentChannelNotConfigured)
}

async fn enqueue_web_recruitment_message_notification(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    snapshot: &RecruitmentSnapshot,
    command: &WebRecruitmentMutation,
    notification_id: NotificationId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let (Some(channel_id), Some(_message_id)) = (
        snapshot.recruitment.channel_id.as_ref(),
        snapshot.recruitment.message_id.as_ref(),
    ) else {
        return Ok(());
    };
    let (notification_type, deduplication_key) = match command {
        WebRecruitmentMutation::AddMember { .. }
        | WebRecruitmentMutation::RemoveMember { .. }
        | WebRecruitmentMutation::Close { .. } => (
            RECRUITMENT_MESSAGE_UPDATE_TYPE,
            format!(
                "recruitment:{}:message:update:{}",
                snapshot.recruitment.id, snapshot.recruitment.revision
            ),
        ),
        WebRecruitmentMutation::Cancel { .. } => (
            RECRUITMENT_MESSAGE_DELETE_TYPE,
            format!("recruitment:{}:message:delete", snapshot.recruitment.id),
        ),
    };
    enqueue_recruitment_message_notification(
        transaction,
        guild_id,
        channel_id,
        snapshot,
        notification_type,
        deduplication_key,
        notification_id,
        now,
    )
    .await
}

async fn enqueue_recruitment_message_update(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    snapshot: &RecruitmentSnapshot,
    notification_id: NotificationId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let (Some(channel_id), Some(_message_id)) = (
        snapshot.recruitment.channel_id.as_ref(),
        snapshot.recruitment.message_id.as_ref(),
    ) else {
        return Ok(());
    };
    enqueue_recruitment_message_notification(
        transaction,
        guild_id,
        channel_id,
        snapshot,
        RECRUITMENT_MESSAGE_UPDATE_TYPE,
        format!(
            "recruitment:{}:message:update:{}",
            snapshot.recruitment.id, snapshot.recruitment.revision
        ),
        notification_id,
        now,
    )
    .await
}

async fn enqueue_recruitment_message_notification(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    channel_id: &DiscordChannelId,
    snapshot: &RecruitmentSnapshot,
    notification_type: &'static str,
    deduplication_key: String,
    notification_id: NotificationId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
    let payload =
        serde_json::to_value(notification_outbox::RecruitmentMessageNotificationPayload {
            schema_version: RECRUITMENT_MESSAGE_SCHEMA_VERSION,
            recruitment_id: snapshot.recruitment.id,
            snapshot: serde_json::to_value(snapshot)?,
        })?;
    notification_outbox::enqueue(
        transaction,
        &EnqueueNotification {
            id: notification_id,
            guild_id,
            deduplication_key: &deduplication_key,
            channel_id,
            notification_type,
            payload: &payload,
            created_at: now,
        },
    )
    .await?;
    Ok(())
}

async fn require_active_session_operator(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &UserId,
    operator_role_id: &DiscordRoleId,
) -> Result<(), AttemptError> {
    require_active_member_for_creation(transaction, guild_id, actor_id).await?;
    require_session_operator_role(transaction, guild_id, actor_id, operator_role_id).await
}

async fn require_active_creator_or_operator(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &UserId,
    created_by: &UserId,
    operator_role_id: &DiscordRoleId,
) -> Result<(), AttemptError> {
    require_active_member_for_creation(transaction, guild_id, actor_id).await?;
    if actor_id == created_by {
        return Ok(());
    }
    require_session_operator_role(transaction, guild_id, actor_id, operator_role_id).await
}

async fn require_session_operator_role(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &UserId,
    operator_role_id: &DiscordRoleId,
) -> Result<(), AttemptError> {
    let has_role = sqlx::query_scalar::<_, bool>(
        "SELECT true
         FROM guild_member_discord_roles
         WHERE guild_id = $1 AND user_id = $2 AND role_id = $3
         FOR SHARE",
    )
    .bind(guild_id)
    .bind(actor_id)
    .bind(operator_role_id)
    .fetch_optional(&mut **transaction)
    .await?
    .is_some();
    if has_role {
        Ok(())
    } else {
        Err(AttemptError::OperatorRequired)
    }
}

async fn is_active_session_operator(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    user_id: &UserId,
) -> Result<bool, AttemptError> {
    Ok(sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1
             FROM guild_members AS member
             JOIN guild_config AS config
               ON config.guild_id = member.guild_id
             JOIN guild_member_discord_roles AS role
               ON role.guild_id = member.guild_id
              AND role.user_id = member.user_id
              AND role.role_id = config.operator_role_id
             WHERE member.guild_id = $1
               AND member.user_id = $2
               AND member.status = 'ACTIVE'
         )",
    )
    .bind(guild_id)
    .bind(user_id)
    .fetch_one(&mut **transaction)
    .await?)
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
        "SELECT season_id, status, target_count, revision, created_by
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

async fn resolve_series_source_recruitment(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    series_id: SeriesId,
) -> Result<RecruitmentId, AttemptError> {
    sqlx::query_scalar::<_, Uuid>(
        "SELECT source_recruitment_id
         FROM series
         WHERE guild_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(guild_id)
    .bind(series_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .map(RecruitmentId::from_uuid)
    .ok_or(AttemptError::SeriesNotFound)
}

async fn lock_series(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    series_id: SeriesId,
) -> Result<SeriesLockRow, AttemptError> {
    sqlx::query_as::<_, SeriesLockRow>(
        "SELECT source_recruitment_id, status, revision
         FROM series
         WHERE guild_id = $1 AND id = $2 AND deleted_at IS NULL
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(series_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::SeriesNotFound)
}

async fn confirmed_series_game_count(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    series_id: SeriesId,
) -> Result<i64, AttemptError> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT count(*)
         FROM games
         WHERE guild_id = $1 AND series_id = $2 AND status = 'CONFIRMED'",
    )
    .bind(guild_id)
    .bind(series_id.as_uuid())
    .fetch_one(&mut **transaction)
    .await?)
}

async fn lock_entry_draft(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
) -> Result<(), AttemptError> {
    let _ = lock_entry_draft_revision(transaction, guild_id, recruitment_id).await?;
    Ok(())
}

async fn lock_entry_draft_revision(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
) -> Result<i64, AttemptError> {
    sqlx::query_scalar::<_, i64>(
        "SELECT revision
         FROM recruitment_entry_drafts
         WHERE guild_id = $1 AND recruitment_id = $2
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::MissingEntryDraft)
}

async fn close_recruitment_for_entry(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    actor_id: &UserId,
    now: OffsetDateTime,
) -> Result<(RecruitmentSnapshot, RecruitmentSnapshot), AttemptError> {
    let locked = lock_recruitment(transaction, guild_id, recruitment_id).await?;
    ensure_open(locked.status)?;
    let before = recruitment_snapshot(transaction, guild_id, recruitment_id).await?;
    let joined_count = joined_participant_count(transaction, guild_id, recruitment_id).await?;
    if joined_count != i64::from(locked.target_count) {
        return Err(AttemptError::CloseRequiresFullRoster {
            joined: joined_count,
            target: locked.target_count,
        });
    }

    let closed = sqlx::query(
        "UPDATE recruitments
         SET status = 'CLOSED',
             revision = revision + 1,
             updated_by = $1,
             updated_at = $2,
             closed_at = $2
         WHERE guild_id = $3 AND id = $4",
    )
    .bind(actor_id)
    .bind(now)
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .execute(&mut **transaction)
    .await?;
    if closed.rows_affected() != 1 {
        return Err(AttemptError::InvariantViolation);
    }

    let snapshot = recruitment_snapshot(transaction, guild_id, recruitment_id).await?;
    Ok((before, snapshot))
}

async fn locked_full_entry_assignments(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    target_count: i16,
) -> Result<Vec<SeriesEntryAssignment>, AttemptError> {
    let joined_count = joined_participant_count(transaction, guild_id, recruitment_id).await?;
    let assignments = sqlx::query_as::<_, EntryDraftAssignmentRow>(
        "SELECT assignment.user_id AS participant_id, assignment.team, assignment.lane
         FROM recruitment_entry_assignments AS assignment
         JOIN recruitment_participants AS participant
           ON participant.guild_id = assignment.guild_id
          AND participant.recruitment_id = assignment.recruitment_id
          AND participant.user_id = assignment.user_id
          AND participant.status = 'JOINED'
         WHERE assignment.guild_id = $1 AND assignment.recruitment_id = $2
         FOR UPDATE OF assignment",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await?;
    if !has_complete_entry_assignments(joined_count, &assignments, target_count) {
        return Err(AttemptError::EntryDraftIncomplete {
            joined: joined_count,
            assigned: i64::try_from(assignments.len())
                .map_err(|_| AttemptError::InvariantViolation)?,
            target: target_count,
        });
    }

    Ok(assignments
        .into_iter()
        .map(|assignment| SeriesEntryAssignment {
            user_id: assignment.participant_id,
            team: assignment.team,
            lane: assignment.lane,
        })
        .collect())
}

fn has_complete_entry_assignments(
    joined_count: i64,
    assignments: &[EntryDraftAssignmentRow],
    target_count: i16,
) -> bool {
    let expected_count = usize::try_from(target_count).ok();
    if target_count != RECRUITMENT_TARGET_COUNT
        || joined_count != i64::from(target_count)
        || expected_count != Some(assignments.len())
    {
        return false;
    }

    let participant_ids = assignments
        .iter()
        .map(|assignment| assignment.participant_id.clone())
        .collect::<HashSet<_>>();
    if participant_ids.len() != assignments.len() {
        return false;
    }

    let slots = assignments
        .iter()
        .map(|assignment| (assignment.team.as_str(), assignment.lane))
        .collect::<HashSet<_>>();
    let required_slots = [
        ("TEAM_1", Lane::Top),
        ("TEAM_1", Lane::Jungle),
        ("TEAM_1", Lane::Mid),
        ("TEAM_1", Lane::Bottom),
        ("TEAM_1", Lane::Support),
        ("TEAM_2", Lane::Top),
        ("TEAM_2", Lane::Jungle),
        ("TEAM_2", Lane::Mid),
        ("TEAM_2", Lane::Bottom),
        ("TEAM_2", Lane::Support),
    ]
    .into_iter()
    .collect::<HashSet<_>>();

    slots == required_slots
}

async fn apply_entry_draft_operation(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    operation: &EntryDraftOperation,
) -> Result<(), AttemptError> {
    match operation {
        EntryDraftOperation::MoveParticipant {
            participant_id,
            slot,
        } => {
            let is_joined = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS (
                     SELECT 1
                     FROM recruitment_participants
                     WHERE guild_id = $1
                       AND recruitment_id = $2
                       AND user_id = $3
                       AND status = 'JOINED'
                 )",
            )
            .bind(guild_id)
            .bind(recruitment_id.as_uuid())
            .bind(participant_id)
            .fetch_one(&mut **transaction)
            .await?;
            if !is_joined {
                return Err(AttemptError::ParticipantNotJoined);
            }

            let previous = sqlx::query_as::<_, EntryDraftAssignmentRow>(
                "SELECT user_id AS participant_id, team, lane
                 FROM recruitment_entry_assignments
                 WHERE guild_id = $1 AND recruitment_id = $2 AND user_id = $3
                 FOR UPDATE",
            )
            .bind(guild_id)
            .bind(recruitment_id.as_uuid())
            .bind(participant_id)
            .fetch_optional(&mut **transaction)
            .await?;

            let Some((team, lane)) = slot.as_deref().map(parse_entry_slot).transpose()? else {
                sqlx::query(
                    "DELETE FROM recruitment_entry_assignments
                     WHERE guild_id = $1 AND recruitment_id = $2 AND user_id = $3",
                )
                .bind(guild_id)
                .bind(recruitment_id.as_uuid())
                .bind(participant_id)
                .execute(&mut **transaction)
                .await?;
                return Ok(());
            };

            let occupant = sqlx::query_as::<_, EntryDraftAssignmentRow>(
                "SELECT user_id AS participant_id, team, lane
                 FROM recruitment_entry_assignments
                 WHERE guild_id = $1
                   AND recruitment_id = $2
                   AND team = $3
                   AND lane = $4
                 FOR UPDATE",
            )
            .bind(guild_id)
            .bind(recruitment_id.as_uuid())
            .bind(team)
            .bind(lane)
            .fetch_optional(&mut **transaction)
            .await?;

            sqlx::query(
                "DELETE FROM recruitment_entry_assignments
                 WHERE guild_id = $1 AND recruitment_id = $2 AND user_id = $3",
            )
            .bind(guild_id)
            .bind(recruitment_id.as_uuid())
            .bind(participant_id)
            .execute(&mut **transaction)
            .await?;

            if let Some(occupant) =
                occupant.filter(|occupant| occupant.participant_id != *participant_id)
            {
                if let Some(previous) = previous.as_ref() {
                    sqlx::query(
                        "UPDATE recruitment_entry_assignments
                         SET team = $1, lane = $2
                         WHERE guild_id = $3 AND recruitment_id = $4 AND user_id = $5",
                    )
                    .bind(&previous.team)
                    .bind(previous.lane)
                    .bind(guild_id)
                    .bind(recruitment_id.as_uuid())
                    .bind(&occupant.participant_id)
                    .execute(&mut **transaction)
                    .await?;
                } else {
                    sqlx::query(
                        "DELETE FROM recruitment_entry_assignments
                         WHERE guild_id = $1 AND recruitment_id = $2 AND user_id = $3",
                    )
                    .bind(guild_id)
                    .bind(recruitment_id.as_uuid())
                    .bind(&occupant.participant_id)
                    .execute(&mut **transaction)
                    .await?;
                }
            }

            sqlx::query(
                "INSERT INTO recruitment_entry_assignments (
                     guild_id, recruitment_id, user_id, team, lane
                 ) VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(guild_id)
            .bind(recruitment_id.as_uuid())
            .bind(participant_id)
            .bind(team)
            .bind(lane)
            .execute(&mut **transaction)
            .await?;
            Ok(())
        }
    }
}

fn parse_entry_slot(slot: &str) -> Result<(&'static str, Lane), AttemptError> {
    for (prefix, team) in [("TEAM_1_", "TEAM_1"), ("TEAM_2_", "TEAM_2")] {
        if let Some(lane) = slot.strip_prefix(prefix) {
            return lane
                .parse::<Lane>()
                .map(|lane| (team, lane))
                .map_err(|_| AttemptError::InvalidEntrySlot);
        }
    }
    Err(AttemptError::InvalidEntrySlot)
}

async fn bump_entry_draft(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    actor_id: &UserId,
    now: OffsetDateTime,
) -> Result<(), AttemptError> {
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
    let recruitment = sqlx::query(
        "UPDATE recruitments
         SET updated_by = $1, updated_at = $2
         WHERE guild_id = $3 AND id = $4",
    )
    .bind(actor_id)
    .bind(now)
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .execute(&mut **transaction)
    .await?;
    if draft.rows_affected() != 1 || recruitment.rows_affected() != 1 {
        return Err(AttemptError::MissingEntryDraft);
    }
    Ok(())
}

async fn bump_recruitment_and_draft(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    actor_id: &UserId,
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

async fn next_series_display_number(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
) -> Result<i64, AttemptError> {
    sqlx::query_scalar::<_, i64>(
        "UPDATE guild_counters
         SET next_value = next_value + 1
         WHERE guild_id = $1 AND counter_key = 'SERIES'
         RETURNING next_value - 1",
    )
    .bind(guild_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AttemptError::MissingSeriesCounter)
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
        principal_type: request.principal_type,
        principal_id: request.principal_id,
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

fn require_creator_or_operator(
    actor: &BotActor,
    created_by: &UserId,
    operator_role_id: &DiscordRoleId,
) -> Result<(), AttemptError> {
    if UserId::from(&actor.id) == *created_by {
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

fn ensure_series_source_ready(status: RecruitmentStatus) -> Result<(), AttemptError> {
    if status == RecruitmentStatus::Closed {
        Ok(())
    } else {
        Err(AttemptError::SeriesSourceNotReady(status))
    }
}

fn ensure_series_source_converted(status: RecruitmentStatus) -> Result<(), AttemptError> {
    if status == RecruitmentStatus::Converted {
        Ok(())
    } else {
        Err(AttemptError::SeriesSourceNotConverted(status))
    }
}

fn ensure_series_in_progress(status: SeriesStatus) -> Result<(), AttemptError> {
    if status == SeriesStatus::InProgress {
        Ok(())
    } else {
        Err(AttemptError::SeriesNotInProgress(status))
    }
}

fn ensure_cancellable(status: RecruitmentStatus) -> Result<(), AttemptError> {
    if matches!(status, RecruitmentStatus::Open | RecruitmentStatus::Closed) {
        Ok(())
    } else {
        Err(AttemptError::RecruitmentNotOpen(status))
    }
}

fn ensure_entry_editable(status: RecruitmentStatus) -> Result<(), AttemptError> {
    if status == RecruitmentStatus::Closed {
        Ok(())
    } else {
        Err(AttemptError::EntryDraftNotEditable(status))
    }
}

fn ensure_reopenable(status: RecruitmentStatus) -> Result<(), AttemptError> {
    if status == RecruitmentStatus::Closed {
        Ok(())
    } else {
        Err(AttemptError::EntryDraftNotEditable(status))
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
        AttemptError::Mmr(MmrRepositoryError::Sql(error)) => Some(error),
        AttemptError::Idempotency(IdempotencyRepositoryError::Sql(error)) => Some(error),
        AttemptError::Notification(NotificationOutboxError::Sql(error)) => Some(error),
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
    principal_type: IdempotencyPrincipalType,
    principal_id: &'a str,
    method: &'a str,
    route_template: &'a str,
    key: &'a str,
    request_hash: &'a [u8],
    now: OffsetDateTime,
}

struct AuditInsert<'a> {
    id: Uuid,
    actor_id: &'a str,
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
    notification_id: NotificationId,
}

impl RecruitmentPrepared {
    fn new() -> Self {
        Self {
            now: OffsetDateTime::now_utc(),
            idempotency_record_id: IdempotencyRecordId::new(),
            recruitment_id: RecruitmentId::new(),
            audit_id: Uuid::now_v7(),
            outbox_event_id: OutboxEventId::new(),
            notification_id: NotificationId::new(),
        }
    }
}

#[derive(Clone, Copy)]
struct WebRecruitmentMutationPrepared {
    now: OffsetDateTime,
    idempotency_record_id: IdempotencyRecordId,
    audit_id: Uuid,
    outbox_event_id: OutboxEventId,
    notification_id: NotificationId,
}

impl WebRecruitmentMutationPrepared {
    fn new() -> Self {
        Self {
            now: OffsetDateTime::now_utc(),
            idempotency_record_id: IdempotencyRecordId::new(),
            audit_id: Uuid::now_v7(),
            outbox_event_id: OutboxEventId::new(),
            notification_id: NotificationId::new(),
        }
    }
}

#[derive(Clone, Copy)]
struct WebEntryDraftMutationPrepared {
    now: OffsetDateTime,
    idempotency_record_id: IdempotencyRecordId,
    audit_id: Uuid,
    outbox_event_id: OutboxEventId,
    recruitment_outbox_event_id: OutboxEventId,
    notification_id: NotificationId,
}

impl WebEntryDraftMutationPrepared {
    fn new() -> Self {
        Self {
            now: OffsetDateTime::now_utc(),
            idempotency_record_id: IdempotencyRecordId::new(),
            audit_id: Uuid::now_v7(),
            outbox_event_id: OutboxEventId::new(),
            recruitment_outbox_event_id: OutboxEventId::new(),
            notification_id: NotificationId::new(),
        }
    }
}

#[derive(Clone, Copy)]
struct WebSeriesCreatePrepared {
    now: OffsetDateTime,
    idempotency_record_id: IdempotencyRecordId,
    series_id: SeriesId,
    audit_id: Uuid,
    recruitment_outbox_event_id: OutboxEventId,
    series_outbox_event_id: OutboxEventId,
}

impl WebSeriesCreatePrepared {
    fn new() -> Self {
        Self {
            now: OffsetDateTime::now_utc(),
            idempotency_record_id: IdempotencyRecordId::new(),
            series_id: SeriesId::new(),
            audit_id: Uuid::now_v7(),
            recruitment_outbox_event_id: OutboxEventId::new(),
            series_outbox_event_id: OutboxEventId::new(),
        }
    }
}

#[derive(Clone, Copy)]
struct WebSeriesLifecyclePrepared {
    now: OffsetDateTime,
    idempotency_record_id: IdempotencyRecordId,
    audit_id: Uuid,
    recruitment_outbox_event_id: OutboxEventId,
    series_outbox_event_id: OutboxEventId,
}

impl WebSeriesLifecyclePrepared {
    fn new() -> Self {
        Self {
            now: OffsetDateTime::now_utc(),
            idempotency_record_id: IdempotencyRecordId::new(),
            audit_id: Uuid::now_v7(),
            recruitment_outbox_event_id: OutboxEventId::new(),
            series_outbox_event_id: OutboxEventId::new(),
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

#[derive(Clone)]
enum RecruitmentCreateActor {
    Bot(BotActor),
    Session(UserId),
}

impl RecruitmentCreateActor {
    fn user_id(&self) -> UserId {
        match self {
            Self::Bot(actor) => UserId::from(&actor.id),
            Self::Session(user_id) => user_id.clone(),
        }
    }
}

#[derive(Serialize)]
struct WebRecruitmentCreateRequest {}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WebSeriesCreateCommand {
    source_recruitment_id: RecruitmentId,
    expected_revision: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum SeriesLifecycleAction {
    Revert,
    Delete,
}

impl SeriesLifecycleAction {
    fn metadata(self) -> SeriesLifecycleMetadata {
        match self {
            Self::Revert => SeriesLifecycleMetadata {
                method: SERIES_REVERT_METHOD,
                route_template: SERIES_REVERT_ROUTE,
                audit_action: "SERIES_REVERTED",
                series_event_type: "SERIES_REVERTED",
                recruitment_event_type: "RECRUITMENT_RESTORED_FROM_SERIES",
                source_status: "CLOSED",
            },
            Self::Delete => SeriesLifecycleMetadata {
                method: SERIES_DELETE_METHOD,
                route_template: SERIES_DELETE_ROUTE,
                audit_action: "SERIES_CANCELLED",
                series_event_type: "SERIES_CANCELLED",
                recruitment_event_type: "RECRUITMENT_CANCELLED_FROM_SERIES",
                source_status: "CANCELLED",
            },
        }
    }
}

#[derive(Clone, Copy)]
struct SeriesLifecycleMetadata {
    method: &'static str,
    route_template: &'static str,
    audit_action: &'static str,
    series_event_type: &'static str,
    recruitment_event_type: &'static str,
    source_status: &'static str,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WebSeriesLifecycleCommand {
    series_id: SeriesId,
    expected_revision: i64,
    action: SeriesLifecycleAction,
}

#[derive(Clone, Serialize)]
enum WebRecruitmentMutation {
    AddMember {
        recruitment_id: RecruitmentId,
        member_id: UserId,
    },
    RemoveMember {
        recruitment_id: RecruitmentId,
        member_id: UserId,
    },
    Close {
        recruitment_id: RecruitmentId,
    },
    Cancel {
        recruitment_id: RecruitmentId,
    },
}

impl WebRecruitmentMutation {
    fn metadata(&self) -> WebRecruitmentMutationMetadata {
        match self {
            Self::AddMember { .. } => WebRecruitmentMutationMetadata {
                method: "POST",
                route_template: "/api/guilds/{guildId}/recruitments/{id}/members",
                audit_action: "RECRUITMENT_MEMBER_ADDED",
                event_type: "RECRUITMENT_MEMBER_ADDED",
            },
            Self::RemoveMember { .. } => WebRecruitmentMutationMetadata {
                method: "DELETE",
                route_template: "/api/guilds/{guildId}/recruitments/{id}/members/{memberId}",
                audit_action: "RECRUITMENT_MEMBER_REMOVED",
                event_type: "RECRUITMENT_MEMBER_REMOVED",
            },
            Self::Close { .. } => WebRecruitmentMutationMetadata {
                method: "POST",
                route_template: "/api/guilds/{guildId}/recruitments/{id}/close",
                audit_action: "RECRUITMENT_CLOSE",
                event_type: "RECRUITMENT_CLOSED",
            },
            Self::Cancel { .. } => WebRecruitmentMutationMetadata {
                method: "DELETE",
                route_template: "/api/guilds/{guildId}/recruitments/{id}",
                audit_action: "RECRUITMENT_CANCELLED",
                event_type: "RECRUITMENT_CANCELLED",
            },
        }
    }
}

#[derive(Clone, Serialize)]
enum WebEntryDraftMutation {
    Apply {
        recruitment_id: RecruitmentId,
        expected_revision: i64,
        operation: EntryDraftOperation,
    },
    Reset {
        recruitment_id: RecruitmentId,
        expected_revision: i64,
    },
    Reopen {
        recruitment_id: RecruitmentId,
        expected_revision: i64,
    },
}

impl WebEntryDraftMutation {
    fn recruitment_id(&self) -> RecruitmentId {
        match self {
            Self::Apply { recruitment_id, .. }
            | Self::Reset { recruitment_id, .. }
            | Self::Reopen { recruitment_id, .. } => *recruitment_id,
        }
    }

    fn expected_revision(&self) -> i64 {
        match self {
            Self::Apply {
                expected_revision, ..
            }
            | Self::Reset {
                expected_revision, ..
            }
            | Self::Reopen {
                expected_revision, ..
            } => *expected_revision,
        }
    }

    fn metadata(&self) -> WebEntryDraftMutationMetadata {
        match self {
            Self::Apply { .. } => WebEntryDraftMutationMetadata {
                method: "PATCH",
                route_template: "/api/guilds/{guildId}/recruitments/{id}/entry-draft",
                audit_action: "RECRUITMENT_ENTRY_DRAFT_UPDATED",
                event_type: "RECRUITMENT_ENTRY_DRAFT_UPDATED",
            },
            Self::Reset { .. } => WebEntryDraftMutationMetadata {
                method: "POST",
                route_template: "/api/guilds/{guildId}/recruitments/{id}/entry-draft/reset",
                audit_action: "RECRUITMENT_ENTRY_DRAFT_RESET",
                event_type: "RECRUITMENT_ENTRY_DRAFT_RESET",
            },
            Self::Reopen { .. } => WebEntryDraftMutationMetadata {
                method: "POST",
                route_template: "/api/guilds/{guildId}/recruitments/{id}/reopen",
                audit_action: "RECRUITMENT_REOPENED",
                event_type: "RECRUITMENT_REOPENED",
            },
        }
    }
}

#[derive(Clone, Copy)]
struct WebRecruitmentMutationMetadata {
    method: &'static str,
    route_template: &'static str,
    audit_action: &'static str,
    event_type: &'static str,
}

#[derive(Clone, Copy)]
struct WebEntryDraftMutationMetadata {
    method: &'static str,
    route_template: &'static str,
    audit_action: &'static str,
    event_type: &'static str,
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
    season_id: Uuid,
    status: RecruitmentStatus,
    target_count: i16,
    revision: i64,
    created_by: UserId,
}

#[derive(Debug, FromRow)]
struct SeriesLockRow {
    source_recruitment_id: RecruitmentId,
    status: SeriesStatus,
    revision: i64,
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
    created_by: UserId,
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
            created_by_id: self.created_by,
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

#[derive(Debug, FromRow)]
struct EntryDraftRecruitmentRow {
    id: Uuid,
    display_number: i64,
    title: String,
    status: RecruitmentStatus,
    target_count: i16,
    revision: i64,
    updated_at: OffsetDateTime,
    created_by: UserId,
    draft_revision: i64,
    draft_updated_at: OffsetDateTime,
    draft_updated_by: Option<UserId>,
    draft_updated_by_display_name: Option<String>,
}

#[derive(Debug, FromRow)]
struct EntryDraftParticipantRow {
    id: UserId,
    display_name: String,
    main_lane: Option<Lane>,
    mmr: f64,
    games: i32,
    wins: i32,
    solo_rank_tier: Option<String>,
    solo_rank_division: Option<String>,
    profile_icon_id: Option<i32>,
}

fn compact_riot_tier(tier: Option<&str>, division: Option<&str>) -> Option<String> {
    let prefix = match tier?.trim().to_ascii_uppercase().as_str() {
        "IRON" => "I",
        "BRONZE" => "B",
        "SILVER" => "S",
        "GOLD" => "G",
        "PLATINUM" => "P",
        "EMERALD" => "E",
        "DIAMOND" => "D",
        "MASTER" => "M",
        "GRANDMASTER" => "GM",
        "CHALLENGER" => "C",
        _ => return None,
    };
    let suffix = match division.map(str::trim).filter(|value| !value.is_empty()) {
        None => "",
        Some("I") => "1",
        Some("II") => "2",
        Some("III") => "3",
        Some("IV") => "4",
        Some(_) => return None,
    };

    Some(format!("{prefix}{suffix}"))
}

#[derive(Debug, FromRow)]
struct EntryDraftChampionRow {
    user_id: UserId,
    champion_id: String,
}

#[derive(Debug, FromRow)]
struct SeriesPickBanSeriesRow {
    id: Uuid,
    display_number: i64,
    title: String,
    best_of: i16,
    status: SeriesStatus,
    revision: i64,
    source_recruitment_id: Uuid,
    updated_at: OffsetDateTime,
}

#[derive(Debug, FromRow)]
struct SeriesPickBanParticipantRow {
    team: String,
    id: UserId,
    lane: Lane,
    mmr: Option<f64>,
    display_name: String,
    solo_rank_tier: Option<String>,
    solo_rank_division: Option<String>,
}

#[derive(Debug, FromRow)]
struct SeriesPickBanChampionRow {
    user_id: UserId,
    champion_id: String,
}

#[derive(Debug, FromRow)]
struct SeriesPickBanGameInputRow {
    id: Uuid,
    set_number: i16,
    side_preset: String,
    winning_side: Option<String>,
    patch_version: Option<String>,
}

#[derive(Debug, FromRow)]
struct SeriesPickBanPickRow {
    user_id: UserId,
    champion_id: String,
}

#[derive(Debug, FromRow)]
struct SeriesPickBanBanRow {
    side: String,
    slot: i16,
    champion_id: String,
}

#[derive(Debug, FromRow)]
struct EntryDraftAssignmentRow {
    participant_id: UserId,
    team: String,
    lane: Lane,
}

#[derive(Debug)]
struct SeriesEntryAssignment {
    user_id: UserId,
    team: String,
    lane: Lane,
}

impl EntryDraftAssignmentRow {
    fn into_snapshot(self) -> Result<EntryDraftAssignmentSnapshot, AttemptError> {
        if !matches!(self.team.as_str(), "TEAM_1" | "TEAM_2") {
            return Err(AttemptError::InvariantViolation);
        }
        Ok(EntryDraftAssignmentSnapshot {
            participant_id: self.participant_id,
            team: self.team,
            lane: self.lane,
        })
    }
}

#[derive(Debug, Error)]
enum AttemptError {
    #[error("PostgreSQL bot workflow operation failed")]
    Sql(#[from] sqlx::Error),
    #[error("MMR repository operation failed")]
    Mmr(#[from] MmrRepositoryError),
    #[error("idempotency repository operation failed")]
    Idempotency(#[from] IdempotencyRepositoryError),
    #[error("notification outbox operation failed")]
    Notification(#[from] NotificationOutboxError),
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
    #[error("the caller is not an active member of this guild")]
    MembershipRequired,
    #[error("the recruitment member was not found in this guild")]
    RecruitmentMemberNotFound,
    #[error("the recruitment member is inactive")]
    RecruitmentMemberInactive,
    #[error("the guild has no active season")]
    NoActiveSeason,
    #[error("the guild has no configured recruitment channel")]
    RecruitmentChannelNotConfigured,
    #[error("the guild recruitment counter is missing")]
    MissingRecruitmentCounter,
    #[error("the guild series counter is missing")]
    MissingSeriesCounter,
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
    #[error("entry draft can only be changed while recruitment is CLOSED, but is {0}")]
    EntryDraftNotEditable(RecruitmentStatus),
    #[error("entry draft revision mismatch: expected {expected}, actual {actual}")]
    EntryDraftRevisionConflict { expected: i64, actual: i64 },
    #[error("recruitment revision mismatch: expected {expected}, actual {actual}")]
    RecruitmentRevisionConflict { expected: i64, actual: i64 },
    #[error("series source recruitment must be CLOSED, but is {0}")]
    SeriesSourceNotReady(RecruitmentStatus),
    #[error("series was not found in the tenant")]
    SeriesNotFound,
    #[error("series must be IN_PROGRESS, but is {0}")]
    SeriesNotInProgress(SeriesStatus),
    #[error("series revision mismatch: expected {expected}, actual {actual}")]
    SeriesRevisionConflict { expected: i64, actual: i64 },
    #[error("series source recruitment must be CONVERTED, but is {0}")]
    SeriesSourceNotConverted(RecruitmentStatus),
    #[error("series has {count} confirmed games")]
    SeriesHasConfirmedGames { count: i64 },
    #[error("entry draft must assign all joined participants before series creation")]
    EntryDraftIncomplete {
        joined: i64,
        assigned: i64,
        target: i16,
    },
    #[error("entry draft slot is invalid")]
    InvalidEntrySlot,
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
            Self::Mmr(MmrRepositoryError::Sql(error)) => BotWorkflowError::Database(error),
            Self::Mmr(MmrRepositoryError::MissingInitializedRow) => {
                BotWorkflowError::InvariantViolation
            }
            Self::Idempotency(IdempotencyRepositoryError::Sql(error)) => {
                BotWorkflowError::Database(error)
            }
            Self::Idempotency(_) => BotWorkflowError::InvariantViolation,
            Self::Notification(NotificationOutboxError::Sql(error)) => {
                BotWorkflowError::Database(error)
            }
            Self::Notification(_) => BotWorkflowError::InvariantViolation,
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
            Self::MembershipRequired => BotWorkflowError::MembershipRequired,
            Self::RecruitmentMemberNotFound => BotWorkflowError::RecruitmentMemberNotFound,
            Self::RecruitmentMemberInactive => BotWorkflowError::RecruitmentMemberInactive,
            Self::NoActiveSeason => BotWorkflowError::NoActiveSeason,
            Self::RecruitmentChannelNotConfigured => {
                BotWorkflowError::RecruitmentChannelNotConfigured
            }
            Self::MissingRecruitmentCounter | Self::MissingSeriesCounter => {
                BotWorkflowError::InvariantViolation
            }
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
            Self::EntryDraftNotEditable(status) => BotWorkflowError::EntryDraftNotEditable(status),
            Self::EntryDraftRevisionConflict { expected, actual } => {
                BotWorkflowError::EntryDraftRevisionConflict { expected, actual }
            }
            Self::RecruitmentRevisionConflict { expected, actual } => {
                BotWorkflowError::RecruitmentRevisionConflict { expected, actual }
            }
            Self::SeriesSourceNotReady(status) => BotWorkflowError::SeriesSourceNotReady(status),
            Self::SeriesNotFound => BotWorkflowError::SeriesNotFound,
            Self::SeriesNotInProgress(status) => BotWorkflowError::SeriesNotInProgress(status),
            Self::SeriesRevisionConflict { expected, actual } => {
                BotWorkflowError::SeriesRevisionConflict { expected, actual }
            }
            Self::SeriesSourceNotConverted(status) => {
                BotWorkflowError::SeriesSourceNotConverted(status)
            }
            Self::SeriesHasConfirmedGames { count } => {
                BotWorkflowError::SeriesHasConfirmedGames { count }
            }
            Self::EntryDraftIncomplete {
                joined,
                assigned,
                target,
            } => BotWorkflowError::EntryDraftIncomplete {
                joined,
                assigned,
                target,
            },
            Self::InvalidEntrySlot => BotWorkflowError::InvalidEntrySlot,
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
    #[error("the caller is not an active member of this guild")]
    MembershipRequired,
    #[error("the recruitment member was not found in this guild")]
    RecruitmentMemberNotFound,
    #[error("the recruitment member is inactive")]
    RecruitmentMemberInactive,
    #[error("the Riot account is already linked to another user")]
    RiotAccountAlreadyLinked,
    #[error("the canonical Riot ID conflicts with another account")]
    RiotIdentityConflict,
    #[error("the Discord identity conflicts with a non-Discord user")]
    UserIdentityConflict,
    #[error("the guild has no active season")]
    NoActiveSeason,
    #[error("the guild has no configured recruitment channel")]
    RecruitmentChannelNotConfigured,
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
    #[error("entry draft can only be changed while recruitment is CLOSED, but is {0}")]
    EntryDraftNotEditable(RecruitmentStatus),
    #[error("entry draft revision mismatch: expected {expected}, actual {actual}")]
    EntryDraftRevisionConflict { expected: i64, actual: i64 },
    #[error("recruitment revision mismatch: expected {expected}, actual {actual}")]
    RecruitmentRevisionConflict { expected: i64, actual: i64 },
    #[error("series source recruitment must be CLOSED, but is {0}")]
    SeriesSourceNotReady(RecruitmentStatus),
    #[error("series was not found")]
    SeriesNotFound,
    #[error("series must be IN_PROGRESS, but is {0}")]
    SeriesNotInProgress(SeriesStatus),
    #[error("series revision mismatch: expected {expected}, actual {actual}")]
    SeriesRevisionConflict { expected: i64, actual: i64 },
    #[error("series source recruitment must be CONVERTED, but is {0}")]
    SeriesSourceNotConverted(RecruitmentStatus),
    #[error("series has {count} confirmed games")]
    SeriesHasConfirmedGames { count: i64 },
    #[error("entry draft must assign all joined participants before series creation")]
    EntryDraftIncomplete {
        joined: i64,
        assigned: i64,
        target: i16,
    },
    #[error("entry draft slot is invalid")]
    InvalidEntrySlot,
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
    fn compact_riot_tier_only_exposes_canonical_solo_rank_values() {
        assert_eq!(
            compact_riot_tier(Some("PLATINUM"), Some("III")),
            Some("P3".to_owned())
        );
        assert_eq!(
            compact_riot_tier(Some("grandmaster"), None),
            Some("GM".to_owned())
        );
        assert_eq!(
            compact_riot_tier(Some("CHALLENGER"), Some("I")),
            Some("C1".to_owned())
        );
        assert_eq!(compact_riot_tier(Some("UNRANKED"), None), None);
        assert_eq!(compact_riot_tier(Some("PLATINUM"), Some("V")), None);
        assert_eq!(compact_riot_tier(None, Some("III")), None);
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
