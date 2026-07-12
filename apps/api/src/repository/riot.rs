use sqlx::{FromRow, PgPool};
use thiserror::Error;
use time::OffsetDateTime;

use crate::domain::DiscordUserId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RiotAccountRecord {
    pub puuid: String,
    pub platform: String,
    pub routing_region: String,
    pub game_name: String,
    pub tag_line: String,
    pub normalized_game_name: String,
    pub normalized_tag_line: String,
    pub profile_icon_id: Option<i32>,
    pub is_main: bool,
    pub main_position: Option<String>,
    pub main_position_updated_at: Option<OffsetDateTime>,
    pub last_synced_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NewRiotAccount {
    pub user_id: DiscordUserId,
    pub puuid: String,
    pub platform: String,
    pub routing_region: String,
    pub game_name: String,
    pub tag_line: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserPrivacySettings {
    pub public_profile_enabled: bool,
    pub show_riot_ids: bool,
    pub allow_external_record_search: bool,
    pub updated_at: OffsetDateTime,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdatedPrivacySettings {
    pub public_profile_enabled: bool,
    pub show_riot_ids: bool,
    pub allow_external_record_search: bool,
}

pub async fn list_accounts(
    pool: &PgPool,
    user_id: &DiscordUserId,
) -> Result<Vec<RiotAccountRecord>, RiotRepositoryError> {
    let rows = sqlx::query_as::<_, RiotAccountRow>(
        "SELECT puuid, platform, routing_region, game_name, tag_line,
                normalized_game_name, normalized_tag_line, profile_icon_id,
                is_main, main_position, main_position_updated_at,
                last_synced_at, created_at, updated_at
         FROM riot_accounts
         WHERE user_id = $1
         ORDER BY is_main DESC, created_at DESC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(RiotAccountRecord::from).collect())
}

pub async fn get_account(
    pool: &PgPool,
    user_id: &DiscordUserId,
    puuid: &str,
) -> Result<Option<RiotAccountRecord>, RiotRepositoryError> {
    let row = sqlx::query_as::<_, RiotAccountRow>(
        "SELECT puuid, platform, routing_region, game_name, tag_line,
                normalized_game_name, normalized_tag_line, profile_icon_id,
                is_main, main_position, main_position_updated_at,
                last_synced_at, created_at, updated_at
         FROM riot_accounts
         WHERE user_id = $1 AND puuid = $2",
    )
    .bind(user_id)
    .bind(puuid)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(RiotAccountRecord::from))
}

pub async fn add_account(
    pool: &PgPool,
    request: NewRiotAccount,
) -> Result<RiotAccountRecord, RiotRepositoryError> {
    let mut transaction = pool.begin().await?;
    let normalized_game_name = normalize_riot_id(&request.game_name)?;
    let normalized_tag_line = normalize_riot_id(&request.tag_line)?;

    let existing =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM riot_accounts WHERE user_id = $1")
            .bind(&request.user_id)
            .fetch_one(&mut *transaction)
            .await?;
    let is_main = existing == 0;

    let row = sqlx::query_as::<_, RiotAccountRow>(
        "INSERT INTO riot_accounts (
             puuid, user_id, platform, routing_region,
             game_name, tag_line, normalized_game_name, normalized_tag_line,
             profile_icon_id, is_main, main_position, main_position_updated_at,
             last_synced_at, created_at, updated_at
         ) VALUES (
             $1, $2, $3, $4, $5, $6, $7, $8,
             NULL, $9, NULL, NULL, NULL, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
         )
         RETURNING puuid, platform, routing_region, game_name, tag_line,
                   normalized_game_name, normalized_tag_line, profile_icon_id,
                   is_main, main_position, main_position_updated_at,
                   last_synced_at, created_at, updated_at",
    )
    .bind(&request.puuid)
    .bind(&request.user_id)
    .bind(&request.platform)
    .bind(&request.routing_region)
    .bind(&request.game_name)
    .bind(&request.tag_line)
    .bind(&normalized_game_name)
    .bind(&normalized_tag_line)
    .bind(is_main)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| match error {
        sqlx::Error::Database(err) if is_unique_violation(err.as_ref()) => {
            RiotRepositoryError::DuplicateAccount
        }
        error => RiotRepositoryError::Sql(error),
    })?;

    let row = row.ok_or_else(|| RiotRepositoryError::InvalidAccountData)?;
    transaction.commit().await?;
    Ok(row.into())
}

pub async fn delete_account(
    pool: &PgPool,
    user_id: &DiscordUserId,
    puuid: &str,
) -> Result<Option<RiotAccountRecord>, RiotRepositoryError> {
    let mut transaction = pool.begin().await?;

    let deleted = sqlx::query_as::<_, RiotAccountRow>(
        "DELETE FROM riot_accounts
         WHERE user_id = $1 AND puuid = $2
         RETURNING puuid, platform, routing_region, game_name, tag_line,
                   normalized_game_name, normalized_tag_line, profile_icon_id,
                   is_main, main_position, main_position_updated_at,
                   last_synced_at, created_at, updated_at",
    )
    .bind(user_id)
    .bind(puuid)
    .fetch_optional(&mut *transaction)
    .await?;

    if let Some(deleted) = deleted {
        let deleted = RiotAccountRecord::from(deleted);

        if deleted.is_main {
            let remaining = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM riot_accounts WHERE user_id = $1",
            )
            .bind(user_id)
            .fetch_one(&mut *transaction)
            .await?;
            if remaining > 0 {
                sqlx::query(
                    "UPDATE riot_accounts
                     SET is_main = TRUE
                     WHERE user_id = $1
                       AND puuid = (
                         SELECT puuid
                         FROM riot_accounts
                         WHERE user_id = $1
                         ORDER BY created_at ASC
                         LIMIT 1
                     )",
                )
                .bind(user_id)
                .execute(&mut *transaction)
                .await?;
            }
        }

        transaction.commit().await?;
        return Ok(Some(deleted));
    }

    transaction.commit().await?;
    Ok(None)
}

pub async fn set_main_account(
    pool: &PgPool,
    user_id: &DiscordUserId,
    puuid: &str,
) -> Result<Option<RiotAccountRecord>, RiotRepositoryError> {
    let mut transaction = pool.begin().await?;

    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM riot_accounts WHERE user_id = $1 AND puuid = $2)",
    )
    .bind(user_id)
    .bind(puuid)
    .fetch_one(&mut *transaction)
    .await?;
    if !exists {
        transaction.commit().await?;
        return Ok(None);
    }

    sqlx::query("UPDATE riot_accounts SET is_main = FALSE WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("UPDATE riot_accounts SET is_main = TRUE WHERE user_id = $1 AND puuid = $2")
        .bind(user_id)
        .bind(puuid)
        .execute(&mut *transaction)
        .await?;

    let row = sqlx::query_as::<_, RiotAccountRow>(
        "SELECT puuid, platform, routing_region, game_name, tag_line,
                normalized_game_name, normalized_tag_line, profile_icon_id,
                is_main, main_position, main_position_updated_at,
                last_synced_at, created_at, updated_at
         FROM riot_accounts
         WHERE user_id = $1 AND puuid = $2",
    )
    .bind(user_id)
    .bind(puuid)
    .fetch_optional(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(row.map(RiotAccountRecord::from))
}

pub async fn touch_last_synced_at(
    pool: &PgPool,
    user_id: &DiscordUserId,
    puuid: &str,
) -> Result<Option<RiotAccountRecord>, RiotRepositoryError> {
    let row = sqlx::query_as::<_, RiotAccountRow>(
        "UPDATE riot_accounts
         SET last_synced_at = CURRENT_TIMESTAMP,
             updated_at = CURRENT_TIMESTAMP
         WHERE user_id = $1 AND puuid = $2
         RETURNING puuid, platform, routing_region, game_name, tag_line,
                   normalized_game_name, normalized_tag_line, profile_icon_id,
                   is_main, main_position, main_position_updated_at,
                   last_synced_at, created_at, updated_at",
    )
    .bind(user_id)
    .bind(puuid)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(RiotAccountRecord::from))
}

pub async fn get_privacy_settings(
    pool: &PgPool,
    user_id: &DiscordUserId,
) -> Result<UserPrivacySettings, RiotRepositoryError> {
    let row = sqlx::query_as::<_, UserPrivacyRow>(
        "SELECT public_profile_enabled, show_riot_ids, allow_external_record_search, updated_at
         FROM user_privacy_settings
         WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    Ok(row
        .map(UserPrivacyRow::into_record)
        .unwrap_or_else(|| UserPrivacySettings {
            public_profile_enabled: true,
            show_riot_ids: true,
            allow_external_record_search: false,
            updated_at: OffsetDateTime::now_utc(),
        }))
}

pub async fn upsert_privacy_settings(
    pool: &PgPool,
    user_id: &DiscordUserId,
    settings: UpdatedPrivacySettings,
) -> Result<UserPrivacySettings, RiotRepositoryError> {
    let row = sqlx::query_as::<_, UserPrivacyRow>(
        "INSERT INTO user_privacy_settings (
             user_id, public_profile_enabled, show_riot_ids,
             allow_external_record_search, updated_at
         ) VALUES ($1, $2, $3, $4, CURRENT_TIMESTAMP)
         ON CONFLICT (user_id)
           DO UPDATE SET
             public_profile_enabled = EXCLUDED.public_profile_enabled,
             show_riot_ids = EXCLUDED.show_riot_ids,
             allow_external_record_search = EXCLUDED.allow_external_record_search,
             updated_at = CURRENT_TIMESTAMP
         RETURNING public_profile_enabled, show_riot_ids, allow_external_record_search, updated_at",
    )
    .bind(user_id)
    .bind(settings.public_profile_enabled)
    .bind(settings.show_riot_ids)
    .bind(settings.allow_external_record_search)
    .fetch_one(pool)
    .await?;

    Ok(row.into_record())
}

fn normalize_riot_id(value: &str) -> Result<String, RiotRepositoryError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(RiotRepositoryError::InvalidInput);
    }
    if trimmed.len() > 64 {
        return Err(RiotRepositoryError::InvalidInput);
    }

    let normalized = trimmed.to_lowercase();
    if normalized.is_empty() {
        Err(RiotRepositoryError::InvalidInput)
    } else {
        Ok(normalized)
    }
}

fn is_unique_violation(error: &(dyn sqlx::error::DatabaseError + 'static)) -> bool {
    error.code().as_deref() == Some("23505")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_riot_id_rejects_empty_and_trims_edges() {
        assert!(matches!(
            normalize_riot_id(""),
            Err(RiotRepositoryError::InvalidInput)
        ));
        assert!(matches!(
            normalize_riot_id("  123ABC  "),
            Ok(value) if value == "123abc"
        ));
        assert!(matches!(
            normalize_riot_id("   "),
            Err(RiotRepositoryError::InvalidInput)
        ));
        assert!(matches!(
            normalize_riot_id("abc\n\n"),
            Ok(value) if value == "abc"
        ));
    }
}

#[derive(Debug, FromRow)]
struct RiotAccountRow {
    puuid: String,
    platform: String,
    routing_region: String,
    game_name: String,
    tag_line: String,
    normalized_game_name: String,
    normalized_tag_line: String,
    profile_icon_id: Option<i32>,
    is_main: bool,
    main_position: Option<String>,
    main_position_updated_at: Option<OffsetDateTime>,
    last_synced_at: Option<OffsetDateTime>,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

impl From<RiotAccountRow> for RiotAccountRecord {
    fn from(row: RiotAccountRow) -> Self {
        Self {
            puuid: row.puuid,
            platform: row.platform,
            routing_region: row.routing_region,
            game_name: row.game_name,
            tag_line: row.tag_line,
            normalized_game_name: row.normalized_game_name,
            normalized_tag_line: row.normalized_tag_line,
            profile_icon_id: row.profile_icon_id,
            is_main: row.is_main,
            main_position: row.main_position,
            main_position_updated_at: row.main_position_updated_at,
            last_synced_at: row.last_synced_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(Debug, FromRow)]
struct UserPrivacyRow {
    public_profile_enabled: bool,
    show_riot_ids: bool,
    allow_external_record_search: bool,
    updated_at: OffsetDateTime,
}

impl UserPrivacyRow {
    fn into_record(self) -> UserPrivacySettings {
        UserPrivacySettings {
            public_profile_enabled: self.public_profile_enabled,
            show_riot_ids: self.show_riot_ids,
            allow_external_record_search: self.allow_external_record_search,
            updated_at: self.updated_at,
        }
    }
}

#[derive(Debug, Error)]
pub enum RiotRepositoryError {
    #[error("Riot account is already registered")]
    DuplicateAccount,
    #[error("Riot account payload contains invalid field")]
    InvalidInput,
    #[error("Riot account data is invalid")]
    InvalidAccountData,
    #[error("PostgreSQL Riot repository operation failed")]
    Sql(#[from] sqlx::Error),
}
