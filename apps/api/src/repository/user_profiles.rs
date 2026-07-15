use sqlx::{FromRow, PgPool};
use thiserror::Error;

use crate::domain::DiscordUserId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicUserProfile {
    pub discord_id: DiscordUserId,
    pub display_name: String,
    pub active_guild_count: u64,
    pub season_count: u64,
}

pub async fn find_public(
    pool: &PgPool,
    discord_id: &DiscordUserId,
) -> Result<Option<PublicUserProfile>, UserProfileRepositoryError> {
    let row = sqlx::query_as::<_, PublicUserProfileRow>(
        "SELECT identity.discord_id,
                user_record.display_name,
                count(DISTINCT guild.guild_id) AS active_guild_count,
                count(DISTINCT season.id) AS season_count
         FROM user_discord_identities AS identity
         JOIN users AS user_record
           ON user_record.user_id = identity.user_id
          AND user_record.identity_kind = 'DISCORD'
         LEFT JOIN user_privacy_settings AS privacy
           ON privacy.user_id = user_record.user_id
         LEFT JOIN guild_members AS member
           ON member.user_id = user_record.user_id
          AND member.status = 'ACTIVE'
         LEFT JOIN guilds AS guild
           ON guild.guild_id = member.guild_id
          AND guild.status = 'ACTIVE'
         LEFT JOIN seasons AS season
           ON season.guild_id = guild.guild_id
         WHERE identity.discord_id = $1
           AND identity.identity_kind = 'DISCORD'
           AND user_record.deleted_at IS NULL
           AND COALESCE(privacy.public_profile_enabled, true)
         GROUP BY identity.discord_id, user_record.display_name",
    )
    .bind(discord_id)
    .fetch_optional(pool)
    .await?;

    row.map(PublicUserProfile::try_from).transpose()
}

#[derive(FromRow)]
struct PublicUserProfileRow {
    discord_id: String,
    display_name: String,
    active_guild_count: i64,
    season_count: i64,
}

impl TryFrom<PublicUserProfileRow> for PublicUserProfile {
    type Error = UserProfileRepositoryError;

    fn try_from(row: PublicUserProfileRow) -> Result<Self, Self::Error> {
        let discord_id = DiscordUserId::parse(row.discord_id)
            .map_err(|_| UserProfileRepositoryError::StoredInvariantViolation)?;
        let active_guild_count = u64::try_from(row.active_guild_count)
            .map_err(|_| UserProfileRepositoryError::StoredInvariantViolation)?;
        let season_count = u64::try_from(row.season_count)
            .map_err(|_| UserProfileRepositoryError::StoredInvariantViolation)?;
        if row.display_name.trim().is_empty() {
            return Err(UserProfileRepositoryError::StoredInvariantViolation);
        }

        Ok(Self {
            discord_id,
            display_name: row.display_name,
            active_guild_count,
            season_count,
        })
    }
}

#[derive(Debug, Error)]
pub enum UserProfileRepositoryError {
    #[error("PostgreSQL user profile repository operation failed")]
    Database(#[from] sqlx::Error),
    #[error("stored public user profile violates an invariant")]
    StoredInvariantViolation,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_stored_public_profile_rows() {
        let invalid_id = PublicUserProfileRow {
            discord_id: "not-a-discord-id".to_owned(),
            display_name: "Longcat".to_owned(),
            active_guild_count: 1,
            season_count: 2,
        };
        assert!(matches!(
            PublicUserProfile::try_from(invalid_id),
            Err(UserProfileRepositoryError::StoredInvariantViolation)
        ));

        let negative_count = PublicUserProfileRow {
            discord_id: "1525847677542207548".to_owned(),
            display_name: "Longcat".to_owned(),
            active_guild_count: -1,
            season_count: 2,
        };
        assert!(matches!(
            PublicUserProfile::try_from(negative_count),
            Err(UserProfileRepositoryError::StoredInvariantViolation)
        ));
    }
}
