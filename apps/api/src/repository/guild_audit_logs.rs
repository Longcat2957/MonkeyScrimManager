use serde::Serialize;
use serde_json::Value;
use sqlx::{FromRow, PgConnection, PgPool};
use thiserror::Error;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::domain::{GuildId, UserId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuditLogCursor {
    pub created_at: OffsetDateTime,
    pub id: Uuid,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildAuditLogEntry {
    pub id: Uuid,
    pub principal_type: AuditPrincipalType,
    pub principal_id: String,
    pub principal_display_name: Option<String>,
    pub action: String,
    pub target_type: Option<String>,
    pub target_id: Option<String>,
    pub target_display_name: Option<String>,
    pub checkpoint_id: Option<Uuid>,
    pub request_id: Option<String>,
    pub before_summary: Option<Value>,
    pub after_summary: Option<Value>,
    pub result: AuditResult,
    pub note: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum AuditPrincipalType {
    User,
    Bot,
    System,
    Migration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum AuditResult {
    Applied,
    Reverted,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GuildAuditLogPage {
    pub entries: Vec<GuildAuditLogEntry>,
    pub next_cursor: Option<AuditLogCursor>,
}

pub async fn list(
    pool: &PgPool,
    guild_id: &GuildId,
    actor_id: &UserId,
    cursor: Option<AuditLogCursor>,
    limit: i64,
) -> Result<GuildAuditLogPage, GuildAuditLogsRepositoryError> {
    let mut transaction = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *transaction)
        .await?;
    authorize_operator(&mut transaction, guild_id, actor_id)
        .await
        .map_err(AttemptError::into_public)?;
    let page = fetch_page(&mut transaction, guild_id, cursor, limit)
        .await
        .map_err(AttemptError::into_public)?;
    transaction.commit().await?;
    Ok(page)
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

async fn fetch_page(
    connection: &mut PgConnection,
    guild_id: &GuildId,
    cursor: Option<AuditLogCursor>,
    limit: i64,
) -> Result<GuildAuditLogPage, AttemptError> {
    let (cursor_created_at, cursor_id) = cursor.map_or((None, None), |cursor| {
        (Some(cursor.created_at), Some(cursor.id))
    });
    let rows = sqlx::query_as::<_, AuditLogRow>(
        "SELECT audit.id,
                audit.principal_type,
                audit.principal_id,
                CASE WHEN audit.principal_type = 'USER'
                     THEN COALESCE(principal.display_name, principal_user.display_name)
                     ELSE NULL
                END AS principal_display_name,
                audit.action,
                audit.target_type,
                audit.target_id,
                CASE audit.target_type
                    WHEN 'GUILD' THEN config.display_name
                    WHEN 'GUILD_CONFIG' THEN config.display_name
                    WHEN 'GUILD_MEMBER' THEN target_member.display_name
                    WHEN 'GUILD_MEMBER_MMR' THEN target_member.display_name
                    WHEN 'SEASON' THEN target_season.name
                    WHEN 'RECRUITMENT' THEN target_recruitment.title
                    ELSE NULL
                END AS target_display_name,
                audit.checkpoint_id,
                audit.request_id,
                audit.before_summary,
                audit.after_summary,
                audit.result,
                audit.note,
                audit.created_at
         FROM admin_audit_log AS audit
         JOIN guild_config AS config ON config.guild_id = audit.guild_id
         LEFT JOIN guild_members AS principal
           ON principal.guild_id = audit.guild_id
          AND principal.user_id = audit.principal_id
          AND audit.principal_type = 'USER'
         LEFT JOIN users AS principal_user
           ON principal_user.user_id = audit.principal_id
          AND audit.principal_type = 'USER'
         LEFT JOIN guild_members AS target_member
           ON target_member.guild_id = audit.guild_id
          AND target_member.user_id = audit.target_id
          AND audit.target_type IN ('GUILD_MEMBER', 'GUILD_MEMBER_MMR')
         LEFT JOIN seasons AS target_season
           ON target_season.guild_id = audit.guild_id
          AND target_season.id::text = audit.target_id
          AND audit.target_type = 'SEASON'
         LEFT JOIN recruitments AS target_recruitment
           ON target_recruitment.guild_id = audit.guild_id
          AND target_recruitment.id::text = audit.target_id
          AND audit.target_type = 'RECRUITMENT'
         WHERE audit.guild_id = $1
           AND (
               $2::timestamptz IS NULL
               OR (audit.created_at, audit.id) < ($2, $3::uuid)
           )
         ORDER BY audit.created_at DESC, audit.id DESC
         LIMIT $4",
    )
    .bind(guild_id)
    .bind(cursor_created_at)
    .bind(cursor_id)
    .bind(limit + 1)
    .fetch_all(&mut *connection)
    .await?;

    page_from_rows(rows, limit)
}

fn page_from_rows(
    mut rows: Vec<AuditLogRow>,
    limit: i64,
) -> Result<GuildAuditLogPage, AttemptError> {
    let limit = usize::try_from(limit).map_err(|_| AttemptError::InvariantViolation)?;
    let has_next = rows.len() > limit;
    rows.truncate(limit);
    let next_cursor = has_next.then(|| {
        let last = rows
            .last()
            .expect("a retained page with a next row is non-empty");
        AuditLogCursor {
            created_at: last.created_at,
            id: last.id,
        }
    });
    let entries = rows
        .into_iter()
        .map(AuditLogRow::into_entry)
        .collect::<Result<_, _>>()?;
    Ok(GuildAuditLogPage {
        entries,
        next_cursor,
    })
}

#[derive(Debug, FromRow)]
struct AuditLogRow {
    id: Uuid,
    principal_type: String,
    principal_id: String,
    principal_display_name: Option<String>,
    action: String,
    target_type: Option<String>,
    target_id: Option<String>,
    target_display_name: Option<String>,
    checkpoint_id: Option<Uuid>,
    request_id: Option<String>,
    before_summary: Option<Value>,
    after_summary: Option<Value>,
    result: String,
    note: Option<String>,
    created_at: OffsetDateTime,
}

impl AuditLogRow {
    fn into_entry(self) -> Result<GuildAuditLogEntry, AttemptError> {
        let principal_type = match self.principal_type.as_str() {
            "USER" => AuditPrincipalType::User,
            "BOT" => AuditPrincipalType::Bot,
            "SYSTEM" => AuditPrincipalType::System,
            "MIGRATION" => AuditPrincipalType::Migration,
            _ => return Err(AttemptError::InvariantViolation),
        };
        let result = match self.result.as_str() {
            "APPLIED" => AuditResult::Applied,
            "REVERTED" => AuditResult::Reverted,
            _ => return Err(AttemptError::InvariantViolation),
        };
        Ok(GuildAuditLogEntry {
            id: self.id,
            principal_type,
            principal_id: self.principal_id,
            principal_display_name: self.principal_display_name,
            action: self.action,
            target_type: self.target_type,
            target_id: self.target_id,
            target_display_name: self.target_display_name,
            checkpoint_id: self.checkpoint_id,
            request_id: self.request_id,
            before_summary: self.before_summary,
            after_summary: self.after_summary,
            result,
            note: self.note,
            created_at: self.created_at,
        })
    }
}

#[derive(Debug, Error)]
enum AttemptError {
    #[error("guild does not exist or is not accessible")]
    GuildNotFound,
    #[error("guild operator permission is required")]
    OperatorRequired,
    #[error("stored audit-log data violates an invariant")]
    InvariantViolation,
    #[error("database operation failed")]
    Sql(#[from] sqlx::Error),
}

impl AttemptError {
    fn into_public(self) -> GuildAuditLogsRepositoryError {
        match self {
            Self::GuildNotFound => GuildAuditLogsRepositoryError::GuildNotFound,
            Self::OperatorRequired => GuildAuditLogsRepositoryError::OperatorRequired,
            Self::InvariantViolation => GuildAuditLogsRepositoryError::InvariantViolation,
            Self::Sql(error) => GuildAuditLogsRepositoryError::Database(error),
        }
    }
}

#[derive(Debug, Error)]
pub enum GuildAuditLogsRepositoryError {
    #[error("guild does not exist or is not accessible")]
    GuildNotFound,
    #[error("guild operator permission is required")]
    OperatorRequired,
    #[error("stored audit-log data violates an invariant")]
    InvariantViolation,
    #[error("database operation failed")]
    Database(#[from] sqlx::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_cursor_uses_last_retained_entry() {
        let created_at =
            OffsetDateTime::from_unix_timestamp(1_700_000_000).expect("valid test timestamp");
        let ids = [Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7()];
        let rows = ids
            .into_iter()
            .map(|id| AuditLogRow {
                id,
                principal_type: "SYSTEM".to_owned(),
                principal_id: "system".to_owned(),
                principal_display_name: None,
                action: "TEST".to_owned(),
                target_type: None,
                target_id: None,
                target_display_name: None,
                checkpoint_id: None,
                request_id: None,
                before_summary: None,
                after_summary: None,
                result: "APPLIED".to_owned(),
                note: None,
                created_at,
            })
            .collect();

        let page = page_from_rows(rows, 2).expect("valid page");
        assert_eq!(page.entries.len(), 2);
        assert_eq!(page.next_cursor.map(|cursor| cursor.id), Some(ids[1]));
    }
}
