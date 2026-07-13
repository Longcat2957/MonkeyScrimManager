use sqlx::{FromRow, PgPool};
use thiserror::Error;

use crate::domain::{GuildId, UserId};

#[derive(Clone, Debug, Eq, FromRow, PartialEq)]
pub struct RiotAccountSuggestionRecord {
    pub platform: String,
    pub game_name: String,
    pub tag_line: String,
    pub profile_icon_id: Option<i32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildMemberSuggestionRecord {
    pub display_name: String,
    pub game_name: Option<String>,
    pub tag_line: Option<String>,
    pub profile_icon_id: Option<i32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GuildMemberSuggestionSearch {
    Allowed(Vec<GuildMemberSuggestionRecord>),
    NotMember,
}

pub async fn public_riot_accounts(
    pool: &PgPool,
    platform: &str,
    normalized_prefix: &str,
    limit: i64,
) -> Result<Vec<RiotAccountSuggestionRecord>, SearchSuggestionRepositoryError> {
    let rows = sqlx::query_as::<_, RiotAccountSuggestionRecord>(
        "WITH known_riot_ids AS (
             SELECT account.platform,
                    account.game_name,
                    account.tag_line,
                    account.normalized_game_name,
                    account.normalized_tag_line,
                    account.profile_icon_id,
                    account.updated_at AS known_at,
                    1 AS source_priority
             FROM riot_accounts AS account
             WHERE account.platform = $1
               AND account.origin = 'RIOT'
               AND (
                   starts_with(account.normalized_game_name, $2)
                   OR starts_with(
                       account.normalized_game_name || '#' || account.normalized_tag_line,
                       $2
                   )
               )

             UNION ALL

             SELECT snapshot.platform,
                    snapshot.game_name,
                    snapshot.tag_line,
                    snapshot.normalized_game_name,
                    snapshot.normalized_tag_line,
                    CASE
                        WHEN jsonb_typeof(snapshot.snapshot -> 'profileIconId') = 'number'
                        THEN CASE
                            WHEN (snapshot.snapshot ->> 'profileIconId')::numeric
                                 BETWEEN 0 AND 2147483647
                            THEN (snapshot.snapshot ->> 'profileIconId')::numeric::integer
                        END
                    END AS profile_icon_id,
                    snapshot.updated_at AS known_at,
                    0 AS source_priority
             FROM riot_records_snapshots AS snapshot
             WHERE snapshot.platform = $1
               AND (
                   starts_with(snapshot.normalized_game_name, $2)
                   OR starts_with(
                       snapshot.normalized_game_name || '#' || snapshot.normalized_tag_line,
                       $2
                   )
               )
         ),
         canonical_riot_ids AS (
             SELECT DISTINCT ON (
                        known.platform,
                        known.normalized_game_name,
                        known.normalized_tag_line
                    )
                    known.platform,
                    known.game_name,
                    known.tag_line,
                    known.normalized_game_name,
                    known.normalized_tag_line,
                    known.profile_icon_id
             FROM known_riot_ids AS known
             ORDER BY known.platform,
                      known.normalized_game_name,
                      known.normalized_tag_line,
                      known.known_at DESC,
                      known.source_priority
         )
         SELECT known.platform,
                known.game_name,
                known.tag_line,
                known.profile_icon_id
         FROM canonical_riot_ids AS known
         ORDER BY known.normalized_game_name,
                  known.normalized_tag_line,
                  known.game_name,
                  known.tag_line
         LIMIT $3",
    )
    .bind(platform)
    .bind(normalized_prefix)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

pub async fn guild_members(
    pool: &PgPool,
    viewer_id: &UserId,
    guild_id: &GuildId,
    display_name_prefix: &str,
    limit: i64,
) -> Result<GuildMemberSuggestionSearch, SearchSuggestionRepositoryError> {
    let rows = sqlx::query_as::<_, GuildMemberSuggestionRow>(
        "WITH viewer AS (
             SELECT EXISTS (
                 SELECT 1
                 FROM guild_members AS membership
                 JOIN guilds AS guild
                   ON guild.guild_id = membership.guild_id
                  AND guild.status = 'ACTIVE'
                 WHERE membership.guild_id = $2
                   AND membership.user_id = $1
                   AND membership.status = 'ACTIVE'
             ) AS has_access
         )
         SELECT viewer.has_access,
                suggestion.display_name,
                suggestion.game_name,
                suggestion.tag_line,
                suggestion.profile_icon_id
         FROM viewer
         LEFT JOIN LATERAL (
             SELECT member.display_name,
                    CASE WHEN privacy.user_id IS NOT NULL THEN account.game_name END AS game_name,
                    CASE WHEN privacy.user_id IS NOT NULL THEN account.tag_line END AS tag_line,
                    CASE
                        WHEN privacy.user_id IS NOT NULL THEN account.profile_icon_id
                    END AS profile_icon_id
             FROM guild_members AS member
             JOIN users AS app_user
               ON app_user.user_id = member.user_id
              AND app_user.deleted_at IS NULL
             LEFT JOIN riot_accounts AS account
               ON account.user_id = member.user_id
              AND account.is_main
             LEFT JOIN user_privacy_settings AS privacy
               ON privacy.user_id = member.user_id
              AND privacy.show_riot_ids
             WHERE viewer.has_access
               AND member.guild_id = $2
               AND member.status = 'ACTIVE'
               AND starts_with(lower(member.display_name), lower($3))
             ORDER BY lower(member.display_name), member.display_name, member.user_id
             LIMIT $4
         ) AS suggestion ON true",
    )
    .bind(viewer_id)
    .bind(guild_id)
    .bind(display_name_prefix)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    let Some(first) = rows.first() else {
        return Ok(GuildMemberSuggestionSearch::NotMember);
    };
    if !first.has_access {
        return Ok(GuildMemberSuggestionSearch::NotMember);
    }

    let suggestions = rows
        .into_iter()
        .filter_map(GuildMemberSuggestionRow::into_record)
        .collect();
    Ok(GuildMemberSuggestionSearch::Allowed(suggestions))
}

#[derive(Debug, FromRow)]
struct GuildMemberSuggestionRow {
    has_access: bool,
    display_name: Option<String>,
    game_name: Option<String>,
    tag_line: Option<String>,
    profile_icon_id: Option<i32>,
}

impl GuildMemberSuggestionRow {
    fn into_record(self) -> Option<GuildMemberSuggestionRecord> {
        self.display_name
            .map(|display_name| GuildMemberSuggestionRecord {
                display_name,
                game_name: self.game_name,
                tag_line: self.tag_line,
                profile_icon_id: self.profile_icon_id,
            })
    }
}

#[derive(Debug, Error)]
pub enum SearchSuggestionRepositoryError {
    #[error("PostgreSQL search suggestion repository operation failed")]
    Sql(#[from] sqlx::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_lateral_row_does_not_create_a_fake_guild_member() {
        let row = GuildMemberSuggestionRow {
            has_access: true,
            display_name: None,
            game_name: None,
            tag_line: None,
            profile_icon_id: None,
        };

        assert_eq!(row.into_record(), None);
    }

    #[test]
    fn guild_member_record_keeps_riot_fields_optional_as_a_group() {
        let hidden = GuildMemberSuggestionRow {
            has_access: true,
            display_name: Some("Alpha".to_owned()),
            game_name: None,
            tag_line: None,
            profile_icon_id: None,
        }
        .into_record()
        .expect("active member");

        assert_eq!(hidden.display_name, "Alpha");
        assert_eq!(hidden.game_name, None);
        assert_eq!(hidden.tag_line, None);
        assert_eq!(hidden.profile_icon_id, None);
    }
}
