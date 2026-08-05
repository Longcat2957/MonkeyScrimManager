use std::cmp::Ordering;

use serde::Serialize;
use sqlx::{AssertSqlSafe, FromRow, PgPool, Postgres};
use thiserror::Error;

use crate::domain::{GuildId, Lane, SeasonId, SeasonStatus, UserId};

const TOP_ITEMS: usize = 10;
const MINIMUM_WIN_RATE_GAMES: i64 = 3;

const VALID_GAMES_CTE: &str = r#"
WITH valid_games AS (
    SELECT game.id,
           game.side_preset,
           game.winning_side,
           game.duration_seconds,
           game.series_id IS NOT NULL AS is_regular,
           CASE
               WHEN (game.side_preset = 'DEFAULT' AND game.winning_side = 'BLUE')
                 OR (game.side_preset = 'SWAPPED' AND game.winning_side = 'RED')
               THEN 'TEAM_1'
               ELSE 'TEAM_2'
           END AS winning_team
      FROM games AS game
 LEFT JOIN series AS parent_series
        ON parent_series.guild_id = game.guild_id
       AND parent_series.id = game.series_id
 LEFT JOIN auction_matches AS auction_match
        ON auction_match.guild_id = game.guild_id
       AND auction_match.id = game.auction_match_id
 LEFT JOIN auction_tournaments AS auction_tournament
        ON auction_tournament.guild_id = game.guild_id
       AND auction_tournament.id = auction_match.tournament_id
     WHERE game.guild_id = $1
       AND game.season_id = $2
       AND game.status = 'CONFIRMED'
       AND (
           (game.series_id IS NOT NULL
            AND parent_series.status <> 'CANCELLED'
            AND parent_series.deleted_at IS NULL)
           OR
           (game.auction_match_id IS NOT NULL
            AND auction_match.status <> 'CANCELLED'
            AND auction_match.deleted_at IS NULL
            AND auction_tournament.status <> 'CANCELLED'
            AND auction_tournament.deleted_at IS NULL)
       )
       AND (
           $3 = 'ALL'
           OR ($3 = 'REGULAR' AND game.series_id IS NOT NULL)
           OR ($3 = 'AUCTION' AND game.auction_match_id IS NOT NULL)
       )
)
"#;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum GuildStatisticsScope {
    All,
    Regular,
    Auction,
}

impl GuildStatisticsScope {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::All => "ALL",
            Self::Regular => "REGULAR",
            Self::Auction => "AUCTION",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "ALL" => Some(Self::All),
            "REGULAR" => Some(Self::Regular),
            "AUCTION" => Some(Self::Auction),
            _ => None,
        }
    }

    const fn includes_auction(self) -> bool {
        matches!(self, Self::All | Self::Auction)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildStatisticsSnapshot {
    pub seasons: Vec<GuildStatisticsSeason>,
    pub selected_season_id: Option<SeasonId>,
    pub scope: GuildStatisticsScope,
    pub overview: GuildStatisticsOverview,
    pub players: GuildStatisticsPlayers,
    pub relationships: GuildStatisticsRelationships,
    pub champions: GuildStatisticsChampions,
    pub auction: GuildStatisticsAuction,
}

#[derive(Clone, Debug, Eq, FromRow, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildStatisticsSeason {
    pub id: SeasonId,
    pub name: String,
    pub status: SeasonStatus,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildStatisticsOverview {
    pub games: i64,
    pub regular_games: i64,
    pub auction_games: i64,
    pub unique_players: i64,
    pub blue_wins: i64,
    pub red_wins: i64,
    pub average_duration_seconds: Option<f64>,
    pub coverage: GuildStatisticsCoverage,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildStatisticsCoverage {
    pub duration_games: i64,
    pub participant_stat_games: i64,
    pub champion_pick_games: i64,
    pub champion_ban_games: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildStatisticsPlayers {
    pub minimum_win_rate_games: i64,
    pub most_active: Vec<GuildStatisticsPlayer>,
    pub best_win_rates: Vec<GuildStatisticsPlayer>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildStatisticsPlayer {
    pub user_id: UserId,
    pub name: String,
    pub games: i64,
    pub wins: i64,
    pub losses: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildStatisticsRelationships {
    pub best_duos: Vec<GuildStatisticsDuo>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildStatisticsDuo {
    pub user_a_id: UserId,
    pub user_a_name: String,
    pub user_b_id: UserId,
    pub user_b_name: String,
    pub games: i64,
    pub wins: i64,
    pub losses: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildStatisticsChampions {
    pub picks: Vec<GuildStatisticsChampionPick>,
    pub bans: Vec<GuildStatisticsChampionBan>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildStatisticsChampionPick {
    pub champion_id: String,
    pub lane: Lane,
    pub games: i64,
    pub wins: i64,
    pub losses: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildStatisticsChampionBan {
    pub champion_id: String,
    pub bans: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildStatisticsAuction {
    pub completed_tournaments: i64,
    pub captains: Vec<GuildStatisticsAuctionCaptain>,
    pub top_bids: Vec<GuildStatisticsAuctionBid>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildStatisticsAuctionCaptain {
    pub user_id: UserId,
    pub name: String,
    pub teams_captained: i64,
    pub titles: i64,
    pub final_appearances: i64,
    pub match_wins: i64,
    pub match_losses: i64,
    pub game_wins: i64,
    pub game_losses: i64,
    pub total_spent_points: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildStatisticsAuctionBid {
    pub tournament_id: String,
    pub tournament_title: String,
    pub team_name: String,
    pub user_id: UserId,
    pub name: String,
    pub points: i32,
    pub games: i64,
    pub wins: i64,
    pub losses: i64,
    pub won_tournament: bool,
}

pub async fn get(
    pool: &PgPool,
    guild_id: &GuildId,
    viewer_id: &UserId,
    requested_season_id: Option<SeasonId>,
    scope: GuildStatisticsScope,
) -> Result<GuildStatisticsSnapshot, GuildStatisticsRepositoryError> {
    let mut transaction = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *transaction)
        .await?;

    if !has_access(&mut transaction, guild_id, viewer_id).await? {
        transaction.commit().await?;
        return Err(GuildStatisticsRepositoryError::GuildNotFound);
    }

    let seasons = load_seasons(&mut transaction, guild_id).await?;
    let selected_season_id = match requested_season_id {
        Some(requested) if seasons.iter().any(|season| season.id == requested) => Some(requested),
        Some(_) => {
            transaction.commit().await?;
            return Err(GuildStatisticsRepositoryError::SeasonNotFound);
        }
        None => seasons
            .iter()
            .find(|season| season.status == SeasonStatus::Active)
            .map(|season| season.id),
    };

    let (overview, players, relationships, champions, auction) =
        if let Some(season_id) = selected_season_id {
            let overview = load_overview(&mut transaction, guild_id, season_id, scope).await?;
            let players = load_players(&mut transaction, guild_id, season_id, scope).await?;
            let relationships =
                load_relationships(&mut transaction, guild_id, season_id, scope).await?;
            let champions = load_champions(&mut transaction, guild_id, season_id, scope).await?;
            let auction = if scope.includes_auction() {
                load_auction(&mut transaction, guild_id, season_id).await?
            } else {
                GuildStatisticsAuction::empty()
            };
            (overview, players, relationships, champions, auction)
        } else {
            (
                GuildStatisticsOverview::empty(),
                GuildStatisticsPlayers::empty(),
                GuildStatisticsRelationships::empty(),
                GuildStatisticsChampions::empty(),
                GuildStatisticsAuction::empty(),
            )
        };

    transaction.commit().await?;
    Ok(GuildStatisticsSnapshot {
        seasons,
        selected_season_id,
        scope,
        overview,
        players,
        relationships,
        champions,
        auction,
    })
}

impl GuildStatisticsOverview {
    fn empty() -> Self {
        Self {
            games: 0,
            regular_games: 0,
            auction_games: 0,
            unique_players: 0,
            blue_wins: 0,
            red_wins: 0,
            average_duration_seconds: None,
            coverage: GuildStatisticsCoverage {
                duration_games: 0,
                participant_stat_games: 0,
                champion_pick_games: 0,
                champion_ban_games: 0,
            },
        }
    }
}

impl GuildStatisticsPlayers {
    fn empty() -> Self {
        Self {
            minimum_win_rate_games: MINIMUM_WIN_RATE_GAMES,
            most_active: Vec::new(),
            best_win_rates: Vec::new(),
        }
    }
}

impl GuildStatisticsRelationships {
    fn empty() -> Self {
        Self {
            best_duos: Vec::new(),
        }
    }
}

impl GuildStatisticsChampions {
    fn empty() -> Self {
        Self {
            picks: Vec::new(),
            bans: Vec::new(),
        }
    }
}

impl GuildStatisticsAuction {
    fn empty() -> Self {
        Self {
            completed_tournaments: 0,
            captains: Vec::new(),
            top_bids: Vec::new(),
        }
    }
}

async fn has_access(
    transaction: &mut sqlx::Transaction<'_, Postgres>,
    guild_id: &GuildId,
    viewer_id: &UserId,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1
             FROM guild_members AS member
             JOIN guilds AS guild
               ON guild.guild_id = member.guild_id
              AND guild.status = 'ACTIVE'
             WHERE member.guild_id = $1
               AND member.user_id = $2
               AND member.status = 'ACTIVE'
         )",
    )
    .bind(guild_id)
    .bind(viewer_id)
    .fetch_one(&mut **transaction)
    .await
}

async fn load_seasons(
    transaction: &mut sqlx::Transaction<'_, Postgres>,
    guild_id: &GuildId,
) -> Result<Vec<GuildStatisticsSeason>, GuildStatisticsRepositoryError> {
    let seasons = sqlx::query_as::<_, GuildStatisticsSeason>(
        "SELECT id, name, status
         FROM seasons
         WHERE guild_id = $1
         ORDER BY (status = 'ACTIVE') DESC, started_at DESC, id DESC",
    )
    .bind(guild_id)
    .fetch_all(&mut **transaction)
    .await?;
    if seasons.iter().any(|season| season.name.trim().is_empty()) {
        return Err(GuildStatisticsRepositoryError::StoredInvariantViolation);
    }
    Ok(seasons)
}

async fn load_overview(
    transaction: &mut sqlx::Transaction<'_, Postgres>,
    guild_id: &GuildId,
    season_id: SeasonId,
    scope: GuildStatisticsScope,
) -> Result<GuildStatisticsOverview, GuildStatisticsRepositoryError> {
    let statement = format!(
        "{VALID_GAMES_CTE}
         SELECT count(*) AS games,
                count(*) FILTER (WHERE game.is_regular) AS regular_games,
                count(*) FILTER (WHERE NOT game.is_regular) AS auction_games,
                (SELECT count(DISTINCT participant.user_id)
                   FROM valid_games AS player_game
                   JOIN game_participants AS participant
                     ON participant.guild_id = $1
                    AND participant.game_id = player_game.id) AS unique_players,
                count(*) FILTER (WHERE game.winning_side = 'BLUE') AS blue_wins,
                count(*) FILTER (WHERE game.winning_side = 'RED') AS red_wins,
                AVG(game.duration_seconds)::double precision AS average_duration_seconds,
                count(*) FILTER (WHERE game.duration_seconds IS NOT NULL) AS duration_games,
                count(*) FILTER (WHERE EXISTS (
                    SELECT 1 FROM game_participant_stats AS stat
                    WHERE stat.guild_id = $1 AND stat.game_id = game.id
                )) AS participant_stat_games,
                count(*) FILTER (WHERE EXISTS (
                    SELECT 1 FROM game_participants AS participant
                    WHERE participant.guild_id = $1
                      AND participant.game_id = game.id
                      AND participant.champion_id IS NOT NULL
                )) AS champion_pick_games,
                count(*) FILTER (WHERE EXISTS (
                    SELECT 1 FROM game_bans AS ban
                    WHERE ban.guild_id = $1 AND ban.game_id = game.id
                )) AS champion_ban_games
           FROM valid_games AS game"
    );
    // Both fragments are compile-time SQL constants. Runtime values are bound below.
    let row = sqlx::query_as::<_, OverviewRow>(AssertSqlSafe(statement))
        .bind(guild_id)
        .bind(season_id)
        .bind(scope.as_str())
        .fetch_one(&mut **transaction)
        .await?;
    validate_overview(&row)?;
    Ok(GuildStatisticsOverview {
        games: row.games,
        regular_games: row.regular_games,
        auction_games: row.auction_games,
        unique_players: row.unique_players,
        blue_wins: row.blue_wins,
        red_wins: row.red_wins,
        average_duration_seconds: row.average_duration_seconds,
        coverage: GuildStatisticsCoverage {
            duration_games: row.duration_games,
            participant_stat_games: row.participant_stat_games,
            champion_pick_games: row.champion_pick_games,
            champion_ban_games: row.champion_ban_games,
        },
    })
}

async fn load_players(
    transaction: &mut sqlx::Transaction<'_, Postgres>,
    guild_id: &GuildId,
    season_id: SeasonId,
    scope: GuildStatisticsScope,
) -> Result<GuildStatisticsPlayers, GuildStatisticsRepositoryError> {
    let statement = format!(
        "{VALID_GAMES_CTE}
         SELECT participant.user_id,
                member.display_name AS name,
                count(*) AS games,
                count(*) FILTER (WHERE participant.team = game.winning_team) AS wins
           FROM valid_games AS game
           JOIN game_participants AS participant
             ON participant.guild_id = $1
            AND participant.game_id = game.id
           JOIN guild_members AS member
             ON member.guild_id = participant.guild_id
            AND member.user_id = participant.user_id
          GROUP BY participant.user_id, member.display_name"
    );
    // Both fragments are compile-time SQL constants. Runtime values are bound below.
    let rows = sqlx::query_as::<_, PlayerRow>(AssertSqlSafe(statement))
        .bind(guild_id)
        .bind(season_id)
        .bind(scope.as_str())
        .fetch_all(&mut **transaction)
        .await?;
    let players = rows
        .into_iter()
        .map(PlayerRow::into_snapshot)
        .collect::<Result<Vec<_>, _>>()?;

    let mut most_active = players.clone();
    most_active.sort_by(|left, right| {
        right
            .games
            .cmp(&left.games)
            .then_with(|| right.wins.cmp(&left.wins))
            .then_with(|| left.user_id.as_str().cmp(right.user_id.as_str()))
    });
    most_active.truncate(TOP_ITEMS);

    let mut best_win_rates = players
        .into_iter()
        .filter(|player| player.games >= MINIMUM_WIN_RATE_GAMES)
        .collect::<Vec<_>>();
    best_win_rates.sort_by(compare_win_rates);
    best_win_rates.truncate(TOP_ITEMS);

    Ok(GuildStatisticsPlayers {
        minimum_win_rate_games: MINIMUM_WIN_RATE_GAMES,
        most_active,
        best_win_rates,
    })
}

async fn load_relationships(
    transaction: &mut sqlx::Transaction<'_, Postgres>,
    guild_id: &GuildId,
    season_id: SeasonId,
    scope: GuildStatisticsScope,
) -> Result<GuildStatisticsRelationships, GuildStatisticsRepositoryError> {
    let statement = format!(
        "{VALID_GAMES_CTE}
         SELECT user_a.user_id AS user_a_id,
                member_a.display_name AS user_a_name,
                user_b.user_id AS user_b_id,
                member_b.display_name AS user_b_name,
                count(*) AS games,
                count(*) FILTER (WHERE user_a.team = game.winning_team) AS wins
           FROM valid_games AS game
           JOIN game_participants AS user_a
             ON user_a.guild_id = $1
            AND user_a.game_id = game.id
           JOIN game_participants AS user_b
             ON user_b.guild_id = user_a.guild_id
            AND user_b.game_id = user_a.game_id
            AND user_b.team = user_a.team
            AND user_a.user_id < user_b.user_id
           JOIN guild_members AS member_a
             ON member_a.guild_id = user_a.guild_id
            AND member_a.user_id = user_a.user_id
           JOIN guild_members AS member_b
             ON member_b.guild_id = user_b.guild_id
            AND member_b.user_id = user_b.user_id
          GROUP BY user_a.user_id, member_a.display_name, user_b.user_id, member_b.display_name
          ORDER BY games DESC, wins DESC, user_a.user_id, user_b.user_id
          LIMIT 10"
    );
    // Both fragments are compile-time SQL constants. Runtime values are bound below.
    let rows = sqlx::query_as::<_, DuoRow>(AssertSqlSafe(statement))
        .bind(guild_id)
        .bind(season_id)
        .bind(scope.as_str())
        .fetch_all(&mut **transaction)
        .await?;
    let best_duos = rows
        .into_iter()
        .map(DuoRow::into_snapshot)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(GuildStatisticsRelationships { best_duos })
}

async fn load_champions(
    transaction: &mut sqlx::Transaction<'_, Postgres>,
    guild_id: &GuildId,
    season_id: SeasonId,
    scope: GuildStatisticsScope,
) -> Result<GuildStatisticsChampions, GuildStatisticsRepositoryError> {
    let picks_statement = format!(
        "{VALID_GAMES_CTE}
         SELECT participant.champion_id,
                participant.lane,
                count(*) AS games,
                count(*) FILTER (WHERE participant.team = game.winning_team) AS wins
           FROM valid_games AS game
           JOIN game_participants AS participant
             ON participant.guild_id = $1
            AND participant.game_id = game.id
          WHERE participant.champion_id IS NOT NULL
          GROUP BY participant.champion_id, participant.lane
          ORDER BY games DESC, wins DESC, participant.champion_id, participant.lane
          LIMIT 10"
    );
    // Both fragments are compile-time SQL constants. Runtime values are bound below.
    let pick_rows = sqlx::query_as::<_, ChampionPickRow>(AssertSqlSafe(picks_statement))
        .bind(guild_id)
        .bind(season_id)
        .bind(scope.as_str())
        .fetch_all(&mut **transaction)
        .await?;
    let picks = pick_rows
        .into_iter()
        .map(ChampionPickRow::into_snapshot)
        .collect::<Result<Vec<_>, _>>()?;

    let bans_statement = format!(
        "{VALID_GAMES_CTE}
         SELECT ban.champion_id, count(*) AS bans
           FROM valid_games AS game
           JOIN game_bans AS ban
             ON ban.guild_id = $1
            AND ban.game_id = game.id
          GROUP BY ban.champion_id
          ORDER BY bans DESC, ban.champion_id
          LIMIT 10"
    );
    // Both fragments are compile-time SQL constants. Runtime values are bound below.
    let ban_rows = sqlx::query_as::<_, ChampionBanRow>(AssertSqlSafe(bans_statement))
        .bind(guild_id)
        .bind(season_id)
        .bind(scope.as_str())
        .fetch_all(&mut **transaction)
        .await?;
    let bans = ban_rows
        .into_iter()
        .map(ChampionBanRow::into_snapshot)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(GuildStatisticsChampions { picks, bans })
}

async fn load_auction(
    transaction: &mut sqlx::Transaction<'_, Postgres>,
    guild_id: &GuildId,
    season_id: SeasonId,
) -> Result<GuildStatisticsAuction, GuildStatisticsRepositoryError> {
    let completed_tournaments = sqlx::query_scalar::<_, i64>(
        "SELECT count(*)
         FROM auction_tournaments
         WHERE guild_id = $1
           AND season_id = $2
           AND status = 'COMPLETED'
           AND deleted_at IS NULL",
    )
    .bind(guild_id)
    .bind(season_id)
    .fetch_one(&mut **transaction)
    .await?;
    let captains = load_auction_captains(transaction, guild_id, season_id).await?;
    let top_bids = load_auction_top_bids(transaction, guild_id, season_id).await?;
    Ok(GuildStatisticsAuction {
        completed_tournaments,
        captains,
        top_bids,
    })
}

async fn load_auction_captains(
    transaction: &mut sqlx::Transaction<'_, Postgres>,
    guild_id: &GuildId,
    season_id: SeasonId,
) -> Result<Vec<GuildStatisticsAuctionCaptain>, GuildStatisticsRepositoryError> {
    let rows = sqlx::query_as::<_, AuctionCaptainRow>(
        "WITH captain_teams AS (
             SELECT tournament.id AS tournament_id,
                    team.id AS team_id,
                    captain.user_id,
                    member.display_name AS name,
                    tournament.champion_team_id,
                    (team.initial_points - team.current_points)::bigint AS spent_points
             FROM auction_tournaments AS tournament
             JOIN auction_teams AS team
               ON team.guild_id = tournament.guild_id
              AND team.tournament_id = tournament.id
             JOIN auction_team_members AS captain
               ON captain.guild_id = team.guild_id
              AND captain.tournament_id = team.tournament_id
              AND captain.team_id = team.id
              AND captain.acquired_via = 'CAPTAIN'
              AND captain.released_at IS NULL
             JOIN guild_members AS member
               ON member.guild_id = captain.guild_id
              AND member.user_id = captain.user_id
             WHERE tournament.guild_id = $1
               AND tournament.season_id = $2
               AND tournament.status = 'COMPLETED'
               AND tournament.deleted_at IS NULL
         ),
         team_stats AS (
             SELECT user_id,
                    name,
                    count(*)::bigint AS teams_captained,
                    count(*) FILTER (WHERE champion_team_id = team_id)::bigint AS titles,
                    sum(spent_points)::bigint AS total_spent_points
             FROM captain_teams
             GROUP BY user_id, name
         ),
         match_stats AS (
             SELECT captain.user_id,
                    count(*)::bigint AS matches,
                    count(*) FILTER (WHERE game_match.winner_team_id = captain.team_id)::bigint AS match_wins,
                    count(*) FILTER (WHERE game_match.match_key = 'FINAL')::bigint AS final_appearances
             FROM captain_teams AS captain
             JOIN auction_matches AS game_match
               ON game_match.guild_id = $1
              AND game_match.tournament_id = captain.tournament_id
              AND (game_match.left_team_id = captain.team_id OR game_match.right_team_id = captain.team_id)
             WHERE game_match.status = 'COMPLETED'
               AND game_match.deleted_at IS NULL
             GROUP BY captain.user_id
         ),
         game_stats AS (
             SELECT captain.user_id,
                    count(*)::bigint AS games,
                    count(*) FILTER (WHERE
                        (game_match.left_team_id = captain.team_id AND (
                            (game.side_preset = 'DEFAULT' AND game.winning_side = 'BLUE') OR
                            (game.side_preset = 'SWAPPED' AND game.winning_side = 'RED')
                        ))
                        OR
                        (game_match.right_team_id = captain.team_id AND (
                            (game.side_preset = 'DEFAULT' AND game.winning_side = 'RED') OR
                            (game.side_preset = 'SWAPPED' AND game.winning_side = 'BLUE')
                        ))
                    )::bigint AS game_wins
             FROM captain_teams AS captain
             JOIN auction_matches AS game_match
               ON game_match.guild_id = $1
              AND game_match.tournament_id = captain.tournament_id
              AND (game_match.left_team_id = captain.team_id OR game_match.right_team_id = captain.team_id)
             JOIN games AS game
               ON game.guild_id = game_match.guild_id
              AND game.auction_match_id = game_match.id
              AND game.status = 'CONFIRMED'
             WHERE game_match.status = 'COMPLETED'
               AND game_match.deleted_at IS NULL
             GROUP BY captain.user_id
         )
         SELECT team_stats.user_id,
                team_stats.name,
                team_stats.teams_captained,
                team_stats.titles,
                coalesce(match_stats.final_appearances, 0)::bigint AS final_appearances,
                coalesce(match_stats.matches, 0)::bigint AS matches,
                coalesce(match_stats.match_wins, 0)::bigint AS match_wins,
                coalesce(game_stats.games, 0)::bigint AS games,
                coalesce(game_stats.game_wins, 0)::bigint AS game_wins,
                team_stats.total_spent_points
         FROM team_stats
         LEFT JOIN match_stats ON match_stats.user_id = team_stats.user_id
         LEFT JOIN game_stats ON game_stats.user_id = team_stats.user_id
         ORDER BY team_stats.titles DESC,
                  coalesce(game_stats.game_wins, 0) DESC,
                  coalesce(game_stats.games, 0) DESC,
                  team_stats.user_id
         LIMIT 10",
    )
    .bind(guild_id)
    .bind(season_id)
    .fetch_all(&mut **transaction)
    .await?;
    rows.into_iter()
        .map(AuctionCaptainRow::into_snapshot)
        .collect()
}

async fn load_auction_top_bids(
    transaction: &mut sqlx::Transaction<'_, Postgres>,
    guild_id: &GuildId,
    season_id: SeasonId,
) -> Result<Vec<GuildStatisticsAuctionBid>, GuildStatisticsRepositoryError> {
    let rows = sqlx::query_as::<_, AuctionBidRow>(
        "WITH bid_members AS (
             SELECT tournament.id AS tournament_id,
                    tournament.title AS tournament_title,
                    tournament.champion_team_id,
                    team.id AS team_id,
                    team.team_name,
                    bidder.user_id,
                    member.display_name AS name,
                    action.points
             FROM auction_tournaments AS tournament
             JOIN auction_teams AS team
               ON team.guild_id = tournament.guild_id
              AND team.tournament_id = tournament.id
             JOIN auction_team_members AS bidder
               ON bidder.guild_id = team.guild_id
              AND bidder.tournament_id = team.tournament_id
              AND bidder.team_id = team.id
              AND bidder.acquired_via = 'BID'
              AND bidder.released_at IS NULL
             JOIN auction_actions AS action
               ON action.guild_id = bidder.guild_id
              AND action.tournament_id = bidder.tournament_id
              AND action.id = bidder.acquisition_action_id
              AND action.kind = 'AWARD_BID'
             JOIN guild_members AS member
               ON member.guild_id = bidder.guild_id
              AND member.user_id = bidder.user_id
             WHERE tournament.guild_id = $1
               AND tournament.season_id = $2
               AND tournament.status = 'COMPLETED'
               AND tournament.deleted_at IS NULL
         ),
         bid_game_stats AS (
             SELECT bid.tournament_id,
                    bid.team_id,
                    bid.user_id,
                    count(*)::bigint AS games,
                    count(*) FILTER (WHERE participant.team = CASE
                        WHEN (game.side_preset = 'DEFAULT' AND game.winning_side = 'BLUE')
                          OR (game.side_preset = 'SWAPPED' AND game.winning_side = 'RED')
                        THEN 'TEAM_1'
                        ELSE 'TEAM_2'
                    END)::bigint AS wins
             FROM bid_members AS bid
             JOIN auction_matches AS game_match
               ON game_match.guild_id = $1
              AND game_match.tournament_id = bid.tournament_id
              AND game_match.status = 'COMPLETED'
              AND game_match.deleted_at IS NULL
             JOIN games AS game
               ON game.guild_id = game_match.guild_id
              AND game.auction_match_id = game_match.id
              AND game.status = 'CONFIRMED'
             JOIN game_participants AS participant
               ON participant.guild_id = game.guild_id
              AND participant.game_id = game.id
              AND participant.user_id = bid.user_id
             GROUP BY bid.tournament_id, bid.team_id, bid.user_id
         )
         SELECT bid.tournament_id,
                bid.tournament_title,
                bid.team_name,
                bid.user_id,
                bid.name,
                bid.points,
                coalesce(stats.games, 0)::bigint AS games,
                coalesce(stats.wins, 0)::bigint AS wins,
                (bid.team_id = bid.champion_team_id) AS won_tournament
         FROM bid_members AS bid
         LEFT JOIN bid_game_stats AS stats
           ON stats.tournament_id = bid.tournament_id
          AND stats.team_id = bid.team_id
          AND stats.user_id = bid.user_id
         ORDER BY bid.points DESC, coalesce(stats.games, 0) DESC, bid.tournament_id, bid.user_id
         LIMIT 10",
    )
    .bind(guild_id)
    .bind(season_id)
    .fetch_all(&mut **transaction)
    .await?;
    rows.into_iter().map(AuctionBidRow::into_snapshot).collect()
}

fn compare_win_rates(left: &GuildStatisticsPlayer, right: &GuildStatisticsPlayer) -> Ordering {
    let left_rate = i128::from(left.wins) * i128::from(right.games);
    let right_rate = i128::from(right.wins) * i128::from(left.games);
    right_rate
        .cmp(&left_rate)
        .then_with(|| right.games.cmp(&left.games))
        .then_with(|| right.wins.cmp(&left.wins))
        .then_with(|| left.user_id.as_str().cmp(right.user_id.as_str()))
}

fn validate_record(games: i64, wins: i64) -> Result<i64, GuildStatisticsRepositoryError> {
    if games < 0 || wins < 0 || wins > games {
        return Err(GuildStatisticsRepositoryError::StoredInvariantViolation);
    }
    Ok(games - wins)
}

fn validate_overview(row: &OverviewRow) -> Result<(), GuildStatisticsRepositoryError> {
    let counts = [
        row.games,
        row.regular_games,
        row.auction_games,
        row.unique_players,
        row.blue_wins,
        row.red_wins,
        row.duration_games,
        row.participant_stat_games,
        row.champion_pick_games,
        row.champion_ban_games,
    ];
    if counts.iter().any(|count| *count < 0)
        || row.games != row.regular_games + row.auction_games
        || row.games != row.blue_wins + row.red_wins
        || [
            row.duration_games,
            row.participant_stat_games,
            row.champion_pick_games,
            row.champion_ban_games,
        ]
        .iter()
        .any(|count| *count > row.games)
        || row
            .average_duration_seconds
            .is_some_and(|seconds| !seconds.is_finite() || seconds <= 0.0)
    {
        return Err(GuildStatisticsRepositoryError::StoredInvariantViolation);
    }
    Ok(())
}

#[derive(Debug, FromRow)]
struct OverviewRow {
    games: i64,
    regular_games: i64,
    auction_games: i64,
    unique_players: i64,
    blue_wins: i64,
    red_wins: i64,
    average_duration_seconds: Option<f64>,
    duration_games: i64,
    participant_stat_games: i64,
    champion_pick_games: i64,
    champion_ban_games: i64,
}

#[derive(Debug, FromRow)]
struct PlayerRow {
    user_id: UserId,
    name: String,
    games: i64,
    wins: i64,
}

impl PlayerRow {
    fn into_snapshot(self) -> Result<GuildStatisticsPlayer, GuildStatisticsRepositoryError> {
        let losses = validate_record(self.games, self.wins)?;
        if self.name.trim().is_empty() {
            return Err(GuildStatisticsRepositoryError::StoredInvariantViolation);
        }
        Ok(GuildStatisticsPlayer {
            user_id: self.user_id,
            name: self.name,
            games: self.games,
            wins: self.wins,
            losses,
        })
    }
}

#[derive(Debug, FromRow)]
struct DuoRow {
    user_a_id: UserId,
    user_a_name: String,
    user_b_id: UserId,
    user_b_name: String,
    games: i64,
    wins: i64,
}

impl DuoRow {
    fn into_snapshot(self) -> Result<GuildStatisticsDuo, GuildStatisticsRepositoryError> {
        let losses = validate_record(self.games, self.wins)?;
        if self.user_a_id >= self.user_b_id
            || self.user_a_name.trim().is_empty()
            || self.user_b_name.trim().is_empty()
        {
            return Err(GuildStatisticsRepositoryError::StoredInvariantViolation);
        }
        Ok(GuildStatisticsDuo {
            user_a_id: self.user_a_id,
            user_a_name: self.user_a_name,
            user_b_id: self.user_b_id,
            user_b_name: self.user_b_name,
            games: self.games,
            wins: self.wins,
            losses,
        })
    }
}

#[derive(Debug, FromRow)]
struct ChampionPickRow {
    champion_id: String,
    lane: Lane,
    games: i64,
    wins: i64,
}

impl ChampionPickRow {
    fn into_snapshot(self) -> Result<GuildStatisticsChampionPick, GuildStatisticsRepositoryError> {
        let losses = validate_record(self.games, self.wins)?;
        if self.champion_id.trim().is_empty() {
            return Err(GuildStatisticsRepositoryError::StoredInvariantViolation);
        }
        Ok(GuildStatisticsChampionPick {
            champion_id: self.champion_id,
            lane: self.lane,
            games: self.games,
            wins: self.wins,
            losses,
        })
    }
}

#[derive(Debug, FromRow)]
struct ChampionBanRow {
    champion_id: String,
    bans: i64,
}

impl ChampionBanRow {
    fn into_snapshot(self) -> Result<GuildStatisticsChampionBan, GuildStatisticsRepositoryError> {
        if self.champion_id.trim().is_empty() || self.bans < 0 {
            return Err(GuildStatisticsRepositoryError::StoredInvariantViolation);
        }
        Ok(GuildStatisticsChampionBan {
            champion_id: self.champion_id,
            bans: self.bans,
        })
    }
}

#[derive(Debug, FromRow)]
struct AuctionCaptainRow {
    user_id: UserId,
    name: String,
    teams_captained: i64,
    titles: i64,
    final_appearances: i64,
    matches: i64,
    match_wins: i64,
    games: i64,
    game_wins: i64,
    total_spent_points: i64,
}

impl AuctionCaptainRow {
    fn into_snapshot(
        self,
    ) -> Result<GuildStatisticsAuctionCaptain, GuildStatisticsRepositoryError> {
        let match_losses = validate_record(self.matches, self.match_wins)?;
        let game_losses = validate_record(self.games, self.game_wins)?;
        if self.name.trim().is_empty()
            || self.teams_captained < 1
            || self.titles < 0
            || self.titles > self.teams_captained
            || self.final_appearances < 0
            || self.final_appearances > self.matches
            || self.total_spent_points < 0
        {
            return Err(GuildStatisticsRepositoryError::StoredInvariantViolation);
        }
        Ok(GuildStatisticsAuctionCaptain {
            user_id: self.user_id,
            name: self.name,
            teams_captained: self.teams_captained,
            titles: self.titles,
            final_appearances: self.final_appearances,
            match_wins: self.match_wins,
            match_losses,
            game_wins: self.game_wins,
            game_losses,
            total_spent_points: self.total_spent_points,
        })
    }
}

#[derive(Debug, FromRow)]
struct AuctionBidRow {
    tournament_id: uuid::Uuid,
    tournament_title: String,
    team_name: String,
    user_id: UserId,
    name: String,
    points: i32,
    games: i64,
    wins: i64,
    won_tournament: bool,
}

impl AuctionBidRow {
    fn into_snapshot(self) -> Result<GuildStatisticsAuctionBid, GuildStatisticsRepositoryError> {
        let losses = validate_record(self.games, self.wins)?;
        if self.tournament_title.trim().is_empty()
            || self.team_name.trim().is_empty()
            || self.name.trim().is_empty()
            || self.points < 0
        {
            return Err(GuildStatisticsRepositoryError::StoredInvariantViolation);
        }
        Ok(GuildStatisticsAuctionBid {
            tournament_id: self.tournament_id.to_string(),
            tournament_title: self.tournament_title,
            team_name: self.team_name,
            user_id: self.user_id,
            name: self.name,
            points: self.points,
            games: self.games,
            wins: self.wins,
            losses,
            won_tournament: self.won_tournament,
        })
    }
}

#[derive(Debug, Error)]
pub enum GuildStatisticsRepositoryError {
    #[error("guild was not found for this viewer")]
    GuildNotFound,
    #[error("season was not found for this guild")]
    SeasonNotFound,
    #[error("stored guild statistics data violated an invariant")]
    StoredInvariantViolation,
    #[error("database operation failed: {0}")]
    Database(#[from] sqlx::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(id: &str, games: i64, wins: i64) -> GuildStatisticsPlayer {
        GuildStatisticsPlayer {
            user_id: UserId::parse(id).expect("valid user ID"),
            name: id.to_owned(),
            games,
            wins,
            losses: games - wins,
        }
    }

    #[test]
    fn win_rate_order_uses_exact_ratio_then_sample_size() {
        let higher_rate = player("100000000000000001", 3, 2);
        let lower_rate = player("100000000000000002", 5, 3);
        let same_rate_more_games = player("100000000000000003", 6, 4);

        assert_eq!(compare_win_rates(&higher_rate, &lower_rate), Ordering::Less);
        assert_eq!(
            compare_win_rates(&higher_rate, &same_rate_more_games),
            Ordering::Greater
        );
    }

    #[test]
    fn scope_accepts_only_documented_values() {
        assert_eq!(
            GuildStatisticsScope::parse("ALL"),
            Some(GuildStatisticsScope::All)
        );
        assert_eq!(
            GuildStatisticsScope::parse("REGULAR"),
            Some(GuildStatisticsScope::Regular)
        );
        assert_eq!(
            GuildStatisticsScope::parse("AUCTION"),
            Some(GuildStatisticsScope::Auction)
        );
        assert_eq!(GuildStatisticsScope::parse("all"), None);
    }
}
