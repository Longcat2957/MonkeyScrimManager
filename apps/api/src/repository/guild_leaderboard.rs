use sqlx::{FromRow, PgPool, Postgres};
use thiserror::Error;

use crate::domain::{DiscordUserId, GuildId, Lane, SeasonId};

#[derive(Clone, Debug, PartialEq)]
pub struct GuildLeaderboardSnapshot {
    pub season: Option<GuildLeaderboardSeason>,
    pub players: Vec<GuildLeaderboardPlayer>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildLeaderboardSeason {
    pub id: SeasonId,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GuildLeaderboardPlayer {
    pub id: DiscordUserId,
    pub name: String,
    pub tag: Option<String>,
    pub profile_icon_id: Option<i32>,
    pub composite_mmr: f64,
    pub main_role: Lane,
    pub is_me: bool,
    pub stats: Vec<GuildLeaderboardRoleStat>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GuildLeaderboardRoleStat {
    pub lane: Lane,
    pub mmr: f64,
    pub games: i32,
    pub wins: i32,
    pub champion_id: Option<String>,
}

pub async fn get(
    pool: &PgPool,
    guild_id: &GuildId,
    viewer_id: &DiscordUserId,
) -> Result<GuildLeaderboardSnapshot, GuildLeaderboardRepositoryError> {
    let mut transaction = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *transaction)
        .await?;

    let access = load_access(&mut transaction, guild_id, viewer_id).await?;
    let Some(access) = access else {
        transaction.commit().await?;
        return Err(GuildLeaderboardRepositoryError::GuildNotFound);
    };
    let season = match (access.season_id, access.season_name) {
        (Some(id), Some(name)) => Some(GuildLeaderboardSeason { id, name }),
        (None, None) => None,
        _ => return Err(GuildLeaderboardRepositoryError::StoredInvariantViolation),
    };

    let players = if let Some(season) = &season {
        let rows = load_rows(&mut transaction, guild_id, season.id).await?;
        build_players(rows, viewer_id)?
    } else {
        Vec::new()
    };

    transaction.commit().await?;
    Ok(GuildLeaderboardSnapshot { season, players })
}

async fn load_access(
    transaction: &mut sqlx::Transaction<'_, Postgres>,
    guild_id: &GuildId,
    viewer_id: &DiscordUserId,
) -> Result<Option<AccessRow>, sqlx::Error> {
    sqlx::query_as::<_, AccessRow>(
        "SELECT season.id AS season_id, season.name AS season_name
         FROM guild_members AS viewer
         JOIN guilds AS guild
           ON guild.guild_id = viewer.guild_id
          AND guild.status = 'ACTIVE'
         LEFT JOIN seasons AS season
           ON season.guild_id = viewer.guild_id
          AND season.status = 'ACTIVE'
         WHERE viewer.guild_id = $1
           AND viewer.user_id = $2
           AND viewer.status = 'ACTIVE'",
    )
    .bind(guild_id)
    .bind(viewer_id)
    .fetch_optional(&mut **transaction)
    .await
}

async fn load_rows(
    transaction: &mut sqlx::Transaction<'_, Postgres>,
    guild_id: &GuildId,
    season_id: SeasonId,
) -> Result<Vec<LeaderboardRow>, sqlx::Error> {
    sqlx::query_as::<_, LeaderboardRow>(
        "WITH champion_counts AS (
             SELECT participant.user_id,
                    participant.lane,
                    participant.champion_id,
                    count(*) AS games
             FROM games AS game
             JOIN game_participants AS participant
               ON participant.guild_id = game.guild_id
              AND participant.game_id = game.id
             WHERE game.guild_id = $1
               AND game.season_id = $2
               AND game.status = 'CONFIRMED'
               AND participant.champion_id IS NOT NULL
               AND (
                   (game.series_id IS NOT NULL AND EXISTS (
                       SELECT 1
                       FROM series AS parent_series
                       WHERE parent_series.guild_id = game.guild_id
                         AND parent_series.id = game.series_id
                         AND parent_series.status <> 'CANCELLED'
                         AND parent_series.deleted_at IS NULL
                   ))
                   OR
                   (game.auction_match_id IS NOT NULL AND EXISTS (
                       SELECT 1
                       FROM auction_matches AS parent_match
                       JOIN auction_tournaments AS parent_tournament
                         ON parent_tournament.guild_id = parent_match.guild_id
                        AND parent_tournament.id = parent_match.tournament_id
                       WHERE parent_match.guild_id = game.guild_id
                         AND parent_match.id = game.auction_match_id
                         AND parent_match.status <> 'CANCELLED'
                         AND parent_match.deleted_at IS NULL
                         AND parent_tournament.status <> 'CANCELLED'
                         AND parent_tournament.deleted_at IS NULL
                   ))
               )
             GROUP BY participant.user_id, participant.lane, participant.champion_id
         ),
         top_champions AS (
             SELECT DISTINCT ON (user_id, lane)
                    user_id, lane, champion_id
             FROM champion_counts
             ORDER BY user_id, lane, games DESC, champion_id
         )
         SELECT member.user_id,
                member.display_name AS name,
                member.main_lane,
                CASE
                    WHEN privacy.show_riot_ids IS TRUE
                    THEN main_account.game_name || '#' || main_account.tag_line
                    ELSE NULL
                END AS tag,
                CASE
                    WHEN privacy.show_riot_ids IS TRUE
                    THEN main_account.profile_icon_id
                    ELSE NULL
                END AS profile_icon_id,
                lane_mmr.lane,
                lane_mmr.mmr,
                lane_mmr.games_played AS games,
                lane_mmr.wins,
                top_champion.champion_id
         FROM user_lane_mmr AS lane_mmr
         JOIN guild_members AS member
           ON member.guild_id = lane_mmr.guild_id
          AND member.user_id = lane_mmr.user_id
          AND member.status = 'ACTIVE'
         LEFT JOIN user_privacy_settings AS privacy
           ON privacy.user_id = member.user_id
         LEFT JOIN riot_accounts AS main_account
           ON main_account.user_id = member.user_id
          AND main_account.is_main IS TRUE
         LEFT JOIN top_champions AS top_champion
           ON top_champion.user_id = lane_mmr.user_id
          AND top_champion.lane = lane_mmr.lane
         WHERE lane_mmr.guild_id = $1
           AND lane_mmr.season_id = $2
           AND lane_mmr.games_played >= 1
         ORDER BY member.user_id,
                  CASE lane_mmr.lane
                      WHEN 'TOP' THEN 1
                      WHEN 'JUNGLE' THEN 2
                      WHEN 'MID' THEN 3
                      WHEN 'BOTTOM' THEN 4
                      WHEN 'SUPPORT' THEN 5
                  END",
    )
    .bind(guild_id)
    .bind(season_id)
    .fetch_all(&mut **transaction)
    .await
}

fn build_players(
    rows: Vec<LeaderboardRow>,
    viewer_id: &DiscordUserId,
) -> Result<Vec<GuildLeaderboardPlayer>, GuildLeaderboardRepositoryError> {
    let mut players = Vec::new();
    let mut current: Option<PlayerBuilder> = None;

    for row in rows {
        validate_row(&row)?;
        let same_player = current
            .as_ref()
            .is_some_and(|builder| builder.id == row.user_id);
        if !same_player {
            if let Some(builder) = current.take() {
                players.push(builder.finish(viewer_id)?);
            }
            current = Some(PlayerBuilder::from_row(&row));
        }
        let builder = current
            .as_mut()
            .ok_or(GuildLeaderboardRepositoryError::StoredInvariantViolation)?;
        builder.push(row)?;
    }
    if let Some(builder) = current {
        players.push(builder.finish(viewer_id)?);
    }

    players.sort_by(|left, right| {
        right
            .composite_mmr
            .total_cmp(&left.composite_mmr)
            .then_with(|| left.id.as_str().cmp(right.id.as_str()))
    });
    Ok(players)
}

fn validate_row(row: &LeaderboardRow) -> Result<(), GuildLeaderboardRepositoryError> {
    if !row.mmr.is_finite()
        || row.games < 1
        || !(0..=row.games).contains(&row.wins)
        || row.profile_icon_id.is_some_and(|id| id < 0)
        || row.tag.as_ref().is_some_and(|tag| tag.trim().is_empty())
    {
        return Err(GuildLeaderboardRepositoryError::StoredInvariantViolation);
    }
    Ok(())
}

struct PlayerBuilder {
    id: DiscordUserId,
    name: String,
    tag: Option<String>,
    profile_icon_id: Option<i32>,
    configured_main_lane: Option<Lane>,
    stats: Vec<GuildLeaderboardRoleStat>,
}

impl PlayerBuilder {
    fn from_row(row: &LeaderboardRow) -> Self {
        Self {
            id: row.user_id.clone(),
            name: row.name.clone(),
            tag: row.tag.clone(),
            profile_icon_id: row.profile_icon_id,
            configured_main_lane: row.main_lane,
            stats: Vec::new(),
        }
    }

    fn push(&mut self, row: LeaderboardRow) -> Result<(), GuildLeaderboardRepositoryError> {
        if self.id != row.user_id
            || self.name != row.name
            || self.tag != row.tag
            || self.profile_icon_id != row.profile_icon_id
            || self.configured_main_lane != row.main_lane
            || self.stats.iter().any(|stat| stat.lane == row.lane)
        {
            return Err(GuildLeaderboardRepositoryError::StoredInvariantViolation);
        }
        self.stats.push(GuildLeaderboardRoleStat {
            lane: row.lane,
            mmr: row.mmr,
            games: row.games,
            wins: row.wins,
            champion_id: row.champion_id,
        });
        Ok(())
    }

    fn finish(
        self,
        viewer_id: &DiscordUserId,
    ) -> Result<GuildLeaderboardPlayer, GuildLeaderboardRepositoryError> {
        let main_role = self
            .configured_main_lane
            .filter(|lane| self.stats.iter().any(|stat| stat.lane == *lane))
            .or_else(|| {
                self.stats
                    .iter()
                    .max_by(|left, right| {
                        left.games
                            .cmp(&right.games)
                            .then_with(|| left.mmr.total_cmp(&right.mmr))
                            .then_with(|| lane_order(right.lane).cmp(&lane_order(left.lane)))
                    })
                    .map(|stat| stat.lane)
            })
            .ok_or(GuildLeaderboardRepositoryError::StoredInvariantViolation)?;
        let total_games = self
            .stats
            .iter()
            .map(|stat| i64::from(stat.games))
            .sum::<i64>();
        let weighted_mmr = self
            .stats
            .iter()
            .map(|stat| stat.mmr * f64::from(stat.games))
            .sum::<f64>();
        let composite_mmr = weighted_mmr / total_games as f64;
        if total_games < 1 || !composite_mmr.is_finite() {
            return Err(GuildLeaderboardRepositoryError::StoredInvariantViolation);
        }
        Ok(GuildLeaderboardPlayer {
            is_me: self.id == *viewer_id,
            id: self.id,
            name: self.name,
            tag: self.tag,
            profile_icon_id: self.profile_icon_id,
            composite_mmr,
            main_role,
            stats: self.stats,
        })
    }
}

const fn lane_order(lane: Lane) -> u8 {
    match lane {
        Lane::Top => 0,
        Lane::Jungle => 1,
        Lane::Mid => 2,
        Lane::Bottom => 3,
        Lane::Support => 4,
    }
}

#[derive(Debug, FromRow)]
struct AccessRow {
    season_id: Option<SeasonId>,
    season_name: Option<String>,
}

#[derive(Debug, FromRow)]
struct LeaderboardRow {
    user_id: DiscordUserId,
    name: String,
    main_lane: Option<Lane>,
    tag: Option<String>,
    profile_icon_id: Option<i32>,
    lane: Lane,
    mmr: f64,
    games: i32,
    wins: i32,
    champion_id: Option<String>,
}

#[derive(Debug, Error)]
pub enum GuildLeaderboardRepositoryError {
    #[error("guild was not found for this viewer")]
    GuildNotFound,
    #[error("stored leaderboard data violated an invariant")]
    StoredInvariantViolation,
    #[error("database operation failed: {0}")]
    Database(#[from] sqlx::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_projection_uses_played_lane_weighting_and_deterministic_main_role() {
        let viewer = DiscordUserId::parse("100000000000000001").expect("viewer ID");
        let player = DiscordUserId::parse("100000000000000002").expect("player ID");
        let rows = vec![
            row(player.clone(), None, Lane::Top, 1600.0, 2, 1),
            row(player, None, Lane::Jungle, 1500.0, 6, 4),
        ];

        let players = build_players(rows, &viewer).expect("valid projection");

        assert_eq!(players.len(), 1);
        assert_eq!(players[0].main_role, Lane::Jungle);
        assert_eq!(players[0].composite_mmr, 1525.0);
        assert!(!players[0].is_me);
    }

    fn row(
        user_id: DiscordUserId,
        main_lane: Option<Lane>,
        lane: Lane,
        mmr: f64,
        games: i32,
        wins: i32,
    ) -> LeaderboardRow {
        LeaderboardRow {
            user_id,
            name: "Player".to_owned(),
            main_lane,
            tag: None,
            profile_icon_id: None,
            lane,
            mmr,
            games,
            wins,
            champion_id: None,
        }
    }
}
