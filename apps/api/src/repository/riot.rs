use sqlx::{FromRow, PgPool, Postgres, Transaction};
use thiserror::Error;
use time::OffsetDateTime;
use unicode_casefold::UnicodeCaseFold;
use unicode_normalization::UnicodeNormalization;

use crate::domain::DiscordUserId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalRiotAccount {
    pub puuid: String,
    pub platform: String,
    pub routing_region: String,
    pub game_name: String,
    pub tag_line: String,
    pub profile_icon_id: Option<i32>,
    pub last_synced_at: OffsetDateTime,
}

#[derive(Clone, Debug, FromRow, Eq, PartialEq)]
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
    let accounts = sqlx::query_as::<_, RiotAccountRecord>(ACCOUNT_SELECT)
        .bind(user_id)
        .fetch_all(pool)
        .await?;
    verify_main_invariant(&accounts)?;
    Ok(accounts)
}

pub async fn get_account(
    pool: &PgPool,
    user_id: &DiscordUserId,
    puuid: &str,
) -> Result<Option<RiotAccountRecord>, RiotRepositoryError> {
    let row = sqlx::query_as::<_, RiotAccountRecord>(
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
    Ok(row)
}

pub async fn link_account(
    pool: &PgPool,
    user_id: &DiscordUserId,
    account: &CanonicalRiotAccount,
) -> Result<Vec<RiotAccountRecord>, RiotRepositoryError> {
    let (normalized_game_name, normalized_tag_line) = normalized_identity(account)?;
    let mut transaction = pool.begin().await?;
    lock_user(&mut transaction, user_id).await?;

    let existing_owner = sqlx::query_scalar::<_, String>(
        "SELECT user_id FROM riot_accounts WHERE puuid = $1 FOR UPDATE",
    )
    .bind(&account.puuid)
    .fetch_optional(&mut *transaction)
    .await?;

    match existing_owner {
        Some(owner) if owner != user_id.as_str() => {
            return Err(RiotRepositoryError::AlreadyLinked);
        }
        Some(_) => {
            update_canonical_identity(
                &mut transaction,
                user_id,
                account,
                &normalized_game_name,
                &normalized_tag_line,
            )
            .await?;
        }
        None => {
            let has_account = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS (SELECT 1 FROM riot_accounts WHERE user_id = $1)",
            )
            .bind(user_id)
            .fetch_one(&mut *transaction)
            .await?;

            sqlx::query(
                "INSERT INTO riot_accounts (
                    puuid, user_id, platform, routing_region,
                    game_name, tag_line, normalized_game_name, normalized_tag_line,
                    profile_icon_id, is_main, main_position, main_position_updated_at,
                    last_synced_at, created_at, updated_at
                 ) VALUES (
                    $1, $2, $3, $4,
                    $5, $6, $7, $8,
                    $9, $10, NULL, NULL,
                    $11, clock_timestamp(), clock_timestamp()
                 )",
            )
            .bind(&account.puuid)
            .bind(user_id)
            .bind(&account.platform)
            .bind(&account.routing_region)
            .bind(&account.game_name)
            .bind(&account.tag_line)
            .bind(&normalized_game_name)
            .bind(&normalized_tag_line)
            .bind(account.profile_icon_id)
            .bind(!has_account)
            .bind(account.last_synced_at)
            .execute(&mut *transaction)
            .await
            .map_err(map_write_error)?;
        }
    }

    finish_account_mutation(transaction, user_id).await
}

pub async fn refresh_account(
    pool: &PgPool,
    user_id: &DiscordUserId,
    account: &CanonicalRiotAccount,
) -> Result<Vec<RiotAccountRecord>, RiotRepositoryError> {
    let (normalized_game_name, normalized_tag_line) = normalized_identity(account)?;
    let mut transaction = pool.begin().await?;
    lock_user(&mut transaction, user_id).await?;
    require_owned_account(&mut transaction, user_id, &account.puuid).await?;
    update_canonical_identity(
        &mut transaction,
        user_id,
        account,
        &normalized_game_name,
        &normalized_tag_line,
    )
    .await?;
    finish_account_mutation(transaction, user_id).await
}

pub async fn set_main_account(
    pool: &PgPool,
    user_id: &DiscordUserId,
    puuid: &str,
) -> Result<Vec<RiotAccountRecord>, RiotRepositoryError> {
    let mut transaction = pool.begin().await?;
    lock_user(&mut transaction, user_id).await?;
    require_owned_account(&mut transaction, user_id, puuid).await?;

    sqlx::query(
        "UPDATE riot_accounts
         SET is_main = false, updated_at = clock_timestamp()
         WHERE user_id = $1 AND is_main",
    )
    .bind(user_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE riot_accounts
         SET is_main = true, updated_at = clock_timestamp()
         WHERE user_id = $1 AND puuid = $2",
    )
    .bind(user_id)
    .bind(puuid)
    .execute(&mut *transaction)
    .await?;

    finish_account_mutation(transaction, user_id).await
}

pub async fn delete_account(
    pool: &PgPool,
    user_id: &DiscordUserId,
    puuid: &str,
) -> Result<Vec<RiotAccountRecord>, RiotRepositoryError> {
    let mut transaction = pool.begin().await?;
    lock_user(&mut transaction, user_id).await?;

    let deleted = sqlx::query_scalar::<_, String>(
        "DELETE FROM riot_accounts
         WHERE user_id = $1 AND puuid = $2
         RETURNING puuid",
    )
    .bind(user_id)
    .bind(puuid)
    .fetch_optional(&mut *transaction)
    .await?;
    if deleted.is_none() {
        return Err(RiotRepositoryError::NotFound);
    }

    finish_account_mutation(transaction, user_id).await
}

pub async fn get_privacy_settings(
    pool: &PgPool,
    user_id: &DiscordUserId,
) -> Result<UserPrivacySettings, RiotRepositoryError> {
    let row = sqlx::query_as::<_, UserPrivacyRow>(
        "SELECT public_profile_enabled, show_riot_ids,
                allow_external_record_search, updated_at
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
         ) VALUES ($1, $2, $3, $4, clock_timestamp())
         ON CONFLICT (user_id) DO UPDATE
         SET public_profile_enabled = EXCLUDED.public_profile_enabled,
             show_riot_ids = EXCLUDED.show_riot_ids,
             allow_external_record_search = EXCLUDED.allow_external_record_search,
             updated_at = EXCLUDED.updated_at
         RETURNING public_profile_enabled, show_riot_ids,
                   allow_external_record_search, updated_at",
    )
    .bind(user_id)
    .bind(settings.public_profile_enabled)
    .bind(settings.show_riot_ids)
    .bind(settings.allow_external_record_search)
    .fetch_one(pool)
    .await?;
    Ok(row.into_record())
}

pub fn normalize_riot_id_v1(value: &str) -> String {
    let nfkc = value.nfkc().collect::<String>();
    nfkc.trim().case_fold().collect()
}

async fn update_canonical_identity(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: &DiscordUserId,
    account: &CanonicalRiotAccount,
    normalized_game_name: &str,
    normalized_tag_line: &str,
) -> Result<(), RiotRepositoryError> {
    let result = sqlx::query(
        "UPDATE riot_accounts
         SET platform = $3,
             routing_region = $4,
             game_name = $5,
             tag_line = $6,
             normalized_game_name = $7,
             normalized_tag_line = $8,
             profile_icon_id = $9,
             last_synced_at = $10,
             updated_at = clock_timestamp()
         WHERE puuid = $1 AND user_id = $2",
    )
    .bind(&account.puuid)
    .bind(user_id)
    .bind(&account.platform)
    .bind(&account.routing_region)
    .bind(&account.game_name)
    .bind(&account.tag_line)
    .bind(normalized_game_name)
    .bind(normalized_tag_line)
    .bind(account.profile_icon_id)
    .bind(account.last_synced_at)
    .execute(&mut **transaction)
    .await
    .map_err(map_write_error)?;
    if result.rows_affected() == 0 {
        return Err(RiotRepositoryError::NotFound);
    }
    Ok(())
}

fn normalized_identity(
    account: &CanonicalRiotAccount,
) -> Result<(String, String), RiotRepositoryError> {
    let game_name = normalize_riot_id_v1(&account.game_name);
    let tag_line = normalize_riot_id_v1(&account.tag_line);
    if account.puuid.is_empty()
        || account.platform.is_empty()
        || account.routing_region.is_empty()
        || game_name.is_empty()
        || tag_line.is_empty()
    {
        return Err(RiotRepositoryError::InvalidCanonicalAccount);
    }
    Ok((game_name, tag_line))
}

async fn lock_user(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: &DiscordUserId,
) -> Result<(), RiotRepositoryError> {
    let exists = sqlx::query_scalar::<_, String>(
        "SELECT discord_id
         FROM users
         WHERE discord_id = $1 AND deleted_at IS NULL
         FOR UPDATE",
    )
    .bind(user_id)
    .fetch_optional(&mut **transaction)
    .await?;
    if exists.is_none() {
        return Err(RiotRepositoryError::UserNotFound);
    }
    Ok(())
}

async fn require_owned_account(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: &DiscordUserId,
    puuid: &str,
) -> Result<(), RiotRepositoryError> {
    let exists = sqlx::query_scalar::<_, String>(
        "SELECT puuid
         FROM riot_accounts
         WHERE user_id = $1 AND puuid = $2
         FOR UPDATE",
    )
    .bind(user_id)
    .bind(puuid)
    .fetch_optional(&mut **transaction)
    .await?;
    if exists.is_none() {
        return Err(RiotRepositoryError::NotFound);
    }
    Ok(())
}

async fn finish_account_mutation(
    mut transaction: Transaction<'_, Postgres>,
    user_id: &DiscordUserId,
) -> Result<Vec<RiotAccountRecord>, RiotRepositoryError> {
    ensure_one_main(&mut transaction, user_id).await?;
    let accounts = list_in_transaction(&mut transaction, user_id).await?;
    transaction.commit().await?;
    Ok(accounts)
}

async fn ensure_one_main(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: &DiscordUserId,
) -> Result<(), RiotRepositoryError> {
    let counts = sqlx::query_as::<_, MainCounts>(
        "SELECT count(*) AS account_count,
                count(*) FILTER (WHERE is_main) AS main_count
         FROM riot_accounts
         WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_one(&mut **transaction)
    .await?;
    if counts.main_count > 1 {
        return Err(RiotRepositoryError::InvariantViolation);
    }
    if counts.account_count > 0 && counts.main_count == 0 {
        sqlx::query(
            "UPDATE riot_accounts
             SET is_main = true, updated_at = clock_timestamp()
             WHERE puuid = (
                 SELECT puuid
                 FROM riot_accounts
                 WHERE user_id = $1
                 ORDER BY created_at, puuid
                 LIMIT 1
             )",
        )
        .bind(user_id)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

async fn list_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: &DiscordUserId,
) -> Result<Vec<RiotAccountRecord>, RiotRepositoryError> {
    let accounts = sqlx::query_as::<_, RiotAccountRecord>(ACCOUNT_SELECT)
        .bind(user_id)
        .fetch_all(&mut **transaction)
        .await?;
    verify_main_invariant(&accounts)?;
    Ok(accounts)
}

fn verify_main_invariant(accounts: &[RiotAccountRecord]) -> Result<(), RiotRepositoryError> {
    let main_count = accounts.iter().filter(|account| account.is_main).count();
    if (!accounts.is_empty() && main_count != 1) || (accounts.is_empty() && main_count != 0) {
        return Err(RiotRepositoryError::InvariantViolation);
    }
    Ok(())
}

fn map_write_error(error: sqlx::Error) -> RiotRepositoryError {
    let constraint = match &error {
        sqlx::Error::Database(database) => database.constraint(),
        _ => None,
    };
    match constraint {
        Some("riot_accounts_pkey") => RiotRepositoryError::AlreadyLinked,
        Some("riot_accounts_normalized_id_key") => RiotRepositoryError::IdentityConflict,
        _ => RiotRepositoryError::Sql(error),
    }
}

const ACCOUNT_SELECT: &str = "SELECT puuid, platform, routing_region, game_name, tag_line,
            normalized_game_name, normalized_tag_line, profile_icon_id,
            is_main, main_position, main_position_updated_at,
            last_synced_at, created_at, updated_at
     FROM riot_accounts
     WHERE user_id = $1
     ORDER BY is_main DESC, created_at, puuid";

#[derive(Debug, FromRow)]
struct MainCounts {
    account_count: i64,
    main_count: i64,
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
    #[error("PostgreSQL Riot repository operation failed")]
    Sql(#[from] sqlx::Error),
    #[error("user does not exist")]
    UserNotFound,
    #[error("Riot account was not found for this user")]
    NotFound,
    #[error("Riot account is already linked to another user")]
    AlreadyLinked,
    #[error("canonical Riot ID conflicts with another account")]
    IdentityConflict,
    #[error("canonical Riot account is invalid")]
    InvalidCanonicalAccount,
    #[error("Riot account main invariant is violated")]
    InvariantViolation,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_is_nfkc_trimmed_and_full_case_folded() {
        assert_eq!(normalize_riot_id_v1("  ＬＯＮＧＣＡＴ  "), "longcat");
        assert_eq!(normalize_riot_id_v1("Straße"), "strasse");
        assert_eq!(normalize_riot_id_v1("Hide  On Bush"), "hide  on bush");
    }

    #[test]
    fn in_memory_main_invariant_matches_database_rule() {
        let now = OffsetDateTime::UNIX_EPOCH;
        let account = |suffix: &str, is_main| RiotAccountRecord {
            puuid: format!("puuid-{suffix}"),
            platform: "KR".to_owned(),
            routing_region: "ASIA".to_owned(),
            game_name: "Name".to_owned(),
            tag_line: "KR1".to_owned(),
            normalized_game_name: "name".to_owned(),
            normalized_tag_line: "kr1".to_owned(),
            profile_icon_id: None,
            is_main,
            main_position: None,
            main_position_updated_at: None,
            last_synced_at: None,
            created_at: now,
            updated_at: now,
        };
        assert!(verify_main_invariant(&[]).is_ok());
        assert!(verify_main_invariant(&[account("a", true), account("b", false)]).is_ok());
        assert!(verify_main_invariant(&[account("a", false)]).is_err());
        assert!(verify_main_invariant(&[account("a", true), account("b", true)]).is_err());
    }
}
