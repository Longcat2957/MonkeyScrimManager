use sqlx::{FromRow, PgPool};
use thiserror::Error;
use time::OffsetDateTime;

use crate::domain::{DiscordUserId, GuildId, SeasonId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GuildContextRole {
    Member,
    Operator,
}

impl GuildContextRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Member => "MEMBER",
            Self::Operator => "OPERATOR",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MeRecord {
    pub discord_id: DiscordUserId,
    pub display_name: String,
    pub avatar_hash: Option<String>,
    pub default_guild_id: Option<GuildId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildSummaryRecord {
    pub id: GuildId,
    pub display_name: String,
    pub icon_hash: Option<String>,
    pub member_count: i64,
    pub current_user_role: GuildContextRole,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CurrentSeasonRecord {
    pub id: SeasonId,
    pub name: String,
    pub started_at: OffsetDateTime,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildContextRecord {
    pub id: GuildId,
    pub display_name: String,
    pub icon_hash: Option<String>,
    pub member_count: i64,
    pub current_user_role: GuildContextRole,
    pub is_externally_public: bool,
    pub current_season: Option<CurrentSeasonRecord>,
}

#[derive(Clone, Debug, Eq, FromRow, PartialEq)]
pub struct DefaultGuildRecord {
    pub default_guild_id: GuildId,
}

pub async fn get_me(
    pool: &PgPool,
    user_id: &DiscordUserId,
) -> Result<Option<MeRecord>, ContextRepositoryError> {
    let row = sqlx::query_as::<_, MeRow>(
        "SELECT app_user.discord_id,
                app_user.display_name,
                app_user.avatar_hash,
                CASE
                    WHEN default_guild.guild_id IS NOT NULL
                     AND default_membership.user_id IS NOT NULL
                    THEN preference.default_guild_id
                    ELSE NULL
                END AS default_guild_id
         FROM users AS app_user
         LEFT JOIN user_guild_preferences AS preference
           ON preference.user_id = app_user.discord_id
         LEFT JOIN guilds AS default_guild
           ON default_guild.guild_id = preference.default_guild_id
          AND default_guild.status = 'ACTIVE'
         LEFT JOIN guild_members AS default_membership
           ON default_membership.guild_id = preference.default_guild_id
          AND default_membership.user_id = app_user.discord_id
          AND default_membership.status = 'ACTIVE'
         WHERE app_user.discord_id = $1
           AND app_user.deleted_at IS NULL",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(MeRow::into_record))
}

pub async fn list_guilds(
    pool: &PgPool,
    user_id: &DiscordUserId,
) -> Result<Vec<GuildSummaryRecord>, ContextRepositoryError> {
    let rows = sqlx::query_as::<_, GuildSummaryRow>(
        "SELECT guild.guild_id AS id,
                COALESCE(NULLIF(config.display_name, ''), guild.discord_name) AS display_name,
                guild.icon_hash,
                (
                    SELECT count(*)
                    FROM guild_members AS counted_member
                    WHERE counted_member.guild_id = guild.guild_id
                      AND counted_member.status = 'ACTIVE'
                ) AS member_count,
                EXISTS (
                    SELECT 1
                    FROM guild_member_discord_roles AS member_role
                    WHERE member_role.guild_id = guild.guild_id
                      AND member_role.user_id = $1
                      AND member_role.role_id = config.operator_role_id
                ) AS is_operator
         FROM guild_members AS current_membership
         JOIN guilds AS guild
           ON guild.guild_id = current_membership.guild_id
          AND guild.status = 'ACTIVE'
         LEFT JOIN guild_config AS config
           ON config.guild_id = guild.guild_id
         WHERE current_membership.user_id = $1
           AND current_membership.status = 'ACTIVE'
         ORDER BY COALESCE(NULLIF(config.display_name, ''), guild.discord_name), guild.guild_id",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(GuildSummaryRow::into_record).collect())
}

pub async fn get_guild(
    pool: &PgPool,
    user_id: &DiscordUserId,
    guild_id: &GuildId,
) -> Result<Option<GuildContextRecord>, ContextRepositoryError> {
    let row = sqlx::query_as::<_, GuildContextRow>(
        "SELECT guild.guild_id AS id,
                COALESCE(NULLIF(config.display_name, ''), guild.discord_name) AS display_name,
                guild.icon_hash,
                (
                    SELECT count(*)
                    FROM guild_members AS counted_member
                    WHERE counted_member.guild_id = $2
                      AND counted_member.status = 'ACTIVE'
                ) AS member_count,
                EXISTS (
                    SELECT 1
                    FROM guild_member_discord_roles AS member_role
                    WHERE member_role.guild_id = $2
                      AND member_role.user_id = $1
                      AND member_role.role_id = config.operator_role_id
                ) AS is_operator,
                COALESCE(config.is_externally_public, false) AS is_externally_public,
                current_season.id AS current_season_id,
                current_season.name AS current_season_name,
                current_season.started_at AS current_season_started_at
         FROM guild_members AS current_membership
         JOIN guilds AS guild
           ON guild.guild_id = current_membership.guild_id
          AND guild.guild_id = $2
          AND guild.status = 'ACTIVE'
         LEFT JOIN guild_config AS config
           ON config.guild_id = guild.guild_id
         LEFT JOIN seasons AS current_season
           ON current_season.guild_id = $2
          AND current_season.status = 'ACTIVE'
         WHERE current_membership.guild_id = $2
           AND current_membership.user_id = $1
           AND current_membership.status = 'ACTIVE'",
    )
    .bind(user_id)
    .bind(guild_id)
    .fetch_optional(pool)
    .await?;

    row.map(GuildContextRow::into_record).transpose()
}

pub async fn set_default_guild(
    pool: &PgPool,
    user_id: &DiscordUserId,
    guild_id: &GuildId,
) -> Result<Option<DefaultGuildRecord>, ContextRepositoryError> {
    let row = sqlx::query_as::<_, DefaultGuildRecord>(
        "INSERT INTO user_guild_preferences (user_id, default_guild_id, updated_at)
         SELECT app_user.discord_id, guild.guild_id, clock_timestamp()
         FROM users AS app_user
         JOIN guild_members AS current_membership
           ON current_membership.user_id = app_user.discord_id
          AND current_membership.guild_id = $2
          AND current_membership.status = 'ACTIVE'
         JOIN guilds AS guild
           ON guild.guild_id = current_membership.guild_id
          AND guild.guild_id = $2
          AND guild.status = 'ACTIVE'
         WHERE app_user.discord_id = $1
           AND app_user.deleted_at IS NULL
         ON CONFLICT (user_id) DO UPDATE
         SET default_guild_id = EXCLUDED.default_guild_id,
             updated_at = EXCLUDED.updated_at
         RETURNING default_guild_id",
    )
    .bind(user_id)
    .bind(guild_id)
    .fetch_optional(pool)
    .await?;

    Ok(row)
}

#[derive(Debug, FromRow)]
struct MeRow {
    discord_id: DiscordUserId,
    display_name: String,
    avatar_hash: Option<String>,
    default_guild_id: Option<GuildId>,
}

impl MeRow {
    fn into_record(self) -> MeRecord {
        MeRecord {
            discord_id: self.discord_id,
            display_name: self.display_name,
            avatar_hash: self.avatar_hash,
            default_guild_id: self.default_guild_id,
        }
    }
}

#[derive(Debug, FromRow)]
struct GuildSummaryRow {
    id: GuildId,
    display_name: String,
    icon_hash: Option<String>,
    member_count: i64,
    is_operator: bool,
}

impl GuildSummaryRow {
    fn into_record(self) -> GuildSummaryRecord {
        GuildSummaryRecord {
            id: self.id,
            display_name: self.display_name,
            icon_hash: self.icon_hash,
            member_count: self.member_count,
            current_user_role: role_from_operator(self.is_operator),
        }
    }
}

#[derive(Debug, FromRow)]
struct GuildContextRow {
    id: GuildId,
    display_name: String,
    icon_hash: Option<String>,
    member_count: i64,
    is_operator: bool,
    is_externally_public: bool,
    current_season_id: Option<SeasonId>,
    current_season_name: Option<String>,
    current_season_started_at: Option<OffsetDateTime>,
}

impl GuildContextRow {
    fn into_record(self) -> Result<GuildContextRecord, ContextRepositoryError> {
        let current_season = match (
            self.current_season_id,
            self.current_season_name,
            self.current_season_started_at,
        ) {
            (Some(id), Some(name), Some(started_at)) => Some(CurrentSeasonRecord {
                id,
                name,
                started_at,
            }),
            (None, None, None) => None,
            _ => return Err(ContextRepositoryError::IncompleteCurrentSeason),
        };

        Ok(GuildContextRecord {
            id: self.id,
            display_name: self.display_name,
            icon_hash: self.icon_hash,
            member_count: self.member_count,
            current_user_role: role_from_operator(self.is_operator),
            is_externally_public: self.is_externally_public,
            current_season,
        })
    }
}

fn role_from_operator(is_operator: bool) -> GuildContextRole {
    if is_operator {
        GuildContextRole::Operator
    } else {
        GuildContextRole::Member
    }
}

#[derive(Debug, Error)]
pub enum ContextRepositoryError {
    #[error("active season projection is incomplete")]
    IncompleteCurrentSeason,
    #[error("PostgreSQL guild context operation failed")]
    Sql(#[from] sqlx::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guild_id() -> GuildId {
        GuildId::parse("123456789012345678").expect("valid guild ID")
    }

    #[test]
    fn operator_flag_maps_to_wire_role_without_localized_labels() {
        assert_eq!(role_from_operator(false), GuildContextRole::Member);
        assert_eq!(role_from_operator(true), GuildContextRole::Operator);
        assert_eq!(GuildContextRole::Member.as_str(), "MEMBER");
        assert_eq!(GuildContextRole::Operator.as_str(), "OPERATOR");
    }

    #[test]
    fn guild_context_maps_absent_active_season_to_none() {
        let record = GuildContextRow {
            id: guild_id(),
            display_name: "MonkeyBot".to_owned(),
            icon_hash: None,
            member_count: 1,
            is_operator: false,
            is_externally_public: false,
            current_season_id: None,
            current_season_name: None,
            current_season_started_at: None,
        }
        .into_record()
        .expect("absent season should map");

        assert_eq!(record.current_season, None);
    }

    #[test]
    fn guild_context_rejects_an_incomplete_active_season() {
        let result = GuildContextRow {
            id: guild_id(),
            display_name: "MonkeyBot".to_owned(),
            icon_hash: None,
            member_count: 1,
            is_operator: true,
            is_externally_public: true,
            current_season_id: Some(SeasonId::new()),
            current_season_name: None,
            current_season_started_at: None,
        }
        .into_record();

        assert!(matches!(
            result,
            Err(ContextRepositoryError::IncompleteCurrentSeason)
        ));
    }
}
