use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgConnection, PgPool, Postgres, QueryBuilder};
use thiserror::Error;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    domain::{DiscordChannelId, DiscordRoleId, DiscordUserId, GuildId},
    infrastructure::postgres::{
        DatabaseTransaction, DeadlockRetryPolicy, TransactionAttemptError, TransactionRetryError,
        run_with_deadlock_retry,
    },
};

const MAX_CATALOG_ROLES: usize = 250;
const MAX_CATALOG_CHANNELS: usize = 500;
const MAX_DISCORD_NAME_CHARS: usize = 100;
const MAX_DISPLAY_NAME_CHARS: usize = 100;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplaceDiscordCatalogCommand {
    pub roles: Vec<DiscordCatalogRoleCommand>,
    pub channels: Vec<DiscordCatalogChannelCommand>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiscordCatalogRoleCommand {
    pub id: DiscordRoleId,
    pub name: String,
    pub position: i32,
    pub managed: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiscordCatalogChannelCommand {
    pub id: DiscordChannelId,
    pub name: String,
    pub parent_name: Option<String>,
    pub position: i32,
    pub kind: DiscordCatalogChannelKind,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DiscordCatalogChannelKind {
    Text,
    Announcement,
}

impl DiscordCatalogChannelKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "TEXT",
            Self::Announcement => "ANNOUNCEMENT",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscordCatalogSyncResult {
    #[serde(with = "time::serde::rfc3339")]
    pub synced_at: OffsetDateTime,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateGuildSettingsCommand {
    pub display_name: String,
    pub operator_role_id: DiscordRoleId,
    pub recruitment_channel_id: Option<DiscordChannelId>,
    pub entry_channel_id: Option<DiscordChannelId>,
    pub log_channel_id: Option<DiscordChannelId>,
    pub is_externally_public: bool,
    pub expected_revision: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildSettingsSnapshot {
    pub settings: GuildSettingsValues,
    pub revision: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    pub updated_by: Option<DiscordUserId>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub catalog_updated_at: Option<OffsetDateTime>,
    pub roles: Vec<GuildSettingsRole>,
    pub channels: Vec<GuildSettingsChannel>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildSettingsValues {
    pub display_name: String,
    pub operator_role_id: DiscordRoleId,
    pub recruitment_channel_id: Option<DiscordChannelId>,
    pub entry_channel_id: Option<DiscordChannelId>,
    pub log_channel_id: Option<DiscordChannelId>,
    pub is_externally_public: bool,
}

#[derive(Clone, Debug, Eq, FromRow, PartialEq, Serialize)]
pub struct GuildSettingsRole {
    pub id: DiscordRoleId,
    pub name: String,
}

#[derive(Clone, Debug, Eq, FromRow, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildSettingsChannel {
    pub id: DiscordChannelId,
    pub name: String,
    pub parent_name: Option<String>,
}

pub async fn replace_discord_catalog(
    pool: &PgPool,
    guild_id: &GuildId,
    command: ReplaceDiscordCatalogCommand,
) -> Result<DiscordCatalogSyncResult, GuildAdminSettingsRepositoryError> {
    let command = normalize_catalog(guild_id, command)?;
    let guild_id = guild_id.clone();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let command = command.clone();
            Box::pin(async move {
                replace_discord_catalog_attempt(transaction, &guild_id, &command).await
            })
        },
        attempt_sql_error,
    )
    .await
    .map_err(map_transaction_error)
}

pub async fn get(
    pool: &PgPool,
    guild_id: &GuildId,
    actor_id: &DiscordUserId,
) -> Result<GuildSettingsSnapshot, GuildAdminSettingsRepositoryError> {
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

pub async fn update(
    pool: &PgPool,
    guild_id: &GuildId,
    actor_id: &DiscordUserId,
    request_id: &str,
    command: UpdateGuildSettingsCommand,
) -> Result<GuildSettingsSnapshot, GuildAdminSettingsRepositoryError> {
    let command = normalize_settings(guild_id, command)?;
    let guild_id = guild_id.clone();
    let actor_id = actor_id.clone();
    let request_id = request_id.to_owned();
    let audit_id = Uuid::now_v7();

    run_with_deadlock_retry(
        pool,
        DeadlockRetryPolicy::DEFAULT,
        move |transaction| {
            let guild_id = guild_id.clone();
            let actor_id = actor_id.clone();
            let request_id = request_id.clone();
            let command = command.clone();
            Box::pin(async move {
                update_attempt(
                    transaction,
                    &guild_id,
                    &actor_id,
                    &request_id,
                    audit_id,
                    &command,
                )
                .await
            })
        },
        attempt_sql_error,
    )
    .await
    .map_err(map_transaction_error)
}

async fn replace_discord_catalog_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    command: &ReplaceDiscordCatalogCommand,
) -> Result<DiscordCatalogSyncResult, AttemptError> {
    let configured = sqlx::query_scalar::<_, bool>(
        "SELECT true
         FROM guilds AS guild
         JOIN guild_config AS config ON config.guild_id = guild.guild_id
         WHERE guild.guild_id = $1 AND guild.status = 'ACTIVE'
         FOR SHARE OF guild
         FOR UPDATE OF config",
    )
    .bind(guild_id)
    .fetch_optional(&mut **transaction)
    .await?;
    if configured.is_none() {
        return Err(AttemptError::GuildNotFound);
    }

    let synced_at = sqlx::query_scalar::<_, OffsetDateTime>("SELECT clock_timestamp()")
        .fetch_one(&mut **transaction)
        .await?;
    sqlx::query("DELETE FROM guild_discord_roles WHERE guild_id = $1")
        .bind(guild_id)
        .execute(&mut **transaction)
        .await?;
    sqlx::query("DELETE FROM guild_discord_channels WHERE guild_id = $1")
        .bind(guild_id)
        .execute(&mut **transaction)
        .await?;

    if !command.roles.is_empty() {
        let mut query = QueryBuilder::<Postgres>::new(
            "INSERT INTO guild_discord_roles \
             (guild_id, role_id, name, position, managed, updated_at) ",
        );
        query.push_values(&command.roles, |mut row, role| {
            row.push_bind(guild_id)
                .push_bind(&role.id)
                .push_bind(&role.name)
                .push_bind(role.position)
                .push_bind(role.managed)
                .push_bind(synced_at);
        });
        query.build().execute(&mut **transaction).await?;
    }

    if !command.channels.is_empty() {
        let mut query = QueryBuilder::<Postgres>::new(
            "INSERT INTO guild_discord_channels \
             (guild_id, channel_id, name, parent_name, position, kind, updated_at) ",
        );
        query.push_values(&command.channels, |mut row, channel| {
            row.push_bind(guild_id)
                .push_bind(&channel.id)
                .push_bind(&channel.name)
                .push_bind(&channel.parent_name)
                .push_bind(channel.position)
                .push_bind(channel.kind.as_str())
                .push_bind(synced_at);
        });
        query.build().execute(&mut **transaction).await?;
    }

    sqlx::query(
        "INSERT INTO guild_discord_catalog_state (guild_id, synced_at)
         VALUES ($1, $2)
         ON CONFLICT (guild_id) DO UPDATE SET synced_at = EXCLUDED.synced_at",
    )
    .bind(guild_id)
    .bind(synced_at)
    .execute(&mut **transaction)
    .await?;

    Ok(DiscordCatalogSyncResult { synced_at })
}

async fn update_attempt(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &DiscordUserId,
    request_id: &str,
    audit_id: Uuid,
    command: &UpdateGuildSettingsCommand,
) -> Result<GuildSettingsSnapshot, AttemptError> {
    let locked = lock_operator_settings(transaction, guild_id, actor_id).await?;
    ensure_revision(command.expected_revision, &locked)?;
    validate_changed_selections(transaction, guild_id, &locked, command).await?;

    let before = locked.settings();
    let after = command.settings();
    let next_revision = locked
        .revision
        .checked_add(1)
        .ok_or(AttemptError::InvariantViolation)?;
    let now = OffsetDateTime::now_utc();
    let updated = sqlx::query(
        "UPDATE guild_config
         SET display_name = $1,
             operator_role_id = $2,
             recruitment_channel_id = $3,
             entry_channel_id = $4,
             log_channel_id = $5,
             is_externally_public = $6,
             revision = $7,
             updated_by = $8,
             updated_at = $9
         WHERE guild_id = $10 AND revision = $11",
    )
    .bind(&after.display_name)
    .bind(&after.operator_role_id)
    .bind(&after.recruitment_channel_id)
    .bind(&after.entry_channel_id)
    .bind(&after.log_channel_id)
    .bind(after.is_externally_public)
    .bind(next_revision)
    .bind(actor_id)
    .bind(now)
    .bind(guild_id)
    .bind(locked.revision)
    .execute(&mut **transaction)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(AttemptError::InvariantViolation);
    }

    insert_audit(
        transaction,
        guild_id,
        AuditInsert {
            id: audit_id,
            actor_id,
            request_id,
            before_summary: serde_json::to_value(before)?,
            after_summary: serde_json::to_value(after)?,
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
    let authorized = sqlx::query_scalar::<_, bool>(
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
    if authorized.is_some() {
        Ok(())
    } else {
        Err(classify_access_failure(connection, guild_id, actor_id).await?)
    }
}

async fn lock_operator_settings(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    actor_id: &DiscordUserId,
) -> Result<SettingsRow, AttemptError> {
    let row = sqlx::query_as::<_, SettingsRow>(
        "SELECT config.display_name,
                config.operator_role_id,
                config.recruitment_channel_id,
                config.entry_channel_id,
                config.log_channel_id,
                config.is_externally_public,
                config.revision,
                config.updated_at,
                config.updated_by
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
         FOR SHARE OF guild, actor, actor_role
         FOR UPDATE OF config",
    )
    .bind(guild_id)
    .bind(actor_id)
    .fetch_optional(&mut **transaction)
    .await?;
    match row {
        Some(row) => Ok(row),
        None => Err(classify_access_failure(transaction, guild_id, actor_id).await?),
    }
}

async fn classify_access_failure(
    connection: &mut PgConnection,
    guild_id: &GuildId,
    actor_id: &DiscordUserId,
) -> Result<AttemptError, sqlx::Error> {
    let active_member = sqlx::query_scalar::<_, bool>(
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
    Ok(if active_member.is_some() {
        AttemptError::OperatorRequired
    } else {
        AttemptError::GuildNotFound
    })
}

async fn fetch_snapshot(
    connection: &mut PgConnection,
    guild_id: &GuildId,
) -> Result<GuildSettingsSnapshot, AttemptError> {
    let row = sqlx::query_as::<_, SettingsRow>(
        "SELECT config.display_name,
                config.operator_role_id,
                config.recruitment_channel_id,
                config.entry_channel_id,
                config.log_channel_id,
                config.is_externally_public,
                config.revision,
                config.updated_at,
                config.updated_by
         FROM guilds AS guild
         JOIN guild_config AS config ON config.guild_id = guild.guild_id
         WHERE guild.guild_id = $1 AND guild.status = 'ACTIVE'",
    )
    .bind(guild_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(AttemptError::GuildNotFound)?;

    let catalog_updated_at = sqlx::query_scalar::<_, OffsetDateTime>(
        "SELECT synced_at FROM guild_discord_catalog_state WHERE guild_id = $1",
    )
    .bind(guild_id)
    .fetch_optional(&mut *connection)
    .await?;
    let roles = sqlx::query_as::<_, GuildSettingsRole>(
        "SELECT role_id AS id, name
         FROM guild_discord_roles
         WHERE guild_id = $1 AND NOT managed
         ORDER BY position DESC, lower(name), role_id",
    )
    .bind(guild_id)
    .fetch_all(&mut *connection)
    .await?;
    let channels = sqlx::query_as::<_, GuildSettingsChannel>(
        "SELECT channel_id AS id, name, parent_name
         FROM guild_discord_channels
         WHERE guild_id = $1
         ORDER BY position, lower(name), channel_id",
    )
    .bind(guild_id)
    .fetch_all(&mut *connection)
    .await?;

    Ok(GuildSettingsSnapshot {
        settings: row.settings(),
        revision: row.revision,
        updated_at: row.updated_at,
        updated_by: row.updated_by,
        catalog_updated_at,
        roles,
        channels,
    })
}

async fn validate_changed_selections(
    connection: &mut PgConnection,
    guild_id: &GuildId,
    current: &SettingsRow,
    command: &UpdateGuildSettingsCommand,
) -> Result<(), AttemptError> {
    if command.operator_role_id != current.operator_role_id
        && !catalog_role_exists(connection, guild_id, &command.operator_role_id).await?
    {
        return Err(AttemptError::InvalidSelection {
            field: "operatorRoleId",
        });
    }
    validate_changed_channel(
        connection,
        guild_id,
        "recruitmentChannelId",
        current.recruitment_channel_id.as_ref(),
        command.recruitment_channel_id.as_ref(),
    )
    .await?;
    validate_changed_channel(
        connection,
        guild_id,
        "entryChannelId",
        current.entry_channel_id.as_ref(),
        command.entry_channel_id.as_ref(),
    )
    .await?;
    validate_changed_channel(
        connection,
        guild_id,
        "logChannelId",
        current.log_channel_id.as_ref(),
        command.log_channel_id.as_ref(),
    )
    .await
}

async fn validate_changed_channel(
    connection: &mut PgConnection,
    guild_id: &GuildId,
    field: &'static str,
    current: Option<&DiscordChannelId>,
    next: Option<&DiscordChannelId>,
) -> Result<(), AttemptError> {
    if current != next
        && let Some(channel_id) = next
        && !catalog_channel_exists(connection, guild_id, channel_id).await?
    {
        return Err(AttemptError::InvalidSelection { field });
    }
    Ok(())
}

async fn catalog_role_exists(
    connection: &mut PgConnection,
    guild_id: &GuildId,
    role_id: &DiscordRoleId,
) -> Result<bool, sqlx::Error> {
    Ok(sqlx::query_scalar::<_, bool>(
        "SELECT true
         FROM guild_discord_roles
         WHERE guild_id = $1 AND role_id = $2 AND NOT managed",
    )
    .bind(guild_id)
    .bind(role_id)
    .fetch_optional(&mut *connection)
    .await?
    .is_some())
}

async fn catalog_channel_exists(
    connection: &mut PgConnection,
    guild_id: &GuildId,
    channel_id: &DiscordChannelId,
) -> Result<bool, sqlx::Error> {
    Ok(sqlx::query_scalar::<_, bool>(
        "SELECT true FROM guild_discord_channels WHERE guild_id = $1 AND channel_id = $2",
    )
    .bind(guild_id)
    .bind(channel_id)
    .fetch_optional(&mut *connection)
    .await?
    .is_some())
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
         ) VALUES (
             $1, $2, 'USER', $3, 'GUILD_SETTINGS_UPDATED', 'GUILD_CONFIG', $4,
             NULL, $5, $6, $7, 'APPLIED', NULL, $8
         )",
    )
    .bind(guild_id)
    .bind(audit.id)
    .bind(audit.actor_id)
    .bind(guild_id.as_str())
    .bind(audit.request_id)
    .bind(audit.before_summary)
    .bind(audit.after_summary)
    .bind(audit.now)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn ensure_revision(expected: i64, current: &SettingsRow) -> Result<(), AttemptError> {
    if expected == current.revision {
        Ok(())
    } else {
        Err(AttemptError::RevisionConflict {
            expected,
            current: current.revision,
            updated_at: current.updated_at,
            updated_by: current.updated_by.clone(),
        })
    }
}

fn normalize_catalog(
    guild_id: &GuildId,
    mut command: ReplaceDiscordCatalogCommand,
) -> Result<ReplaceDiscordCatalogCommand, GuildAdminSettingsRepositoryError> {
    if command.roles.len() > MAX_CATALOG_ROLES {
        return Err(GuildAdminSettingsRepositoryError::InvalidCatalog { field: "roles" });
    }
    if command.channels.len() > MAX_CATALOG_CHANNELS {
        return Err(GuildAdminSettingsRepositoryError::InvalidCatalog { field: "channels" });
    }

    let mut role_ids = HashSet::with_capacity(command.roles.len());
    for role in &mut command.roles {
        if role.position < 0
            || role.id.as_str() == guild_id.as_str()
            || !role_ids.insert(role.id.as_str().to_owned())
        {
            return Err(GuildAdminSettingsRepositoryError::InvalidCatalog { field: "roles" });
        }
        role.name = normalized_text(&role.name, MAX_DISCORD_NAME_CHARS).ok_or(
            GuildAdminSettingsRepositoryError::InvalidCatalog {
                field: "roles.name",
            },
        )?;
    }

    let mut channel_ids = HashSet::with_capacity(command.channels.len());
    for channel in &mut command.channels {
        if channel.position < 0 || !channel_ids.insert(channel.id.as_str().to_owned()) {
            return Err(GuildAdminSettingsRepositoryError::InvalidCatalog { field: "channels" });
        }
        channel.name = normalized_text(&channel.name, MAX_DISCORD_NAME_CHARS).ok_or(
            GuildAdminSettingsRepositoryError::InvalidCatalog {
                field: "channels.name",
            },
        )?;
        channel.parent_name = match channel.parent_name.take() {
            Some(parent_name) => {
                Some(normalized_text(&parent_name, MAX_DISCORD_NAME_CHARS).ok_or(
                    GuildAdminSettingsRepositoryError::InvalidCatalog {
                        field: "channels.parentName",
                    },
                )?)
            }
            None => None,
        };
    }
    Ok(command)
}

fn normalize_settings(
    guild_id: &GuildId,
    mut command: UpdateGuildSettingsCommand,
) -> Result<UpdateGuildSettingsCommand, GuildAdminSettingsRepositoryError> {
    if command.expected_revision < 0 {
        return Err(GuildAdminSettingsRepositoryError::InvalidExpectedRevision);
    }
    command.display_name = normalized_text(&command.display_name, MAX_DISPLAY_NAME_CHARS).ok_or(
        GuildAdminSettingsRepositoryError::InvalidSettings {
            field: "displayName",
        },
    )?;
    if command.operator_role_id.as_str() == guild_id.as_str() {
        return Err(GuildAdminSettingsRepositoryError::InvalidSettings {
            field: "operatorRoleId",
        });
    }
    Ok(command)
}

fn normalized_text(value: &str, maximum: usize) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value.chars().count() <= maximum).then(|| value.to_owned())
}

fn attempt_sql_error(error: &AttemptError) -> Option<&sqlx::Error> {
    match error {
        AttemptError::Sql(error) => Some(error),
        _ => None,
    }
}

fn map_transaction_error(
    error: TransactionRetryError<AttemptError>,
) -> GuildAdminSettingsRepositoryError {
    match error {
        TransactionRetryError::DeadlockRetriesExhausted { .. } => {
            GuildAdminSettingsRepositoryError::DatabaseBusy
        }
        TransactionRetryError::NonRetryable { failure, .. } => match failure {
            TransactionAttemptError::Begin(error) | TransactionAttemptError::Commit(error) => {
                GuildAdminSettingsRepositoryError::Database(error)
            }
            TransactionAttemptError::Operation(error) => error.into_public(),
        },
    }
}

impl UpdateGuildSettingsCommand {
    fn settings(&self) -> GuildSettingsValues {
        GuildSettingsValues {
            display_name: self.display_name.clone(),
            operator_role_id: self.operator_role_id.clone(),
            recruitment_channel_id: self.recruitment_channel_id.clone(),
            entry_channel_id: self.entry_channel_id.clone(),
            log_channel_id: self.log_channel_id.clone(),
            is_externally_public: self.is_externally_public,
        }
    }
}

#[derive(Debug, FromRow)]
struct SettingsRow {
    display_name: String,
    operator_role_id: DiscordRoleId,
    recruitment_channel_id: Option<DiscordChannelId>,
    entry_channel_id: Option<DiscordChannelId>,
    log_channel_id: Option<DiscordChannelId>,
    is_externally_public: bool,
    revision: i64,
    updated_at: OffsetDateTime,
    updated_by: Option<DiscordUserId>,
}

impl SettingsRow {
    fn settings(&self) -> GuildSettingsValues {
        GuildSettingsValues {
            display_name: self.display_name.clone(),
            operator_role_id: self.operator_role_id.clone(),
            recruitment_channel_id: self.recruitment_channel_id.clone(),
            entry_channel_id: self.entry_channel_id.clone(),
            log_channel_id: self.log_channel_id.clone(),
            is_externally_public: self.is_externally_public,
        }
    }
}

struct AuditInsert<'a> {
    id: Uuid,
    actor_id: &'a DiscordUserId,
    request_id: &'a str,
    before_summary: Value,
    after_summary: Value,
    now: OffsetDateTime,
}

#[derive(Debug, Error)]
enum AttemptError {
    #[error("PostgreSQL guild admin settings operation failed")]
    Sql(#[from] sqlx::Error),
    #[error("guild admin settings serialization failed")]
    Serialization(#[from] serde_json::Error),
    #[error("guild was not found or is inaccessible")]
    GuildNotFound,
    #[error("operator permission is required")]
    OperatorRequired,
    #[error("expected revision {expected}, current revision {current}")]
    RevisionConflict {
        expected: i64,
        current: i64,
        updated_at: OffsetDateTime,
        updated_by: Option<DiscordUserId>,
    },
    #[error("stored settings selection is invalid for field {field}")]
    InvalidSelection { field: &'static str },
    #[error("stored guild admin settings invariant was violated")]
    InvariantViolation,
}

impl AttemptError {
    fn into_public(self) -> GuildAdminSettingsRepositoryError {
        match self {
            Self::Sql(error) => GuildAdminSettingsRepositoryError::Database(error),
            Self::Serialization(error) => GuildAdminSettingsRepositoryError::Serialization(error),
            Self::GuildNotFound => GuildAdminSettingsRepositoryError::GuildNotFound,
            Self::OperatorRequired => GuildAdminSettingsRepositoryError::OperatorRequired,
            Self::RevisionConflict {
                expected,
                current,
                updated_at,
                updated_by,
            } => GuildAdminSettingsRepositoryError::RevisionConflict {
                expected,
                current,
                updated_at,
                updated_by,
            },
            Self::InvalidSelection { field } => {
                GuildAdminSettingsRepositoryError::InvalidSelection { field }
            }
            Self::InvariantViolation => GuildAdminSettingsRepositoryError::InvariantViolation,
        }
    }
}

#[derive(Debug, Error)]
pub enum GuildAdminSettingsRepositoryError {
    #[error("PostgreSQL guild admin settings operation failed")]
    Database(#[from] sqlx::Error),
    #[error("guild admin settings serialization failed")]
    Serialization(#[from] serde_json::Error),
    #[error("guild was not found or is inaccessible")]
    GuildNotFound,
    #[error("operator permission is required")]
    OperatorRequired,
    #[error("expected revision {expected}, current revision {current}")]
    RevisionConflict {
        expected: i64,
        current: i64,
        updated_at: OffsetDateTime,
        updated_by: Option<DiscordUserId>,
    },
    #[error("expected revision is invalid")]
    InvalidExpectedRevision,
    #[error("guild settings are invalid for field {field}")]
    InvalidSettings { field: &'static str },
    #[error("guild settings selection is invalid for field {field}")]
    InvalidSelection { field: &'static str },
    #[error("Discord catalog is invalid for field {field}")]
    InvalidCatalog { field: &'static str },
    #[error("database remained busy after bounded retries")]
    DatabaseBusy,
    #[error("stored guild admin settings invariant was violated")]
    InvariantViolation,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn role_id(value: &str) -> DiscordRoleId {
        DiscordRoleId::parse(value).expect("valid role ID")
    }

    fn guild_id() -> GuildId {
        GuildId::parse("200000000000000001").expect("valid guild ID")
    }

    #[test]
    fn settings_normalization_trims_names_and_rejects_everyone_or_negative_revision() {
        let valid = UpdateGuildSettingsCommand {
            display_name: "  MonkeyBot Guild  ".to_owned(),
            operator_role_id: role_id("300000000000000001"),
            recruitment_channel_id: None,
            entry_channel_id: None,
            log_channel_id: None,
            is_externally_public: false,
            expected_revision: 0,
        };
        let normalized = normalize_settings(&guild_id(), valid.clone()).expect("valid settings");
        assert_eq!(normalized.display_name, "MonkeyBot Guild");

        assert!(matches!(
            normalize_settings(
                &guild_id(),
                UpdateGuildSettingsCommand {
                    operator_role_id: role_id("200000000000000001"),
                    ..valid.clone()
                }
            ),
            Err(GuildAdminSettingsRepositoryError::InvalidSettings {
                field: "operatorRoleId"
            })
        ));
        assert!(matches!(
            normalize_settings(
                &guild_id(),
                UpdateGuildSettingsCommand {
                    expected_revision: -1,
                    ..valid
                }
            ),
            Err(GuildAdminSettingsRepositoryError::InvalidExpectedRevision)
        ));
    }

    #[test]
    fn catalog_normalization_rejects_duplicate_ids_and_blank_names() {
        let duplicate = ReplaceDiscordCatalogCommand {
            roles: vec![
                DiscordCatalogRoleCommand {
                    id: role_id("300000000000000001"),
                    name: "Operators".to_owned(),
                    position: 1,
                    managed: false,
                },
                DiscordCatalogRoleCommand {
                    id: role_id("300000000000000001"),
                    name: "Duplicate".to_owned(),
                    position: 2,
                    managed: false,
                },
            ],
            channels: Vec::new(),
        };
        assert!(matches!(
            normalize_catalog(&guild_id(), duplicate),
            Err(GuildAdminSettingsRepositoryError::InvalidCatalog { field: "roles" })
        ));

        let blank = ReplaceDiscordCatalogCommand {
            roles: vec![DiscordCatalogRoleCommand {
                id: role_id("300000000000000001"),
                name: "   ".to_owned(),
                position: 1,
                managed: false,
            }],
            channels: Vec::new(),
        };
        assert!(matches!(
            normalize_catalog(&guild_id(), blank),
            Err(GuildAdminSettingsRepositoryError::InvalidCatalog {
                field: "roles.name"
            })
        ));
    }
}
