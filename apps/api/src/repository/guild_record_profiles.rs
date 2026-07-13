use std::collections::{HashMap, HashSet};

use sqlx::{Executor, FromRow, PgPool, Postgres};
use thiserror::Error;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::domain::{DiscordUserId, GuildId, Lane, SeasonId, UserId};
use crate::repository::champion_preferences::ChampionPreferencesSnapshot;
use crate::repository::riot::normalize_riot_id_v1;

const RECORD_KIND_SERIES: &str = "SERIES";
const RECORD_KIND_TOURNAMENT: &str = "TOURNAMENT";
const RECORD_SCOPE_REGULAR: &str = "REGULAR";
const RECORD_SCOPE_AUCTION: &str = "AUCTION";
const RECORD_SCOPE_TOURNAMENT: &str = "TOURNAMENT";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GuildRecordCursor {
    pub played_at: OffsetDateTime,
    pub kind: GuildRecordKind,
    pub id: Uuid,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GuildRecordKind {
    Series,
    Tournament,
}

impl GuildRecordKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Series => RECORD_KIND_SERIES,
            Self::Tournament => RECORD_KIND_TOURNAMENT,
        }
    }

    fn parse(value: &str) -> Result<Self, GuildRecordProfileRepositoryError> {
        match value {
            RECORD_KIND_SERIES => Ok(Self::Series),
            RECORD_KIND_TOURNAMENT => Ok(Self::Tournament),
            _ => Err(GuildRecordProfileRepositoryError::StoredInvariantViolation),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GuildRecordResult {
    Win,
    Loss,
    Draw,
}

impl GuildRecordResult {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Win => "WIN",
            Self::Loss => "LOSS",
            Self::Draw => "DRAW",
        }
    }

    fn parse(value: &str) -> Result<Self, GuildRecordProfileRepositoryError> {
        match value {
            "WIN" => Ok(Self::Win),
            "LOSS" => Ok(Self::Loss),
            "DRAW" => Ok(Self::Draw),
            _ => Err(GuildRecordProfileRepositoryError::StoredInvariantViolation),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GuildRecordsSearchSnapshot {
    pub member: GuildRecordProfileMember,
    pub champion_preferences: ChampionPreferencesSnapshot,
    pub riot_accounts: Vec<VisibleRiotAccount>,
    pub seasons: Vec<GuildRecordSeason>,
    pub selected_season_id: Option<SeasonId>,
    pub summary: GuildRecordSummary,
    pub lane_mmr: Vec<GuildRecordLaneMmr>,
    pub champion_records: Vec<GuildRecordChampion>,
    pub records: Vec<GuildRecord>,
    pub next_cursor: Option<GuildRecordCursor>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum GuildRecordsSearchLookup {
    Found(Box<GuildRecordsSearchSnapshot>),
    Ambiguous,
    NotFound,
    NotMember,
    SeasonNotFound,
}

#[derive(Clone, Debug, FromRow, PartialEq)]
pub struct GuildRecordSeason {
    pub id: SeasonId,
    pub name: String,
    pub status: String,
    pub started_at: OffsetDateTime,
    pub ended_at: Option<OffsetDateTime>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildRecordSummary {
    pub games: i64,
    pub wins: i64,
    pub losses: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GuildRecordLaneMmr {
    pub lane: Lane,
    pub mmr: f64,
    pub games: i32,
    pub wins: i32,
}

#[derive(Clone, Debug, Eq, FromRow, PartialEq)]
pub struct GuildRecordChampion {
    pub champion_id: String,
    pub lane: Lane,
    pub games: i64,
    pub wins: i64,
    pub losses: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum GuildRecord {
    Series(GuildRecordSeries),
    Tournament(GuildRecordTournament),
}

#[derive(Clone, Debug, PartialEq)]
pub struct GuildRecordSeries {
    pub id: Uuid,
    pub source: String,
    pub title: String,
    pub played_at: OffsetDateTime,
    pub result: GuildRecordResult,
    pub team_name: Option<String>,
    pub opponent_name: Option<String>,
    pub stage: Option<String>,
    pub wins: i64,
    pub losses: i64,
    pub mmr_delta: Option<f64>,
    pub games: Vec<GuildRecordGame>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GuildRecordTournament {
    pub id: Uuid,
    pub title: String,
    pub played_at: OffsetDateTime,
    pub team_name: Option<String>,
    pub spent_points: Option<i32>,
    pub placement: Option<i16>,
    pub wins: i64,
    pub losses: i64,
    pub mmr_delta: Option<f64>,
    pub roster: Vec<GuildRecordLineup>,
    pub series: Vec<GuildRecordTournamentSeries>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GuildRecordTournamentSeries {
    pub id: Uuid,
    pub title: String,
    pub stage: String,
    pub opponent_name: Option<String>,
    pub played_at: OffsetDateTime,
    pub result: GuildRecordResult,
    pub wins: i64,
    pub losses: i64,
    pub mmr_delta: Option<f64>,
    pub games: Vec<GuildRecordGame>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GuildRecordGame {
    pub id: Uuid,
    pub parent_id: Uuid,
    pub game_number: i16,
    pub played_at: OffsetDateTime,
    pub winning_side: String,
    pub target_side: String,
    pub target_lane: Lane,
    pub target_champion_id: Option<String>,
    pub mmr_before: Option<f64>,
    pub mmr_after: Option<f64>,
    pub blue_team: Vec<GuildRecordLineup>,
    pub red_team: Vec<GuildRecordLineup>,
    pub blue_bans: Vec<String>,
    pub red_bans: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildRecordLineup {
    pub display_name: String,
    pub lane: Lane,
    pub champion_id: Option<String>,
    pub is_target: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildRecordProfileMember {
    pub user_id: UserId,
    pub display_name: String,
    pub avatar_hash: Option<String>,
    pub main_lane: Option<Lane>,
    pub matched_by_riot_id: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GuildRecordProfileLookup {
    Found(GuildRecordProfileMember),
    Ambiguous,
    NotFound,
    NotMember,
}

#[derive(Clone, Debug, Eq, FromRow, PartialEq)]
pub struct VisibleRiotAccount {
    pub platform: String,
    pub game_name: String,
    pub tag_line: String,
    pub profile_icon_id: Option<i32>,
    pub is_main: bool,
}

pub async fn find_member(
    pool: &PgPool,
    viewer_id: &UserId,
    guild_id: &GuildId,
    query: &str,
) -> Result<GuildRecordProfileLookup, GuildRecordProfileRepositoryError> {
    find_member_with(pool, viewer_id, guild_id, query).await
}

async fn find_member_with<'e, E>(
    executor: E,
    viewer_id: &UserId,
    guild_id: &GuildId,
    query: &str,
) -> Result<GuildRecordProfileLookup, GuildRecordProfileRepositoryError>
where
    E: Executor<'e, Database = Postgres>,
{
    let query_is_user_id = DiscordUserId::parse(query).is_ok()
        || (query.starts_with("dmy_") && UserId::parse(query).is_ok());
    let normalized_riot_id = normalize_riot_id_v1(query);
    let rows = sqlx::query_as::<_, GuildRecordProfileMemberRow>(
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
         ),
         candidates AS (
             SELECT member.user_id,
                    member.display_name,
                    app_user.avatar_hash,
                    member.main_lane,
                    NOT $4
                    AND lower(member.display_name) <> lower($3)
                    AND EXISTS (
                        SELECT 1
                        FROM riot_accounts AS matched_account
                        JOIN user_privacy_settings AS matched_privacy
                          ON matched_privacy.user_id = matched_account.user_id
                         AND matched_privacy.show_riot_ids IS TRUE
                        WHERE matched_account.user_id = member.user_id
                          AND matched_account.normalized_game_name
                              || '#'
                              || matched_account.normalized_tag_line = $5
                    ) AS matched_by_riot_id
             FROM guild_members AS member
             JOIN users AS app_user
               ON app_user.user_id = member.user_id
              AND app_user.deleted_at IS NULL
             WHERE (SELECT has_access FROM viewer)
               AND member.guild_id = $2
               AND member.status = 'ACTIVE'
               AND (
                   ($4 AND member.user_id = $3)
                   OR (
                       NOT $4
                       AND (
                           lower(member.display_name) = lower($3)
                           OR EXISTS (
                               SELECT 1
                               FROM riot_accounts AS account
                               JOIN user_privacy_settings AS privacy
                                 ON privacy.user_id = account.user_id
                                AND privacy.show_riot_ids IS TRUE
                               WHERE account.user_id = member.user_id
                                 AND account.normalized_game_name
                                     || '#'
                                     || account.normalized_tag_line = $5
                           )
                       )
                   )
               )
             ORDER BY lower(member.display_name), member.display_name, member.user_id
             LIMIT 2
         )
         SELECT viewer.has_access,
                candidate.user_id,
                candidate.display_name,
                candidate.avatar_hash,
                candidate.main_lane,
                candidate.matched_by_riot_id
         FROM viewer
         LEFT JOIN candidates AS candidate ON true",
    )
    .bind(viewer_id)
    .bind(guild_id)
    .bind(query)
    .bind(query_is_user_id)
    .bind(normalized_riot_id)
    .fetch_all(executor)
    .await?;

    classify_member_rows(rows)
}

pub async fn list_visible_riot_accounts(
    pool: &PgPool,
    user_id: &UserId,
) -> Result<Vec<VisibleRiotAccount>, GuildRecordProfileRepositoryError> {
    list_visible_riot_accounts_with(pool, user_id).await
}

async fn list_visible_riot_accounts_with<'e, E>(
    executor: E,
    user_id: &UserId,
) -> Result<Vec<VisibleRiotAccount>, GuildRecordProfileRepositoryError>
where
    E: Executor<'e, Database = Postgres>,
{
    let accounts = sqlx::query_as::<_, VisibleRiotAccount>(
        "SELECT account.platform,
                account.game_name,
                account.tag_line,
                account.profile_icon_id,
                account.is_main
         FROM riot_accounts AS account
         JOIN user_privacy_settings AS privacy
           ON privacy.user_id = account.user_id
          AND privacy.show_riot_ids IS TRUE
         WHERE account.user_id = $1
         ORDER BY account.is_main DESC, account.created_at, account.puuid",
    )
    .bind(user_id)
    .fetch_all(executor)
    .await?;
    Ok(accounts)
}

pub async fn search(
    pool: &PgPool,
    viewer_id: &UserId,
    guild_id: &GuildId,
    query: &str,
    requested_season_id: Option<SeasonId>,
    cursor: Option<GuildRecordCursor>,
    limit: i64,
) -> Result<GuildRecordsSearchLookup, GuildRecordProfileRepositoryError> {
    let mut transaction = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *transaction)
        .await?;

    let member = match find_member_with(&mut *transaction, viewer_id, guild_id, query).await? {
        GuildRecordProfileLookup::Found(member) => member,
        GuildRecordProfileLookup::Ambiguous => {
            transaction.commit().await?;
            return Ok(GuildRecordsSearchLookup::Ambiguous);
        }
        GuildRecordProfileLookup::NotFound => {
            transaction.commit().await?;
            return Ok(GuildRecordsSearchLookup::NotFound);
        }
        GuildRecordProfileLookup::NotMember => {
            transaction.commit().await?;
            return Ok(GuildRecordsSearchLookup::NotMember);
        }
    };

    let champion_preferences = load_champion_preferences(&mut transaction, &member.user_id).await?;
    let riot_accounts = list_visible_riot_accounts_with(&mut *transaction, &member.user_id).await?;
    if member.matched_by_riot_id && riot_accounts.is_empty() {
        transaction.commit().await?;
        return Ok(GuildRecordsSearchLookup::NotFound);
    }

    let seasons = load_seasons(&mut transaction, guild_id).await?;
    let selected_season_id = if let Some(requested) = requested_season_id {
        if seasons.iter().any(|season| season.id == requested) {
            Some(requested)
        } else {
            transaction.commit().await?;
            return Ok(GuildRecordsSearchLookup::SeasonNotFound);
        }
    } else {
        seasons
            .iter()
            .find(|season| season.status == "ACTIVE")
            .map(|season| season.id)
    };

    let (summary, lane_mmr, champion_records, records, next_cursor) = if let Some(season_id) =
        selected_season_id
    {
        let summary = load_summary(&mut transaction, guild_id, season_id, &member.user_id).await?;
        let lane_mmr =
            load_lane_mmr(&mut transaction, guild_id, season_id, &member.user_id).await?;
        let champion_records =
            load_champion_records(&mut transaction, guild_id, season_id, &member.user_id).await?;
        let (records, next_cursor) = load_record_page(
            &mut transaction,
            guild_id,
            season_id,
            &member.user_id,
            cursor,
            limit,
        )
        .await?;
        (summary, lane_mmr, champion_records, records, next_cursor)
    } else {
        (
            GuildRecordSummary {
                games: 0,
                wins: 0,
                losses: 0,
            },
            default_lane_mmr(),
            Vec::new(),
            Vec::new(),
            None,
        )
    };

    transaction.commit().await?;
    Ok(GuildRecordsSearchLookup::Found(Box::new(
        GuildRecordsSearchSnapshot {
            member,
            champion_preferences,
            riot_accounts,
            seasons,
            selected_season_id,
            summary,
            lane_mmr,
            champion_records,
            records,
            next_cursor,
        },
    )))
}

async fn load_champion_preferences(
    transaction: &mut sqlx::Transaction<'_, Postgres>,
    user_id: &UserId,
) -> Result<ChampionPreferencesSnapshot, GuildRecordProfileRepositoryError> {
    let rows = sqlx::query_as::<_, ChampionPreferenceRow>(
        "SELECT lane, position, champion_id
         FROM user_champion_preferences
         WHERE user_id = $1
         ORDER BY lane, position",
    )
    .bind(user_id)
    .fetch_all(&mut **transaction)
    .await?;
    let mut snapshot = ChampionPreferencesSnapshot::default();
    let mut next_position = HashMap::<Lane, i16>::new();
    for row in rows {
        let expected = next_position.entry(row.lane).or_insert(1);
        if row.position != *expected {
            return Err(GuildRecordProfileRepositoryError::StoredInvariantViolation);
        }
        *expected += 1;
        match row.lane {
            Lane::Top => snapshot.top.push(row.champion_id),
            Lane::Jungle => snapshot.jungle.push(row.champion_id),
            Lane::Mid => snapshot.mid.push(row.champion_id),
            Lane::Bottom => snapshot.bottom.push(row.champion_id),
            Lane::Support => snapshot.support.push(row.champion_id),
        }
    }
    Ok(snapshot)
}

async fn load_seasons(
    transaction: &mut sqlx::Transaction<'_, Postgres>,
    guild_id: &GuildId,
) -> Result<Vec<GuildRecordSeason>, GuildRecordProfileRepositoryError> {
    let seasons = sqlx::query_as::<_, GuildRecordSeason>(
        "SELECT id, name, status, started_at, ended_at
         FROM seasons
         WHERE guild_id = $1
         ORDER BY (status = 'ACTIVE') DESC, started_at DESC, id DESC",
    )
    .bind(guild_id)
    .fetch_all(&mut **transaction)
    .await?;
    if seasons.iter().any(|season| {
        !matches!(season.status.as_str(), "ACTIVE" | "CLOSED")
            || (season.status == "ACTIVE") != season.ended_at.is_none()
    }) {
        return Err(GuildRecordProfileRepositoryError::StoredInvariantViolation);
    }
    Ok(seasons)
}

async fn load_summary(
    transaction: &mut sqlx::Transaction<'_, Postgres>,
    guild_id: &GuildId,
    season_id: SeasonId,
    user_id: &UserId,
) -> Result<GuildRecordSummary, GuildRecordProfileRepositoryError> {
    let row = sqlx::query_as::<_, GuildRecordSummaryRow>(
        "SELECT count(*) AS games,
                count(*) FILTER (WHERE
                    (participant.team = 'TEAM_1' AND (
                        (game.side_preset = 'DEFAULT' AND game.winning_side = 'BLUE') OR
                        (game.side_preset = 'SWAPPED' AND game.winning_side = 'RED')
                    )) OR
                    (participant.team = 'TEAM_2' AND (
                        (game.side_preset = 'DEFAULT' AND game.winning_side = 'RED') OR
                        (game.side_preset = 'SWAPPED' AND game.winning_side = 'BLUE')
                    ))
                ) AS wins
         FROM games AS game
         JOIN game_participants AS participant
           ON participant.guild_id = game.guild_id
          AND participant.game_id = game.id
          AND participant.user_id = $3
         WHERE game.guild_id = $1
           AND game.season_id = $2
           AND game.status = 'CONFIRMED'
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
           )",
    )
    .bind(guild_id)
    .bind(season_id)
    .bind(user_id)
    .fetch_one(&mut **transaction)
    .await?;
    Ok(GuildRecordSummary {
        games: row.games,
        wins: row.wins,
        losses: row.games - row.wins,
    })
}

async fn load_lane_mmr(
    transaction: &mut sqlx::Transaction<'_, Postgres>,
    guild_id: &GuildId,
    season_id: SeasonId,
    user_id: &UserId,
) -> Result<Vec<GuildRecordLaneMmr>, GuildRecordProfileRepositoryError> {
    let rows = sqlx::query_as::<_, GuildRecordLaneMmrRow>(
        "SELECT lane, mmr, games_played, wins
         FROM user_lane_mmr
         WHERE guild_id = $1 AND season_id = $2 AND user_id = $3",
    )
    .bind(guild_id)
    .bind(season_id)
    .bind(user_id)
    .fetch_all(&mut **transaction)
    .await?;
    let mut by_lane = rows
        .into_iter()
        .map(|row| {
            if !row.mmr.is_finite()
                || row.games_played < 0
                || !(0..=row.games_played).contains(&row.wins)
            {
                return Err(GuildRecordProfileRepositoryError::StoredInvariantViolation);
            }
            Ok((
                row.lane,
                GuildRecordLaneMmr {
                    lane: row.lane,
                    mmr: row.mmr,
                    games: row.games_played,
                    wins: row.wins,
                },
            ))
        })
        .collect::<Result<HashMap<_, _>, _>>()?;
    Ok(canonical_lanes()
        .into_iter()
        .map(|lane| {
            by_lane.remove(&lane).unwrap_or(GuildRecordLaneMmr {
                lane,
                mmr: 1500.0,
                games: 0,
                wins: 0,
            })
        })
        .collect())
}

async fn load_champion_records(
    transaction: &mut sqlx::Transaction<'_, Postgres>,
    guild_id: &GuildId,
    season_id: SeasonId,
    user_id: &UserId,
) -> Result<Vec<GuildRecordChampion>, GuildRecordProfileRepositoryError> {
    Ok(sqlx::query_as::<_, GuildRecordChampion>(
        "SELECT participant.champion_id,
                participant.lane,
                count(*) AS games,
                count(*) FILTER (WHERE
                    (participant.team = 'TEAM_1' AND (
                        (game.side_preset = 'DEFAULT' AND game.winning_side = 'BLUE') OR
                        (game.side_preset = 'SWAPPED' AND game.winning_side = 'RED')
                    )) OR
                    (participant.team = 'TEAM_2' AND (
                        (game.side_preset = 'DEFAULT' AND game.winning_side = 'RED') OR
                        (game.side_preset = 'SWAPPED' AND game.winning_side = 'BLUE')
                    ))
                ) AS wins,
                count(*) FILTER (WHERE NOT (
                    (participant.team = 'TEAM_1' AND (
                        (game.side_preset = 'DEFAULT' AND game.winning_side = 'BLUE') OR
                        (game.side_preset = 'SWAPPED' AND game.winning_side = 'RED')
                    )) OR
                    (participant.team = 'TEAM_2' AND (
                        (game.side_preset = 'DEFAULT' AND game.winning_side = 'RED') OR
                        (game.side_preset = 'SWAPPED' AND game.winning_side = 'BLUE')
                    ))
                )) AS losses
         FROM games AS game
         JOIN game_participants AS participant
           ON participant.guild_id = game.guild_id
          AND participant.game_id = game.id
          AND participant.user_id = $3
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
         GROUP BY participant.champion_id, participant.lane
         ORDER BY count(*) DESC,
                  count(*) FILTER (WHERE
                    (participant.team = 'TEAM_1' AND (
                        (game.side_preset = 'DEFAULT' AND game.winning_side = 'BLUE') OR
                        (game.side_preset = 'SWAPPED' AND game.winning_side = 'RED')
                    )) OR
                    (participant.team = 'TEAM_2' AND (
                        (game.side_preset = 'DEFAULT' AND game.winning_side = 'RED') OR
                        (game.side_preset = 'SWAPPED' AND game.winning_side = 'BLUE')
                    ))
                  ) DESC,
                  participant.champion_id,
                  CASE participant.lane
                      WHEN 'TOP' THEN 1 WHEN 'JUNGLE' THEN 2 WHEN 'MID' THEN 3
                      WHEN 'BOTTOM' THEN 4 WHEN 'SUPPORT' THEN 5
                  END",
    )
    .bind(guild_id)
    .bind(season_id)
    .bind(user_id)
    .fetch_all(&mut **transaction)
    .await?)
}

fn canonical_lanes() -> [Lane; 5] {
    [
        Lane::Top,
        Lane::Jungle,
        Lane::Mid,
        Lane::Bottom,
        Lane::Support,
    ]
}

fn default_lane_mmr() -> Vec<GuildRecordLaneMmr> {
    canonical_lanes()
        .into_iter()
        .map(|lane| GuildRecordLaneMmr {
            lane,
            mmr: 1500.0,
            games: 0,
            wins: 0,
        })
        .collect()
}

async fn load_record_page(
    transaction: &mut sqlx::Transaction<'_, Postgres>,
    guild_id: &GuildId,
    season_id: SeasonId,
    user_id: &UserId,
    cursor: Option<GuildRecordCursor>,
    limit: i64,
) -> Result<(Vec<GuildRecord>, Option<GuildRecordCursor>), GuildRecordProfileRepositoryError> {
    let cursor_played_at = cursor.map(|value| value.played_at);
    let cursor_id = cursor.map(|value| value.id);
    let cursor_kind = cursor.map(|value| value.kind.as_str());
    let mut heads = sqlx::query_as::<_, GuildRecordHeadRow>(
        "WITH candidates AS (
             SELECT 'SERIES'::text AS kind,
                    'REGULAR'::text AS scope,
                    scrim.id,
                    scrim.title,
                    max(game.played_at) AS played_at,
                    CASE
                        WHEN scrim.winning_team = locked.team THEN 'WIN'
                        WHEN scrim.winning_team IS NULL THEN 'DRAW'
                        ELSE 'LOSS'
                    END::text AS result,
                    NULL::uuid AS team_id,
                    NULL::text AS team_name,
                    NULL::text AS opponent_name,
                    NULL::text AS stage,
                    NULL::integer AS spent_points,
                    NULL::smallint AS placement
             FROM series AS scrim
             JOIN series_participants AS locked
               ON locked.guild_id = scrim.guild_id
              AND locked.series_id = scrim.id
              AND locked.user_id = $3
             JOIN games AS game
               ON game.guild_id = scrim.guild_id
              AND game.season_id = scrim.season_id
              AND game.series_id = scrim.id
              AND game.status = 'CONFIRMED'
             JOIN game_participants AS target
               ON target.guild_id = game.guild_id
              AND target.game_id = game.id
              AND target.user_id = $3
              AND target.team = locked.team
             WHERE scrim.guild_id = $1
               AND scrim.season_id = $2
               AND scrim.status = 'COMPLETED'
               AND scrim.deleted_at IS NULL
             GROUP BY scrim.id, scrim.title, scrim.winning_team, locked.team

             UNION ALL

             SELECT 'SERIES'::text AS kind,
                    'AUCTION'::text AS scope,
                    match.id,
                    tournament.title,
                    max(game.played_at) AS played_at,
                    CASE WHEN match.winner_team_id = target_team.id
                         THEN 'WIN' ELSE 'LOSS' END::text AS result,
                    target_team.id AS team_id,
                    target_team.team_name AS team_name,
                    opponent_team.team_name AS opponent_name,
                    match.match_key AS stage,
                    NULL::integer AS spent_points,
                    NULL::smallint AS placement
             FROM auction_tournaments AS tournament
             JOIN auction_matches AS match
               ON match.guild_id = tournament.guild_id
              AND match.tournament_id = tournament.id
              AND match.season_id = tournament.season_id
              AND match.status = 'COMPLETED'
              AND match.deleted_at IS NULL
             JOIN games AS game
               ON game.guild_id = match.guild_id
              AND game.season_id = match.season_id
              AND game.auction_match_id = match.id
              AND game.status = 'CONFIRMED'
             JOIN game_participants AS target
               ON target.guild_id = game.guild_id
              AND target.game_id = game.id
              AND target.user_id = $3
             JOIN auction_teams AS target_team
               ON target_team.guild_id = match.guild_id
              AND target_team.tournament_id = match.tournament_id
              AND target_team.id = CASE target.team
                    WHEN 'TEAM_1' THEN match.left_team_id ELSE match.right_team_id END
             LEFT JOIN auction_teams AS opponent_team
               ON opponent_team.guild_id = match.guild_id
              AND opponent_team.tournament_id = match.tournament_id
              AND opponent_team.id = CASE target.team
                    WHEN 'TEAM_1' THEN match.right_team_id ELSE match.left_team_id END
             WHERE tournament.guild_id = $1
               AND tournament.season_id = $2
               AND tournament.format = 'TEN_PERSON'
               AND tournament.status = 'COMPLETED'
               AND tournament.deleted_at IS NULL
               AND match.match_key = 'FINAL'
             GROUP BY match.id, tournament.title, match.winner_team_id,
                      target_team.id, target_team.team_name,
                      opponent_team.team_name, match.match_key

             UNION ALL

             SELECT 'TOURNAMENT'::text AS kind,
                    'TOURNAMENT'::text AS scope,
                    tournament.id,
                    tournament.title,
                    max(game.played_at) AS played_at,
                    NULL::text AS result,
                    target_team.id AS team_id,
                    target_team.team_name AS team_name,
                    NULL::text AS opponent_name,
                    NULL::text AS stage,
                    target_team.initial_points - target_team.current_points AS spent_points,
                    CASE
                        WHEN tournament.champion_team_id = target_team.id THEN 1::smallint
                        WHEN bool_or(match.match_key = 'FINAL'
                                     AND match.winner_team_id <> target_team.id) THEN 2::smallint
                        WHEN bool_or(match.match_key IN ('SEMIFINAL_A', 'SEMIFINAL_B')
                                     AND match.winner_team_id <> target_team.id) THEN 3::smallint
                        ELSE NULL::smallint
                    END AS placement
             FROM auction_tournaments AS tournament
             JOIN auction_matches AS match
               ON match.guild_id = tournament.guild_id
              AND match.tournament_id = tournament.id
              AND match.season_id = tournament.season_id
              AND match.status = 'COMPLETED'
              AND match.deleted_at IS NULL
             JOIN games AS game
               ON game.guild_id = match.guild_id
              AND game.season_id = match.season_id
              AND game.auction_match_id = match.id
              AND game.status = 'CONFIRMED'
             JOIN game_participants AS target
               ON target.guild_id = game.guild_id
              AND target.game_id = game.id
              AND target.user_id = $3
             JOIN auction_teams AS target_team
               ON target_team.guild_id = match.guild_id
              AND target_team.tournament_id = match.tournament_id
              AND target_team.id = CASE target.team
                    WHEN 'TEAM_1' THEN match.left_team_id ELSE match.right_team_id END
             WHERE tournament.guild_id = $1
               AND tournament.season_id = $2
               AND tournament.format = 'TWENTY_PERSON'
               AND tournament.status = 'COMPLETED'
               AND tournament.deleted_at IS NULL
             GROUP BY tournament.id, tournament.title, tournament.champion_team_id,
                      target_team.id, target_team.team_name,
                      target_team.initial_points, target_team.current_points
         )
         SELECT kind, scope, id, title, played_at, result, team_id, team_name,
                opponent_name, stage, spent_points, placement
         FROM candidates
         WHERE $4::timestamptz IS NULL
            OR (played_at, id, kind) < ($4, $5::uuid, $6::text)
         ORDER BY played_at DESC, id DESC, kind DESC
         LIMIT $7",
    )
    .bind(guild_id)
    .bind(season_id)
    .bind(user_id)
    .bind(cursor_played_at)
    .bind(cursor_id)
    .bind(cursor_kind)
    .bind(limit + 1)
    .fetch_all(&mut **transaction)
    .await?;

    let has_more = i64::try_from(heads.len()).is_ok_and(|count| count > limit);
    if has_more {
        heads.pop();
    }
    let next_cursor = if has_more {
        heads.last().map(GuildRecordHeadRow::cursor).transpose()?
    } else {
        None
    };
    let records = hydrate_record_heads(transaction, guild_id, user_id, heads).await?;
    Ok((records, next_cursor))
}

async fn hydrate_record_heads(
    transaction: &mut sqlx::Transaction<'_, Postgres>,
    guild_id: &GuildId,
    user_id: &UserId,
    heads: Vec<GuildRecordHeadRow>,
) -> Result<Vec<GuildRecord>, GuildRecordProfileRepositoryError> {
    if heads.is_empty() {
        return Ok(Vec::new());
    }
    let scopes = heads
        .iter()
        .map(|head| head.scope.clone())
        .collect::<Vec<_>>();
    let ids = heads.iter().map(|head| head.id).collect::<Vec<_>>();
    let games_by_top = load_games(transaction, guild_id, user_id, &scopes, &ids).await?;
    let tournament_heads = heads
        .iter()
        .filter(|head| head.scope == RECORD_SCOPE_TOURNAMENT)
        .collect::<Vec<_>>();
    let tournament_ids = tournament_heads
        .iter()
        .map(|head| head.id)
        .collect::<Vec<_>>();
    let tournament_team_ids = tournament_heads
        .iter()
        .map(|head| {
            head.team_id
                .ok_or(GuildRecordProfileRepositoryError::StoredInvariantViolation)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let series_meta =
        load_tournament_series_meta(transaction, guild_id, user_id, &tournament_ids).await?;
    let rosters = load_tournament_rosters(
        transaction,
        guild_id,
        user_id,
        &tournament_ids,
        &tournament_team_ids,
    )
    .await?;
    assemble_records(heads, games_by_top, series_meta, rosters)
}

async fn load_games(
    transaction: &mut sqlx::Transaction<'_, Postgres>,
    guild_id: &GuildId,
    user_id: &UserId,
    scopes: &[String],
    ids: &[Uuid],
) -> Result<HashMap<Uuid, Vec<GuildRecordGame>>, GuildRecordProfileRepositoryError> {
    let bases = sqlx::query_as::<_, GuildRecordGameRow>(
        "WITH requested AS (
             SELECT request.scope, request.id
             FROM unnest($3::text[], $4::uuid[]) AS request(scope, id)
         )
         SELECT requested.id AS top_id,
                game.id,
                COALESCE(game.series_id, game.auction_match_id) AS parent_id,
                game.game_number,
                game.played_at,
                game.winning_side,
                game.side_preset,
                target.team AS target_team,
                target.lane AS target_lane,
                target.champion_id AS target_champion_id,
                ledger.mmr_before,
                ledger.mmr_after
         FROM requested
         JOIN games AS game
           ON game.guild_id = $1
          AND game.status = 'CONFIRMED'
         LEFT JOIN auction_matches AS auction_match
           ON auction_match.guild_id = game.guild_id
          AND auction_match.id = game.auction_match_id
         JOIN game_participants AS target
           ON target.guild_id = game.guild_id
          AND target.game_id = game.id
          AND target.user_id = $2
         LEFT JOIN LATERAL (
             SELECT entry.mmr_before, entry.mmr_after
             FROM mmr_ledger_entries AS entry
             WHERE entry.guild_id = game.guild_id
               AND entry.game_id = game.id
               AND entry.user_id = $2
               AND entry.lane = target.lane
               AND entry.source_type = 'GAME_RESULT'
         ) AS ledger ON true
         WHERE (requested.scope = 'REGULAR' AND game.series_id = requested.id)
            OR (requested.scope = 'AUCTION' AND game.auction_match_id = requested.id)
            OR (requested.scope = 'TOURNAMENT'
                AND auction_match.tournament_id = requested.id)
         ORDER BY requested.id, game.played_at, game.game_number, game.id",
    )
    .bind(guild_id)
    .bind(user_id)
    .bind(scopes)
    .bind(ids)
    .fetch_all(&mut **transaction)
    .await?;
    let game_ids = bases.iter().map(|game| game.id).collect::<Vec<_>>();
    if game_ids.iter().collect::<HashSet<_>>().len() != game_ids.len() {
        return Err(GuildRecordProfileRepositoryError::StoredInvariantViolation);
    }

    let lineup_rows = sqlx::query_as::<_, GuildRecordLineupRow>(
        "SELECT participant.game_id,
                game.side_preset,
                participant.team,
                member.display_name,
                participant.lane,
                participant.champion_id,
                participant.user_id = $3 AS is_target
         FROM game_participants AS participant
         JOIN games AS game
           ON game.guild_id = participant.guild_id
          AND game.id = participant.game_id
         JOIN guild_members AS member
           ON member.guild_id = participant.guild_id
          AND member.user_id = participant.user_id
         WHERE participant.guild_id = $1
           AND participant.game_id = ANY($2::uuid[])
         ORDER BY participant.game_id,
                  participant.team,
                  CASE participant.lane
                      WHEN 'TOP' THEN 1 WHEN 'JUNGLE' THEN 2 WHEN 'MID' THEN 3
                      WHEN 'BOTTOM' THEN 4 WHEN 'SUPPORT' THEN 5
                  END",
    )
    .bind(guild_id)
    .bind(&game_ids)
    .bind(user_id)
    .fetch_all(&mut **transaction)
    .await?;
    let ban_rows = sqlx::query_as::<_, GuildRecordBanRow>(
        "SELECT game_id, side, slot, champion_id
         FROM game_bans
         WHERE guild_id = $1 AND game_id = ANY($2::uuid[])
         ORDER BY game_id, side, slot",
    )
    .bind(guild_id)
    .bind(&game_ids)
    .fetch_all(&mut **transaction)
    .await?;

    let mut lineups = HashMap::<Uuid, (Vec<GuildRecordLineup>, Vec<GuildRecordLineup>)>::new();
    for row in lineup_rows {
        let side = participant_side(&row.team, &row.side_preset)?;
        let lineup = GuildRecordLineup {
            display_name: row.display_name,
            lane: row.lane,
            champion_id: row.champion_id,
            is_target: row.is_target,
        };
        let teams = lineups.entry(row.game_id).or_default();
        match side {
            "BLUE" => teams.0.push(lineup),
            "RED" => teams.1.push(lineup),
            _ => return Err(GuildRecordProfileRepositoryError::StoredInvariantViolation),
        }
    }
    let mut bans = HashMap::<Uuid, (Vec<String>, Vec<String>)>::new();
    for row in ban_rows {
        if !(1..=5).contains(&row.slot) {
            return Err(GuildRecordProfileRepositoryError::StoredInvariantViolation);
        }
        let sides = bans.entry(row.game_id).or_default();
        match row.side.as_str() {
            "BLUE" => sides.0.push(row.champion_id),
            "RED" => sides.1.push(row.champion_id),
            _ => return Err(GuildRecordProfileRepositoryError::StoredInvariantViolation),
        }
    }

    let mut by_top = HashMap::<Uuid, Vec<GuildRecordGame>>::new();
    for row in bases {
        if !matches!(row.winning_side.as_str(), "BLUE" | "RED")
            || !matches!(row.side_preset.as_str(), "DEFAULT" | "SWAPPED")
            || row.mmr_before.is_some() != row.mmr_after.is_some()
            || row.mmr_before.is_some_and(|value| !value.is_finite())
            || row.mmr_after.is_some_and(|value| !value.is_finite())
        {
            return Err(GuildRecordProfileRepositoryError::StoredInvariantViolation);
        }
        let target_side = participant_side(&row.target_team, &row.side_preset)?.to_owned();
        let (blue_team, red_team) = lineups.remove(&row.id).unwrap_or_default();
        if blue_team.len() != 5
            || red_team.len() != 5
            || blue_team
                .iter()
                .chain(&red_team)
                .filter(|member| member.is_target)
                .count()
                != 1
        {
            return Err(GuildRecordProfileRepositoryError::StoredInvariantViolation);
        }
        let (blue_bans, red_bans) = bans.remove(&row.id).unwrap_or_default();
        by_top.entry(row.top_id).or_default().push(GuildRecordGame {
            id: row.id,
            parent_id: row
                .parent_id
                .ok_or(GuildRecordProfileRepositoryError::StoredInvariantViolation)?,
            game_number: row.game_number,
            played_at: row.played_at,
            winning_side: row.winning_side,
            target_side,
            target_lane: row.target_lane,
            target_champion_id: row.target_champion_id,
            mmr_before: row.mmr_before,
            mmr_after: row.mmr_after,
            blue_team,
            red_team,
            blue_bans,
            red_bans,
        });
    }
    if !lineups.is_empty() || !bans.is_empty() {
        return Err(GuildRecordProfileRepositoryError::StoredInvariantViolation);
    }
    Ok(by_top)
}

async fn load_tournament_series_meta(
    transaction: &mut sqlx::Transaction<'_, Postgres>,
    guild_id: &GuildId,
    user_id: &UserId,
    tournament_ids: &[Uuid],
) -> Result<Vec<GuildRecordTournamentSeriesRow>, GuildRecordProfileRepositoryError> {
    if tournament_ids.is_empty() {
        return Ok(Vec::new());
    }
    Ok(sqlx::query_as::<_, GuildRecordTournamentSeriesRow>(
        "SELECT tournament.id AS tournament_id,
                match.id,
                tournament.title,
                match.match_key AS stage,
                opponent_team.team_name AS opponent_name,
                max(game.played_at) AS played_at,
                CASE WHEN match.winner_team_id = target_team.id
                     THEN 'WIN' ELSE 'LOSS' END::text AS result
         FROM auction_tournaments AS tournament
         JOIN auction_matches AS match
           ON match.guild_id = tournament.guild_id
          AND match.tournament_id = tournament.id
          AND match.status = 'COMPLETED'
          AND match.deleted_at IS NULL
         JOIN games AS game
           ON game.guild_id = match.guild_id
          AND game.auction_match_id = match.id
          AND game.status = 'CONFIRMED'
         JOIN game_participants AS target
           ON target.guild_id = game.guild_id
          AND target.game_id = game.id
          AND target.user_id = $2
         JOIN auction_teams AS target_team
           ON target_team.guild_id = match.guild_id
          AND target_team.tournament_id = match.tournament_id
          AND target_team.id = CASE target.team
                WHEN 'TEAM_1' THEN match.left_team_id ELSE match.right_team_id END
         LEFT JOIN auction_teams AS opponent_team
           ON opponent_team.guild_id = match.guild_id
          AND opponent_team.tournament_id = match.tournament_id
          AND opponent_team.id = CASE target.team
                WHEN 'TEAM_1' THEN match.right_team_id ELSE match.left_team_id END
         WHERE tournament.guild_id = $1
           AND tournament.id = ANY($3::uuid[])
         GROUP BY tournament.id, match.id, tournament.title, match.match_key,
                  opponent_team.team_name, match.winner_team_id, target_team.id
         ORDER BY tournament.id, max(game.played_at) DESC, match.id DESC",
    )
    .bind(guild_id)
    .bind(user_id)
    .bind(tournament_ids)
    .fetch_all(&mut **transaction)
    .await?)
}

async fn load_tournament_rosters(
    transaction: &mut sqlx::Transaction<'_, Postgres>,
    guild_id: &GuildId,
    user_id: &UserId,
    tournament_ids: &[Uuid],
    team_ids: &[Uuid],
) -> Result<HashMap<Uuid, Vec<GuildRecordLineup>>, GuildRecordProfileRepositoryError> {
    if tournament_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = sqlx::query_as::<_, GuildRecordRosterRow>(
        "WITH requested AS (
             SELECT request.tournament_id, request.team_id
             FROM unnest($2::uuid[], $3::uuid[]) AS request(tournament_id, team_id)
         )
         SELECT requested.tournament_id,
                member.display_name,
                COALESCE(roster.declared_lane, recent_participation.lane) AS lane,
                roster.user_id = $4 AS is_target
         FROM requested
         JOIN auction_team_members AS roster
           ON roster.guild_id = $1
          AND roster.tournament_id = requested.tournament_id
          AND roster.team_id = requested.team_id
          AND roster.released_at IS NULL
         JOIN guild_members AS member
           ON member.guild_id = roster.guild_id
          AND member.user_id = roster.user_id
         LEFT JOIN LATERAL (
             SELECT participant.lane
             FROM auction_matches AS match
             JOIN games AS game
               ON game.guild_id = match.guild_id
              AND game.auction_match_id = match.id
              AND game.status = 'CONFIRMED'
             JOIN game_participants AS participant
               ON participant.guild_id = game.guild_id
              AND participant.game_id = game.id
              AND participant.user_id = roster.user_id
             WHERE match.guild_id = roster.guild_id
               AND match.tournament_id = roster.tournament_id
               AND CASE participant.team
                     WHEN 'TEAM_1' THEN match.left_team_id ELSE match.right_team_id END
                   = roster.team_id
             ORDER BY game.played_at DESC, game.id DESC
             LIMIT 1
         ) AS recent_participation ON true
         ORDER BY requested.tournament_id, roster.roster_slot",
    )
    .bind(guild_id)
    .bind(tournament_ids)
    .bind(team_ids)
    .bind(user_id)
    .fetch_all(&mut **transaction)
    .await?;
    let mut rosters = HashMap::<Uuid, Vec<GuildRecordLineup>>::new();
    for row in rows {
        rosters
            .entry(row.tournament_id)
            .or_default()
            .push(GuildRecordLineup {
                display_name: row.display_name,
                lane: row
                    .lane
                    .ok_or(GuildRecordProfileRepositoryError::StoredInvariantViolation)?,
                champion_id: None,
                is_target: row.is_target,
            });
    }
    Ok(rosters)
}

fn assemble_records(
    heads: Vec<GuildRecordHeadRow>,
    mut games_by_top: HashMap<Uuid, Vec<GuildRecordGame>>,
    series_meta: Vec<GuildRecordTournamentSeriesRow>,
    mut rosters: HashMap<Uuid, Vec<GuildRecordLineup>>,
) -> Result<Vec<GuildRecord>, GuildRecordProfileRepositoryError> {
    let mut meta_by_tournament = HashMap::<Uuid, Vec<GuildRecordTournamentSeriesRow>>::new();
    for meta in series_meta {
        if !matches!(meta.stage.as_str(), "SEMIFINAL_A" | "SEMIFINAL_B" | "FINAL") {
            return Err(GuildRecordProfileRepositoryError::StoredInvariantViolation);
        }
        meta_by_tournament
            .entry(meta.tournament_id)
            .or_default()
            .push(meta);
    }

    let mut records = Vec::with_capacity(heads.len());
    for head in heads {
        let games = games_by_top
            .remove(&head.id)
            .filter(|games| !games.is_empty())
            .ok_or(GuildRecordProfileRepositoryError::StoredInvariantViolation)?;
        match (GuildRecordKind::parse(&head.kind)?, head.scope.as_str()) {
            (GuildRecordKind::Series, RECORD_SCOPE_REGULAR | RECORD_SCOPE_AUCTION) => {
                if head.scope == RECORD_SCOPE_AUCTION
                    && !matches!(
                        head.stage.as_deref(),
                        Some("SEMIFINAL_A" | "SEMIFINAL_B" | "FINAL")
                    )
                {
                    return Err(GuildRecordProfileRepositoryError::StoredInvariantViolation);
                }
                let (wins, losses, mmr_delta) = game_aggregate(&games)?;
                records.push(GuildRecord::Series(GuildRecordSeries {
                    id: head.id,
                    source: head.scope,
                    title: head.title,
                    played_at: head.played_at,
                    result: GuildRecordResult::parse(
                        head.result
                            .as_deref()
                            .ok_or(GuildRecordProfileRepositoryError::StoredInvariantViolation)?,
                    )?,
                    team_name: head.team_name,
                    opponent_name: head.opponent_name,
                    stage: head.stage,
                    wins,
                    losses,
                    mmr_delta,
                    games,
                }));
            }
            (GuildRecordKind::Tournament, RECORD_SCOPE_TOURNAMENT) => {
                let metas = meta_by_tournament
                    .remove(&head.id)
                    .filter(|rows| !rows.is_empty())
                    .ok_or(GuildRecordProfileRepositoryError::StoredInvariantViolation)?;
                let mut games_by_match = games.iter().cloned().fold(
                    HashMap::<Uuid, Vec<GuildRecordGame>>::new(),
                    |mut grouped, game| {
                        grouped.entry(game.parent_id).or_default().push(game);
                        grouped
                    },
                );
                let mut series = Vec::with_capacity(metas.len());
                for meta in metas {
                    let match_games = games_by_match
                        .remove(&meta.id)
                        .filter(|rows| !rows.is_empty())
                        .ok_or(GuildRecordProfileRepositoryError::StoredInvariantViolation)?;
                    let (wins, losses, mmr_delta) = game_aggregate(&match_games)?;
                    series.push(GuildRecordTournamentSeries {
                        id: meta.id,
                        title: meta.title,
                        stage: meta.stage,
                        opponent_name: meta.opponent_name,
                        played_at: meta.played_at,
                        result: GuildRecordResult::parse(&meta.result)?,
                        wins,
                        losses,
                        mmr_delta,
                        games: match_games,
                    });
                }
                if !games_by_match.is_empty() {
                    return Err(GuildRecordProfileRepositoryError::StoredInvariantViolation);
                }
                let roster = rosters.remove(&head.id).unwrap_or_default();
                if roster.len() != 5 || roster.iter().filter(|member| member.is_target).count() != 1
                {
                    return Err(GuildRecordProfileRepositoryError::StoredInvariantViolation);
                }
                if head.spent_points.is_some_and(|points| points < 0)
                    || head
                        .placement
                        .is_some_and(|placement| !(1..=3).contains(&placement))
                {
                    return Err(GuildRecordProfileRepositoryError::StoredInvariantViolation);
                }
                let (wins, losses, mmr_delta) = game_aggregate(&games)?;
                records.push(GuildRecord::Tournament(GuildRecordTournament {
                    id: head.id,
                    title: head.title,
                    played_at: head.played_at,
                    team_name: head.team_name,
                    spent_points: head.spent_points,
                    placement: head.placement,
                    wins,
                    losses,
                    mmr_delta,
                    roster,
                    series,
                }));
            }
            _ => return Err(GuildRecordProfileRepositoryError::StoredInvariantViolation),
        }
    }
    if !games_by_top.is_empty() || !meta_by_tournament.is_empty() || !rosters.is_empty() {
        return Err(GuildRecordProfileRepositoryError::StoredInvariantViolation);
    }
    Ok(records)
}

fn game_aggregate(
    games: &[GuildRecordGame],
) -> Result<(i64, i64, Option<f64>), GuildRecordProfileRepositoryError> {
    let wins = games
        .iter()
        .filter(|game| game.winning_side == game.target_side)
        .count() as i64;
    let mmr_values = games
        .iter()
        .filter_map(|game| game.mmr_before.zip(game.mmr_after))
        .collect::<Vec<_>>();
    let mmr_delta = (mmr_values.len() == games.len()).then(|| {
        mmr_values
            .into_iter()
            .map(|(before, after)| after - before)
            .sum::<f64>()
    });
    if mmr_delta.is_some_and(|value| !value.is_finite()) {
        return Err(GuildRecordProfileRepositoryError::StoredInvariantViolation);
    }
    Ok((wins, games.len() as i64 - wins, mmr_delta))
}

fn participant_side(
    team: &str,
    side_preset: &str,
) -> Result<&'static str, GuildRecordProfileRepositoryError> {
    match (team, side_preset) {
        ("TEAM_1", "DEFAULT") | ("TEAM_2", "SWAPPED") => Ok("BLUE"),
        ("TEAM_2", "DEFAULT") | ("TEAM_1", "SWAPPED") => Ok("RED"),
        _ => Err(GuildRecordProfileRepositoryError::StoredInvariantViolation),
    }
}

#[derive(FromRow)]
struct ChampionPreferenceRow {
    lane: Lane,
    position: i16,
    champion_id: String,
}

#[derive(FromRow)]
struct GuildRecordSummaryRow {
    games: i64,
    wins: i64,
}

#[derive(FromRow)]
struct GuildRecordLaneMmrRow {
    lane: Lane,
    mmr: f64,
    games_played: i32,
    wins: i32,
}

#[derive(Clone, FromRow)]
struct GuildRecordHeadRow {
    kind: String,
    scope: String,
    id: Uuid,
    title: String,
    played_at: OffsetDateTime,
    result: Option<String>,
    team_id: Option<Uuid>,
    team_name: Option<String>,
    opponent_name: Option<String>,
    stage: Option<String>,
    spent_points: Option<i32>,
    placement: Option<i16>,
}

impl GuildRecordHeadRow {
    fn cursor(&self) -> Result<GuildRecordCursor, GuildRecordProfileRepositoryError> {
        Ok(GuildRecordCursor {
            played_at: self.played_at,
            kind: GuildRecordKind::parse(&self.kind)?,
            id: self.id,
        })
    }
}

#[derive(FromRow)]
struct GuildRecordGameRow {
    top_id: Uuid,
    id: Uuid,
    parent_id: Option<Uuid>,
    game_number: i16,
    played_at: OffsetDateTime,
    winning_side: String,
    side_preset: String,
    target_team: String,
    target_lane: Lane,
    target_champion_id: Option<String>,
    mmr_before: Option<f64>,
    mmr_after: Option<f64>,
}

#[derive(FromRow)]
struct GuildRecordLineupRow {
    game_id: Uuid,
    side_preset: String,
    team: String,
    display_name: String,
    lane: Lane,
    champion_id: Option<String>,
    is_target: bool,
}

#[derive(FromRow)]
struct GuildRecordBanRow {
    game_id: Uuid,
    side: String,
    slot: i16,
    champion_id: String,
}

#[derive(FromRow)]
struct GuildRecordTournamentSeriesRow {
    tournament_id: Uuid,
    id: Uuid,
    title: String,
    stage: String,
    opponent_name: Option<String>,
    played_at: OffsetDateTime,
    result: String,
}

#[derive(FromRow)]
struct GuildRecordRosterRow {
    tournament_id: Uuid,
    display_name: String,
    lane: Option<Lane>,
    is_target: bool,
}

fn classify_member_rows(
    rows: Vec<GuildRecordProfileMemberRow>,
) -> Result<GuildRecordProfileLookup, GuildRecordProfileRepositoryError> {
    let Some(first) = rows.first() else {
        return Err(GuildRecordProfileRepositoryError::StoredInvariantViolation);
    };
    if !first.has_access {
        return Ok(GuildRecordProfileLookup::NotMember);
    }

    let candidates = rows
        .into_iter()
        .filter_map(GuildRecordProfileMemberRow::into_member)
        .collect::<Result<Vec<_>, _>>()?;
    match candidates.len() {
        0 => Ok(GuildRecordProfileLookup::NotFound),
        1 => Ok(GuildRecordProfileLookup::Found(
            candidates
                .into_iter()
                .next()
                .expect("one candidate was counted"),
        )),
        _ => Ok(GuildRecordProfileLookup::Ambiguous),
    }
}

#[derive(Clone, Debug, FromRow, PartialEq)]
struct GuildRecordProfileMemberRow {
    has_access: bool,
    user_id: Option<String>,
    display_name: Option<String>,
    avatar_hash: Option<String>,
    main_lane: Option<Lane>,
    matched_by_riot_id: Option<bool>,
}

impl GuildRecordProfileMemberRow {
    fn into_member(
        self,
    ) -> Option<Result<GuildRecordProfileMember, GuildRecordProfileRepositoryError>> {
        let user_id = self.user_id?;
        let Some(display_name) = self.display_name else {
            return Some(Err(
                GuildRecordProfileRepositoryError::StoredInvariantViolation,
            ));
        };
        let Some(matched_by_riot_id) = self.matched_by_riot_id else {
            return Some(Err(
                GuildRecordProfileRepositoryError::StoredInvariantViolation,
            ));
        };
        Some(
            UserId::parse(user_id)
                .map_err(|_| GuildRecordProfileRepositoryError::StoredInvariantViolation)
                .map(|user_id| GuildRecordProfileMember {
                    user_id,
                    display_name,
                    avatar_hash: self.avatar_hash,
                    main_lane: self.main_lane,
                    matched_by_riot_id,
                }),
        )
    }
}

#[derive(Debug, Error)]
pub enum GuildRecordProfileRepositoryError {
    #[error("PostgreSQL guild record profile repository operation failed")]
    Sql(#[from] sqlx::Error),
    #[error("stored guild record profile identity is invalid")]
    StoredInvariantViolation,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(winning_side: &str, target_side: &str, mmr: Option<(f64, f64)>) -> GuildRecordGame {
        GuildRecordGame {
            id: Uuid::now_v7(),
            parent_id: Uuid::now_v7(),
            game_number: 1,
            played_at: OffsetDateTime::UNIX_EPOCH,
            winning_side: winning_side.to_owned(),
            target_side: target_side.to_owned(),
            target_lane: Lane::Mid,
            target_champion_id: Some("Ahri".to_owned()),
            mmr_before: mmr.map(|value| value.0),
            mmr_after: mmr.map(|value| value.1),
            blue_team: Vec::new(),
            red_team: Vec::new(),
            blue_bans: Vec::new(),
            red_bans: Vec::new(),
        }
    }

    fn row(user_id: Option<&str>, display_name: Option<&str>) -> GuildRecordProfileMemberRow {
        GuildRecordProfileMemberRow {
            has_access: true,
            user_id: user_id.map(str::to_owned),
            display_name: display_name.map(str::to_owned),
            avatar_hash: None,
            main_lane: None,
            matched_by_riot_id: user_id.map(|_| false),
        }
    }

    #[test]
    fn classifier_distinguishes_hidden_missing_unique_and_ambiguous_targets() {
        let mut denied = row(None, None);
        denied.has_access = false;
        assert_eq!(
            classify_member_rows(vec![denied]).expect("access classification"),
            GuildRecordProfileLookup::NotMember
        );
        assert_eq!(
            classify_member_rows(vec![row(None, None)]).expect("missing classification"),
            GuildRecordProfileLookup::NotFound
        );
        assert!(matches!(
            classify_member_rows(vec![row(Some("100000000000000001"), Some("Longcat"))])
                .expect("unique classification"),
            GuildRecordProfileLookup::Found(_)
        ));
        assert_eq!(
            classify_member_rows(vec![
                row(Some("100000000000000001"), Some("Twin")),
                row(Some("100000000000000002"), Some("Twin")),
            ])
            .expect("ambiguous classification"),
            GuildRecordProfileLookup::Ambiguous
        );
    }

    #[test]
    fn classifier_rejects_partial_or_invalid_stored_identity() {
        assert!(matches!(
            classify_member_rows(vec![row(Some("100000000000000001"), None)]),
            Err(GuildRecordProfileRepositoryError::StoredInvariantViolation)
        ));
        assert!(matches!(
            classify_member_rows(vec![row(Some("invalid"), Some("Longcat"))]),
            Err(GuildRecordProfileRepositoryError::StoredInvariantViolation)
        ));
    }

    #[test]
    fn side_projection_honors_swapped_team_assignment() {
        assert_eq!(participant_side("TEAM_1", "DEFAULT").unwrap(), "BLUE");
        assert_eq!(participant_side("TEAM_2", "DEFAULT").unwrap(), "RED");
        assert_eq!(participant_side("TEAM_1", "SWAPPED").unwrap(), "RED");
        assert_eq!(participant_side("TEAM_2", "SWAPPED").unwrap(), "BLUE");
        assert!(participant_side("TEAM_3", "DEFAULT").is_err());
    }

    #[test]
    fn aggregate_does_not_publish_partial_legacy_mmr() {
        let complete = [
            game("BLUE", "BLUE", Some((1500.0, 1515.0))),
            game("RED", "BLUE", Some((1515.0, 1507.0))),
        ];
        assert_eq!(game_aggregate(&complete).unwrap(), (1, 1, Some(7.0)));

        let mixed = [complete[0].clone(), game("RED", "BLUE", None)];
        assert_eq!(game_aggregate(&mixed).unwrap(), (1, 1, None));
    }
}
