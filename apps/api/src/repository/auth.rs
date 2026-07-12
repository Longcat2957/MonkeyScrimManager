use sqlx::{FromRow, PgPool};
use thiserror::Error;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::domain::{DiscordUserId, GuildId, SnowflakeError};

#[derive(Clone)]
pub struct AuthRepository {
    pool: PgPool,
}

impl AuthRepository {
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn create_oauth_attempt(
        &self,
        attempt: &NewOAuthAttempt,
    ) -> Result<(), AuthRepositoryError> {
        sqlx::query(
            "WITH expired_attempts AS (
                DELETE FROM oauth_login_attempts
                WHERE id IN (
                    SELECT id
                    FROM oauth_login_attempts
                    WHERE expires_at <= CURRENT_TIMESTAMP
                    ORDER BY expires_at, id
                    LIMIT 1000
                )
             )
             INSERT INTO oauth_login_attempts (
                id, state_hash, pkce_verifier_ciphertext, redirect_path,
                created_at, expires_at, consumed_at
             )
             VALUES ($1, $2, $3, $4, CURRENT_TIMESTAMP, $5, NULL)",
        )
        .bind(attempt.id)
        .bind(&attempt.state_hash)
        .bind(&attempt.pkce_verifier_ciphertext)
        .bind(&attempt.redirect_path)
        .bind(attempt.expires_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn consume_oauth_attempt(
        &self,
        state_hash: &[u8],
    ) -> Result<Option<ConsumedOAuthAttempt>, AuthRepositoryError> {
        let attempt = sqlx::query_as::<_, ConsumedOAuthAttempt>(
            "UPDATE oauth_login_attempts
             SET consumed_at = CURRENT_TIMESTAMP
             WHERE state_hash = $1
               AND consumed_at IS NULL
               AND expires_at > CURRENT_TIMESTAMP
             RETURNING id, pkce_verifier_ciphertext, redirect_path",
        )
        .bind(state_hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(attempt)
    }

    pub async fn complete_login(&self, login: &NewLogin) -> Result<(), AuthRepositoryError> {
        let mut transaction = self.pool.begin().await?;

        sqlx::query(
            "INSERT INTO users (
                discord_id, display_name, avatar_hash, created_at, updated_at, deleted_at
             )
             VALUES ($1, $2, $3, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, NULL)
             ON CONFLICT (discord_id) DO UPDATE
             SET display_name = EXCLUDED.display_name,
                 avatar_hash = EXCLUDED.avatar_hash,
                 updated_at = CURRENT_TIMESTAMP,
                 deleted_at = NULL",
        )
        .bind(login.user_id.as_str())
        .bind(&login.display_name)
        .bind(&login.avatar_hash)
        .execute(&mut *transaction)
        .await?;

        sqlx::query(
            "INSERT INTO user_privacy_settings (
                user_id, public_profile_enabled, show_riot_ids,
                allow_external_record_search, updated_at
             )
             VALUES ($1, true, true, false, CURRENT_TIMESTAMP)
             ON CONFLICT (user_id) DO NOTHING",
        )
        .bind(login.user_id.as_str())
        .execute(&mut *transaction)
        .await?;

        sqlx::query(
            "INSERT INTO user_guild_preferences (user_id, default_guild_id, updated_at)
             VALUES ($1, NULL, CURRENT_TIMESTAMP)
             ON CONFLICT (user_id) DO NOTHING",
        )
        .bind(login.user_id.as_str())
        .execute(&mut *transaction)
        .await?;

        let rotated_from_id = if let Some(previous_hash) = &login.previous_session_token_hash {
            let previous = sqlx::query_as::<_, PreviousSessionRow>(
                "SELECT id, user_id
                 FROM user_sessions
                 WHERE token_hash = $1
                   AND revoked_at IS NULL
                   AND expires_at > CURRENT_TIMESTAMP
                 FOR UPDATE",
            )
            .bind(previous_hash)
            .fetch_optional(&mut *transaction)
            .await?;

            if let Some(previous) = previous {
                sqlx::query(
                    "UPDATE user_sessions
                     SET revoked_at = CURRENT_TIMESTAMP
                     WHERE id = $1 AND revoked_at IS NULL",
                )
                .bind(previous.id)
                .execute(&mut *transaction)
                .await?;

                (previous.user_id == login.user_id.as_str()).then_some(previous.id)
            } else {
                None
            }
        } else {
            None
        };

        sqlx::query(
            "INSERT INTO user_sessions (
                id, user_id, token_hash, csrf_secret_hash, rotated_from_id,
                created_at, last_seen_at, expires_at, revoked_at
             )
             VALUES (
                $1, $2, $3, $4, $5,
                CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, $6, NULL
             )",
        )
        .bind(login.session_id)
        .bind(login.user_id.as_str())
        .bind(&login.token_hash)
        .bind(&login.csrf_secret_hash)
        .bind(rotated_from_id)
        .bind(login.expires_at)
        .execute(&mut *transaction)
        .await?;

        transaction.commit().await?;
        Ok(())
    }

    pub async fn resolve_session(
        &self,
        token_hash: &[u8],
    ) -> Result<Option<ResolvedSession>, AuthRepositoryError> {
        let row = sqlx::query_as::<_, ResolvedSessionRow>(
            "SELECT session.id AS session_id,
                    session.user_id,
                    users.display_name,
                    users.avatar_hash,
                    CASE
                        WHEN default_member.user_id IS NOT NULL
                         AND default_guild.status = 'ACTIVE'
                        THEN preferences.default_guild_id
                        ELSE NULL
                    END AS default_guild_id,
                    session.csrf_secret_hash,
                    session.last_seen_at
             FROM user_sessions AS session
             JOIN users ON users.discord_id = session.user_id
             LEFT JOIN user_guild_preferences AS preferences
               ON preferences.user_id = session.user_id
             LEFT JOIN guild_members AS default_member
               ON default_member.guild_id = preferences.default_guild_id
              AND default_member.user_id = session.user_id
              AND default_member.status = 'ACTIVE'
             LEFT JOIN guilds AS default_guild
               ON default_guild.guild_id = preferences.default_guild_id
             WHERE session.token_hash = $1
               AND session.revoked_at IS NULL
               AND session.expires_at > CURRENT_TIMESTAMP
               AND users.deleted_at IS NULL",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;

        row.map(ResolvedSession::try_from).transpose()
    }

    pub async fn revoke_session(&self, token_hash: &[u8]) -> Result<bool, AuthRepositoryError> {
        let result = sqlx::query(
            "UPDATE user_sessions
             SET revoked_at = CURRENT_TIMESTAMP
             WHERE token_hash = $1 AND revoked_at IS NULL",
        )
        .bind(token_hash)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn touch_session(&self, session_id: Uuid) -> Result<bool, AuthRepositoryError> {
        let result = sqlx::query(
            "UPDATE user_sessions
             SET last_seen_at = CURRENT_TIMESTAMP
             WHERE id = $1
               AND revoked_at IS NULL
               AND expires_at > CURRENT_TIMESTAMP
               AND last_seen_at < CURRENT_TIMESTAMP - INTERVAL '5 minutes'",
        )
        .bind(session_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }
}

pub struct NewOAuthAttempt {
    pub id: Uuid,
    pub state_hash: Vec<u8>,
    pub pkce_verifier_ciphertext: Vec<u8>,
    pub redirect_path: String,
    pub expires_at: OffsetDateTime,
}

#[derive(Debug, FromRow)]
pub struct ConsumedOAuthAttempt {
    pub id: Uuid,
    pub pkce_verifier_ciphertext: Vec<u8>,
    pub redirect_path: String,
}

pub struct NewLogin {
    pub user_id: DiscordUserId,
    pub display_name: String,
    pub avatar_hash: Option<String>,
    pub previous_session_token_hash: Option<Vec<u8>>,
    pub session_id: Uuid,
    pub token_hash: Vec<u8>,
    pub csrf_secret_hash: Vec<u8>,
    pub expires_at: OffsetDateTime,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedSession {
    pub session_id: Uuid,
    pub user_id: DiscordUserId,
    pub display_name: String,
    pub avatar_hash: Option<String>,
    pub default_guild_id: Option<GuildId>,
    pub csrf_secret_hash: Vec<u8>,
    pub last_seen_at: OffsetDateTime,
}

#[derive(FromRow)]
struct PreviousSessionRow {
    id: Uuid,
    user_id: String,
}

#[derive(FromRow)]
struct ResolvedSessionRow {
    session_id: Uuid,
    user_id: String,
    display_name: String,
    avatar_hash: Option<String>,
    default_guild_id: Option<String>,
    csrf_secret_hash: Vec<u8>,
    last_seen_at: OffsetDateTime,
}

impl TryFrom<ResolvedSessionRow> for ResolvedSession {
    type Error = AuthRepositoryError;

    fn try_from(row: ResolvedSessionRow) -> Result<Self, Self::Error> {
        Ok(Self {
            session_id: row.session_id,
            user_id: DiscordUserId::parse(row.user_id)
                .map_err(AuthRepositoryError::InvalidStoredSnowflake)?,
            display_name: row.display_name,
            avatar_hash: row.avatar_hash,
            default_guild_id: row
                .default_guild_id
                .map(GuildId::parse)
                .transpose()
                .map_err(AuthRepositoryError::InvalidStoredSnowflake)?,
            csrf_secret_hash: row.csrf_secret_hash,
            last_seen_at: row.last_seen_at,
        })
    }
}

#[derive(Debug, Error)]
pub enum AuthRepositoryError {
    #[error("PostgreSQL authentication repository operation failed")]
    Database(#[from] sqlx::Error),
    #[error("PostgreSQL contains an invalid Discord snowflake")]
    InvalidStoredSnowflake(#[source] SnowflakeError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_session_conversion_enforces_typed_ids() {
        let valid = ResolvedSession::try_from(ResolvedSessionRow {
            session_id: Uuid::now_v7(),
            user_id: "123456789012345678".to_owned(),
            display_name: "Longcat".to_owned(),
            avatar_hash: None,
            default_guild_id: Some("223456789012345678".to_owned()),
            csrf_secret_hash: vec![0; 32],
            last_seen_at: OffsetDateTime::now_utc(),
        })
        .expect("valid row");
        assert_eq!(valid.user_id.as_str(), "123456789012345678");
        assert_eq!(
            valid.default_guild_id.as_ref().map(GuildId::as_str),
            Some("223456789012345678")
        );

        let invalid = ResolvedSession::try_from(ResolvedSessionRow {
            session_id: Uuid::now_v7(),
            user_id: "invalid".to_owned(),
            display_name: "Longcat".to_owned(),
            avatar_hash: None,
            default_guild_id: None,
            csrf_secret_hash: vec![0; 32],
            last_seen_at: OffsetDateTime::now_utc(),
        });
        assert!(matches!(
            invalid,
            Err(AuthRepositoryError::InvalidStoredSnowflake(_))
        ));
    }
}
