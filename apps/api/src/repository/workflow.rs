use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use thiserror::Error;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::domain::{
    AuctionMatchId, AuctionRecruitmentId, AuctionTournamentId, AuctionTournamentStatus, GuildId,
    RecruitmentId, RecruitmentStatus, SeasonId, SeriesId, SeriesStatus, UserId,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationKind {
    Entry,
    PickBan,
    Auction,
}

impl OperationKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Entry => "ENTRY",
            Self::PickBan => "PICK_BAN",
            Self::Auction => "AUCTION",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationPriority {
    Urgent,
    Normal,
}

impl OperationPriority {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Urgent => "urgent",
            Self::Normal => "normal",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationRecord {
    pub id: String,
    pub kind: OperationKind,
    pub title: String,
    pub status: &'static str,
    pub detail: String,
    pub updated_at: OffsetDateTime,
    pub href: String,
    pub priority: OperationPriority,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AuctionFormat {
    TenPerson,
    TwentyPerson,
}

impl AuctionFormat {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TenPerson => "TEN_PERSON",
            Self::TwentyPerson => "TWENTY_PERSON",
        }
    }

    pub const fn participant_count(self) -> i16 {
        match self {
            Self::TenPerson => 10,
            Self::TwentyPerson => 20,
        }
    }

    pub const fn team_count(self) -> i16 {
        match self {
            Self::TenPerson => 2,
            Self::TwentyPerson => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ListState {
    Active,
    Completed,
}

impl ListState {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Completed => "completed",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SeriesOutcome {
    Team1,
    Team2,
    Draw,
    Cancel,
}

impl SeriesOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Team1 => "TEAM_1",
            Self::Team2 => "TEAM_2",
            Self::Draw => "DRAW",
            Self::Cancel => "CANCEL",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageCursor {
    pub updated_at: OffsetDateTime,
    pub id: Uuid,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<PageCursor>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecruitmentRecord {
    pub id: RecruitmentId,
    pub display_number: i64,
    pub title: String,
    pub status: RecruitmentStatus,
    pub target_count: i16,
    pub best_of: i16,
    pub hard_fearless: bool,
    pub participant_count: i64,
    pub assigned_count: i64,
    pub revision: i64,
    pub updated_at: OffsetDateTime,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SeriesRecord {
    pub id: SeriesId,
    pub display_number: i64,
    pub title: String,
    pub best_of: i16,
    pub hard_fearless: bool,
    pub status: SeriesStatus,
    pub team_1_score: i64,
    pub team_2_score: i64,
    pub outcome: Option<SeriesOutcome>,
    pub winner: Option<&'static str>,
    pub terminal_reason: Option<String>,
    pub current_set_number: Option<i16>,
    pub updated_at: OffsetDateTime,
    pub played_at: Option<OffsetDateTime>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuctionRecruitmentRecord {
    pub id: AuctionRecruitmentId,
    pub display_number: i64,
    pub title: String,
    pub format: AuctionFormat,
    pub semifinal_best_of: Option<i16>,
    pub final_best_of: i16,
    pub hard_fearless: bool,
    pub target_count: i16,
    pub participant_count: i64,
    pub team_count: i16,
    pub status: RecruitmentStatus,
    pub revision: i64,
    pub updated_at: OffsetDateTime,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuctionMatchSummaryRecord {
    pub id: AuctionMatchId,
    pub match_key: String,
    pub status: String,
    pub confirmed_game_count: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuctionTournamentRecord {
    pub id: AuctionTournamentId,
    pub display_number: i64,
    pub title: String,
    pub format: AuctionFormat,
    pub status: AuctionTournamentStatus,
    pub revision: i64,
    pub participant_count: i16,
    pub team_count: i16,
    pub active_member_count: i64,
    pub bid_member_count: i64,
    pub incomplete_match_count: i64,
    pub champion_team_id: Option<Uuid>,
    pub champion_team_name: Option<String>,
    pub next_match: Option<AuctionMatchSummaryRecord>,
    pub updated_at: OffsetDateTime,
    pub ended_at: Option<OffsetDateTime>,
}

pub async fn has_guild_access(
    pool: &PgPool,
    user_id: &UserId,
    guild_id: &GuildId,
) -> Result<bool, WorkflowRepositoryError> {
    let accessible = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1
             FROM guild_members AS membership
             JOIN guilds AS guild
               ON guild.guild_id = membership.guild_id
              AND guild.status = 'ACTIVE'
             WHERE membership.guild_id = $2
               AND membership.user_id = $1
               AND membership.status = 'ACTIVE'
         )",
    )
    .bind(user_id)
    .bind(guild_id)
    .fetch_one(pool)
    .await?;

    Ok(accessible)
}

pub async fn list_operations(
    pool: &PgPool,
    user_id: &UserId,
    guild_id: &GuildId,
) -> Result<Vec<OperationRecord>, WorkflowRepositoryError> {
    let rows = sqlx::query_as::<_, OperationRow>(
        "WITH viewer AS (
             SELECT membership.guild_id
             FROM guild_members AS membership
             JOIN guilds AS guild
               ON guild.guild_id = membership.guild_id
              AND guild.status = 'ACTIVE'
             WHERE membership.guild_id = $2
               AND membership.user_id = $1
               AND membership.status = 'ACTIVE'
         ),
         operation_rows AS (
             SELECT recruitment.id AS entity_id,
                    'ENTRY'::text AS source_kind,
                    recruitment.title,
                    recruitment.updated_at,
                    joined.count AS first_count,
                    assigned.count AS second_count,
                    recruitment.target_count::bigint AS third_count,
                    NULL::text AS source_status,
                    NULL::text AS auxiliary_status,
                    NULL::uuid AS auxiliary_id,
                    0::smallint AS priority_rank
             FROM recruitments AS recruitment
             JOIN viewer ON viewer.guild_id = recruitment.guild_id
             CROSS JOIN LATERAL (
                 SELECT count(*) AS count
                 FROM recruitment_participants AS participant
                 WHERE participant.guild_id = recruitment.guild_id
                   AND participant.recruitment_id = recruitment.id
                   AND participant.status = 'JOINED'
             ) AS joined
             CROSS JOIN LATERAL (
                 SELECT count(*) AS count
                 FROM recruitment_entry_assignments AS assignment
                 JOIN recruitment_participants AS participant
                   ON participant.guild_id = assignment.guild_id
                  AND participant.recruitment_id = assignment.recruitment_id
                  AND participant.user_id = assignment.user_id
                  AND participant.status = 'JOINED'
                 WHERE assignment.guild_id = recruitment.guild_id
                   AND assignment.recruitment_id = recruitment.id
             ) AS assigned
             WHERE recruitment.guild_id = $2
               AND recruitment.status = 'CLOSED'
               AND recruitment.converted_at IS NULL
               AND NOT EXISTS (
                   SELECT 1
                   FROM series AS existing_series
                   WHERE existing_series.guild_id = recruitment.guild_id
                     AND existing_series.source_recruitment_id = recruitment.id
                     AND existing_series.status <> 'CANCELLED'
                     AND existing_series.deleted_at IS NULL
               )

             UNION ALL

             SELECT scrim.id,
                    'PICK_BAN'::text,
                    scrim.title,
                    scrim.updated_at,
                    score.confirmed_games,
                    scrim.best_of::bigint,
                    0::bigint,
                    scrim.status,
                    NULL::text,
                    NULL::uuid,
                    0::smallint
             FROM series AS scrim
             JOIN viewer ON viewer.guild_id = scrim.guild_id
             CROSS JOIN LATERAL (
                 SELECT count(*) AS confirmed_games
                 FROM games AS game
                 WHERE game.guild_id = scrim.guild_id
                   AND game.series_id = scrim.id
                   AND game.status = 'CONFIRMED'
             ) AS score
             WHERE scrim.guild_id = $2
               AND scrim.status = 'IN_PROGRESS'
               AND scrim.deleted_at IS NULL

             UNION ALL

             SELECT recruitment.id,
                    'AUCTION_RECRUITMENT'::text,
                    recruitment.title,
                    recruitment.updated_at,
                    joined.count,
                    CASE recruitment.format WHEN 'TEN_PERSON' THEN 2 ELSE 4 END::bigint,
                    recruitment.target_count::bigint,
                    recruitment.status,
                    NULL::text,
                    NULL::uuid,
                    1::smallint
             FROM auction_recruitments AS recruitment
             JOIN viewer ON viewer.guild_id = recruitment.guild_id
             CROSS JOIN LATERAL (
                 SELECT count(*) AS count
                 FROM auction_recruitment_participants AS participant
                 WHERE participant.guild_id = recruitment.guild_id
                   AND participant.recruitment_id = recruitment.id
                   AND participant.status = 'JOINED'
             ) AS joined
             WHERE recruitment.guild_id = $2
               AND recruitment.status = 'CLOSED'
               AND recruitment.converted_at IS NULL
               AND NOT EXISTS (
                   SELECT 1
                   FROM auction_tournaments AS existing_tournament
                   WHERE existing_tournament.guild_id = recruitment.guild_id
                     AND existing_tournament.source_recruitment_id = recruitment.id
                     AND existing_tournament.status <> 'CANCELLED'
                     AND existing_tournament.deleted_at IS NULL
               )

             UNION ALL

             SELECT tournament.id,
                    'AUCTION_TOURNAMENT'::text,
                    tournament.title,
                    tournament.updated_at,
                    members.count,
                    teams.count,
                    matches.count,
                    tournament.status,
                    next_match.status,
                    next_match.id,
                    1::smallint
             FROM auction_tournaments AS tournament
             JOIN viewer ON viewer.guild_id = tournament.guild_id
             CROSS JOIN LATERAL (
                 SELECT count(*) AS count
                 FROM auction_team_members AS member
                 WHERE member.guild_id = tournament.guild_id
                   AND member.tournament_id = tournament.id
                   AND member.released_at IS NULL
             ) AS members
             CROSS JOIN LATERAL (
                 SELECT count(*) AS count
                 FROM auction_teams AS team
                 WHERE team.guild_id = tournament.guild_id
                   AND team.tournament_id = tournament.id
             ) AS teams
             CROSS JOIN LATERAL (
                 SELECT count(*) AS count
                 FROM auction_matches AS auction_match
                 WHERE auction_match.guild_id = tournament.guild_id
                   AND auction_match.tournament_id = tournament.id
                   AND auction_match.deleted_at IS NULL
                   AND auction_match.status NOT IN ('COMPLETED', 'CANCELLED')
             ) AS matches
             LEFT JOIN LATERAL (
                 SELECT auction_match.id,
                        auction_match.status
                 FROM auction_matches AS auction_match
                 WHERE auction_match.guild_id = tournament.guild_id
                   AND auction_match.tournament_id = tournament.id
                   AND auction_match.deleted_at IS NULL
                   AND auction_match.status NOT IN ('COMPLETED', 'CANCELLED')
                 ORDER BY CASE auction_match.status
                              WHEN 'IN_PROGRESS' THEN 0
                              WHEN 'READY' THEN 1
                              ELSE 2
                          END,
                          CASE auction_match.match_key
                              WHEN 'SEMIFINAL_A' THEN 0
                              WHEN 'SEMIFINAL_B' THEN 1
                              ELSE 2
                          END,
                          auction_match.id
                 LIMIT 1
             ) AS next_match ON TRUE
             WHERE tournament.guild_id = $2
               AND tournament.status IN ('BIDDING', 'BRACKET_SETUP', 'IN_GAME')
               AND tournament.deleted_at IS NULL
         )
         SELECT entity_id,
                source_kind,
                title,
                updated_at,
                first_count,
                second_count,
                third_count,
                source_status,
                auxiliary_status,
                auxiliary_id
         FROM operation_rows
         ORDER BY priority_rank, updated_at DESC, source_kind, entity_id DESC
         LIMIT 100",
    )
    .bind(user_id)
    .bind(guild_id)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| row.into_record(guild_id))
        .collect()
}

pub async fn list_actionable_recruitments(
    pool: &PgPool,
    user_id: &UserId,
    guild_id: &GuildId,
    cursor: Option<PageCursor>,
    limit: i64,
) -> Result<Page<RecruitmentRecord>, WorkflowRepositoryError> {
    let (cursor_time, cursor_id) = split_cursor(cursor);
    let rows = sqlx::query_as::<_, RecruitmentRow>(
        "WITH page AS (
             SELECT recruitment.guild_id,
                    recruitment.id,
                    recruitment.display_number,
                    recruitment.title,
                    recruitment.status,
                    recruitment.target_count,
                    recruitment.best_of,
                    recruitment.hard_fearless,
                    recruitment.revision,
                    recruitment.updated_at
             FROM recruitments AS recruitment
             JOIN guilds AS guild
               ON guild.guild_id = recruitment.guild_id
              AND guild.status = 'ACTIVE'
             JOIN guild_members AS viewer
               ON viewer.guild_id = recruitment.guild_id
              AND viewer.user_id = $1
              AND viewer.status = 'ACTIVE'
             WHERE recruitment.guild_id = $2
               AND recruitment.status IN ('OPEN', 'CLOSED')
               AND recruitment.converted_at IS NULL
               AND NOT EXISTS (
                   SELECT 1
                   FROM series AS existing_series
                   WHERE existing_series.guild_id = recruitment.guild_id
                     AND existing_series.source_recruitment_id = recruitment.id
                     AND existing_series.status <> 'CANCELLED'
                     AND existing_series.deleted_at IS NULL
               )
               AND (
                   $3::timestamptz IS NULL
                   OR (recruitment.updated_at, recruitment.id) < ($3, $4::uuid)
               )
             ORDER BY recruitment.updated_at DESC, recruitment.id DESC
             LIMIT $5
         )
         SELECT page.id,
                page.display_number,
                page.title,
                page.status,
                page.target_count,
                page.best_of,
                page.hard_fearless,
                page.revision,
                page.updated_at,
                joined.count AS participant_count,
                assigned.count AS assigned_count
         FROM page
         CROSS JOIN LATERAL (
             SELECT count(*) AS count
             FROM recruitment_participants AS participant
             WHERE participant.guild_id = page.guild_id
               AND participant.recruitment_id = page.id
               AND participant.status = 'JOINED'
         ) AS joined
         CROSS JOIN LATERAL (
             SELECT count(*) AS count
             FROM recruitment_entry_assignments AS assignment
             JOIN recruitment_participants AS participant
               ON participant.guild_id = assignment.guild_id
              AND participant.recruitment_id = assignment.recruitment_id
              AND participant.user_id = assignment.user_id
              AND participant.status = 'JOINED'
             WHERE assignment.guild_id = page.guild_id
               AND assignment.recruitment_id = page.id
         ) AS assigned
         ORDER BY page.updated_at DESC, page.id DESC",
    )
    .bind(user_id)
    .bind(guild_id)
    .bind(cursor_time)
    .bind(cursor_id)
    .bind(limit + 1)
    .fetch_all(pool)
    .await?;

    Ok(page_from_rows(
        rows,
        limit,
        RecruitmentRow::cursor,
        RecruitmentRow::into_record,
    ))
}

pub async fn list_series(
    pool: &PgPool,
    user_id: &UserId,
    guild_id: &GuildId,
    state: ListState,
    cursor: Option<PageCursor>,
    limit: i64,
) -> Result<Page<SeriesRecord>, WorkflowRepositoryError> {
    let (cursor_time, cursor_id) = split_cursor(cursor);
    let rows = sqlx::query_as::<_, SeriesRow>(
        "WITH page AS (
             SELECT scrim.guild_id,
                    scrim.id,
                    scrim.display_number,
                    scrim.title,
                    scrim.best_of,
                    scrim.hard_fearless,
                    scrim.status,
                    scrim.winning_team,
                    scrim.terminal_reason,
                    scrim.updated_at,
                    scrim.ended_at
             FROM series AS scrim
             JOIN guilds AS guild
               ON guild.guild_id = scrim.guild_id
              AND guild.status = 'ACTIVE'
             JOIN guild_members AS viewer
               ON viewer.guild_id = scrim.guild_id
              AND viewer.user_id = $1
              AND viewer.status = 'ACTIVE'
             WHERE scrim.guild_id = $2
               AND (
                   ($3 = 'active' AND scrim.status = 'IN_PROGRESS')
                   OR ($3 = 'completed' AND scrim.status = 'COMPLETED')
               )
               AND scrim.deleted_at IS NULL
               AND (
                   $4::timestamptz IS NULL
                   OR (scrim.updated_at, scrim.id) < ($4, $5::uuid)
               )
             ORDER BY scrim.updated_at DESC, scrim.id DESC
             LIMIT $6
         )
         SELECT page.id,
                page.display_number,
                page.title,
                page.best_of,
                page.hard_fearless,
                page.status,
                page.winning_team,
                page.terminal_reason,
                page.updated_at,
                page.ended_at,
                score.team_1_score,
                score.team_2_score
         FROM page
         CROSS JOIN LATERAL (
             SELECT count(*) FILTER (
                        WHERE (game.side_preset = 'DEFAULT' AND game.winning_side = 'BLUE')
                           OR (game.side_preset = 'SWAPPED' AND game.winning_side = 'RED')
                    ) AS team_1_score,
                    count(*) FILTER (
                        WHERE (game.side_preset = 'DEFAULT' AND game.winning_side = 'RED')
                           OR (game.side_preset = 'SWAPPED' AND game.winning_side = 'BLUE')
                    ) AS team_2_score
             FROM games AS game
             WHERE game.guild_id = page.guild_id
               AND game.series_id = page.id
               AND game.status = 'CONFIRMED'
         ) AS score
         ORDER BY page.updated_at DESC, page.id DESC",
    )
    .bind(user_id)
    .bind(guild_id)
    .bind(state.as_str())
    .bind(cursor_time)
    .bind(cursor_id)
    .bind(limit + 1)
    .fetch_all(pool)
    .await?;

    page_from_rows_fallible(rows, limit, SeriesRow::cursor, SeriesRow::into_record)
}

pub async fn list_actionable_auction_recruitments(
    pool: &PgPool,
    user_id: &UserId,
    guild_id: &GuildId,
    cursor: Option<PageCursor>,
    limit: i64,
) -> Result<Page<AuctionRecruitmentRecord>, WorkflowRepositoryError> {
    let (cursor_time, cursor_id) = split_cursor(cursor);
    let rows = sqlx::query_as::<_, AuctionRecruitmentRow>(
        "WITH page AS (
             SELECT recruitment.guild_id,
                    recruitment.id,
                    recruitment.display_number,
                    recruitment.title,
                    recruitment.format,
                    recruitment.target_count,
                    recruitment.semifinal_best_of,
                    recruitment.final_best_of,
                    recruitment.hard_fearless,
                    recruitment.status,
                    recruitment.revision,
                    recruitment.updated_at
             FROM auction_recruitments AS recruitment
             JOIN guilds AS guild
               ON guild.guild_id = recruitment.guild_id
              AND guild.status = 'ACTIVE'
             JOIN guild_members AS viewer
               ON viewer.guild_id = recruitment.guild_id
              AND viewer.user_id = $1
              AND viewer.status = 'ACTIVE'
             WHERE recruitment.guild_id = $2
               AND recruitment.status IN ('OPEN', 'CLOSED')
               AND recruitment.converted_at IS NULL
               AND NOT EXISTS (
                   SELECT 1
                   FROM auction_tournaments AS existing_tournament
                   WHERE existing_tournament.guild_id = recruitment.guild_id
                     AND existing_tournament.source_recruitment_id = recruitment.id
                     AND existing_tournament.status <> 'CANCELLED'
                     AND existing_tournament.deleted_at IS NULL
               )
               AND (
                   $3::timestamptz IS NULL
                   OR (recruitment.updated_at, recruitment.id) < ($3, $4::uuid)
               )
             ORDER BY recruitment.updated_at DESC, recruitment.id DESC
             LIMIT $5
         )
         SELECT page.id,
                page.display_number,
                page.title,
                page.format,
                page.target_count,
                page.semifinal_best_of,
                page.final_best_of,
                page.hard_fearless,
                page.status,
                page.revision,
                page.updated_at,
                joined.count AS participant_count
         FROM page
         CROSS JOIN LATERAL (
             SELECT count(*) AS count
             FROM auction_recruitment_participants AS participant
             WHERE participant.guild_id = page.guild_id
               AND participant.recruitment_id = page.id
               AND participant.status = 'JOINED'
         ) AS joined
         ORDER BY page.updated_at DESC, page.id DESC",
    )
    .bind(user_id)
    .bind(guild_id)
    .bind(cursor_time)
    .bind(cursor_id)
    .bind(limit + 1)
    .fetch_all(pool)
    .await?;

    page_from_rows_fallible(
        rows,
        limit,
        AuctionRecruitmentRow::cursor,
        AuctionRecruitmentRow::into_record,
    )
}

pub async fn list_auction_tournaments(
    pool: &PgPool,
    user_id: &UserId,
    guild_id: &GuildId,
    state: ListState,
    season_id: Option<SeasonId>,
    cursor: Option<PageCursor>,
    limit: i64,
) -> Result<Page<AuctionTournamentRecord>, WorkflowRepositoryError> {
    list_auction_tournaments_for_viewer(
        pool,
        Some(user_id),
        guild_id,
        state,
        season_id,
        cursor,
        limit,
    )
    .await
}

pub async fn list_auction_tournaments_for_bot(
    pool: &PgPool,
    guild_id: &GuildId,
    state: ListState,
    season_id: Option<SeasonId>,
    cursor: Option<PageCursor>,
    limit: i64,
) -> Result<Page<AuctionTournamentRecord>, WorkflowRepositoryError> {
    list_auction_tournaments_for_viewer(pool, None, guild_id, state, season_id, cursor, limit).await
}

async fn list_auction_tournaments_for_viewer(
    pool: &PgPool,
    user_id: Option<&UserId>,
    guild_id: &GuildId,
    state: ListState,
    season_id: Option<SeasonId>,
    cursor: Option<PageCursor>,
    limit: i64,
) -> Result<Page<AuctionTournamentRecord>, WorkflowRepositoryError> {
    let (cursor_time, cursor_id) = split_cursor(cursor);
    let rows = sqlx::query_as::<_, AuctionTournamentRow>(
        "WITH page AS (
             SELECT tournament.guild_id,
                    tournament.id,
                    tournament.display_number,
                    tournament.title,
                    tournament.format,
                    tournament.status,
                    tournament.revision,
                    tournament.champion_team_id,
                    tournament.updated_at,
                    tournament.ended_at
             FROM auction_tournaments AS tournament
             JOIN guilds AS guild
               ON guild.guild_id = tournament.guild_id
              AND guild.status = 'ACTIVE'
             WHERE tournament.guild_id = $2
               AND (
                   $1::text IS NULL
                   OR EXISTS (
                       SELECT 1
                       FROM guild_members AS viewer
                       WHERE viewer.guild_id = tournament.guild_id
                         AND viewer.user_id = $1
                         AND viewer.status = 'ACTIVE'
                   )
               )
               AND (
                   ($3 = 'active' AND tournament.status IN ('BIDDING', 'BRACKET_SETUP', 'IN_GAME'))
                   OR ($3 = 'completed' AND tournament.status IN ('COMPLETED', 'CANCELLED'))
               )
               AND tournament.deleted_at IS NULL
               AND (
                   $4::uuid IS NULL OR tournament.season_id = $4
               )
               AND (
                   $5::timestamptz IS NULL
                   OR (tournament.updated_at, tournament.id) < ($5, $6::uuid)
               )
             ORDER BY tournament.updated_at DESC, tournament.id DESC
             LIMIT $7
         )
         SELECT page.id,
                page.display_number,
                page.title,
                page.format,
                page.status,
                page.revision,
                page.updated_at,
                page.ended_at,
                member_count.active_count AS active_member_count,
                member_count.bid_count AS bid_member_count,
                match_count.count AS incomplete_match_count,
                page.champion_team_id,
                champion.team_name AS champion_team_name,
                next_match.id AS next_match_id,
                next_match.match_key AS next_match_key,
                next_match.status AS next_match_status,
                next_match.confirmed_game_count AS next_match_confirmed_game_count
         FROM page
         CROSS JOIN LATERAL (
             SELECT count(*) FILTER (WHERE member.released_at IS NULL) AS active_count,
                    count(*) FILTER (
                        WHERE member.released_at IS NULL AND member.acquired_via = 'BID'
                    ) AS bid_count
             FROM auction_team_members AS member
             WHERE member.guild_id = page.guild_id
               AND member.tournament_id = page.id
         ) AS member_count
         CROSS JOIN LATERAL (
             SELECT count(*) AS count
             FROM auction_matches AS auction_match
             WHERE auction_match.guild_id = page.guild_id
               AND auction_match.tournament_id = page.id
               AND auction_match.deleted_at IS NULL
               AND auction_match.status NOT IN ('COMPLETED', 'CANCELLED')
         ) AS match_count
         LEFT JOIN auction_teams AS champion
           ON champion.guild_id = page.guild_id
          AND champion.tournament_id = page.id
          AND champion.id = page.champion_team_id
         LEFT JOIN LATERAL (
             SELECT auction_match.id,
                    auction_match.match_key,
                    auction_match.status,
                    (
                        SELECT count(*)
                        FROM games AS game
                        WHERE game.guild_id = auction_match.guild_id
                          AND game.auction_match_id = auction_match.id
                          AND game.status = 'CONFIRMED'
                    ) AS confirmed_game_count
             FROM auction_matches AS auction_match
             WHERE auction_match.guild_id = page.guild_id
               AND auction_match.tournament_id = page.id
               AND auction_match.deleted_at IS NULL
               AND auction_match.status NOT IN ('COMPLETED', 'CANCELLED')
             ORDER BY CASE auction_match.status
                          WHEN 'IN_PROGRESS' THEN 0
                          WHEN 'READY' THEN 1
                          ELSE 2
                      END,
                      CASE auction_match.match_key
                          WHEN 'SEMIFINAL_A' THEN 0
                          WHEN 'SEMIFINAL_B' THEN 1
                          ELSE 2
                      END,
                      auction_match.id
             LIMIT 1
         ) AS next_match ON TRUE
         ORDER BY page.updated_at DESC, page.id DESC",
    )
    .bind(user_id.map(UserId::as_str))
    .bind(guild_id)
    .bind(state.as_str())
    .bind(season_id.map(SeasonId::as_uuid))
    .bind(cursor_time)
    .bind(cursor_id)
    .bind(limit + 1)
    .fetch_all(pool)
    .await?;

    page_from_rows_fallible(
        rows,
        limit,
        AuctionTournamentRow::cursor,
        AuctionTournamentRow::into_record,
    )
}

fn split_cursor(cursor: Option<PageCursor>) -> (Option<OffsetDateTime>, Option<Uuid>) {
    cursor.map_or((None, None), |cursor| {
        (Some(cursor.updated_at), Some(cursor.id))
    })
}

fn page_from_rows<Row, Record>(
    mut rows: Vec<Row>,
    limit: i64,
    cursor: fn(&Row) -> PageCursor,
    map: fn(Row) -> Record,
) -> Page<Record> {
    let has_next = rows.len() > limit as usize;
    if has_next {
        rows.pop();
    }
    let next_cursor = has_next.then(|| cursor(rows.last().expect("non-empty retained page")));
    Page {
        items: rows.into_iter().map(map).collect(),
        next_cursor,
    }
}

fn page_from_rows_fallible<Row, Record>(
    mut rows: Vec<Row>,
    limit: i64,
    cursor: fn(&Row) -> PageCursor,
    map: fn(Row) -> Result<Record, WorkflowRepositoryError>,
) -> Result<Page<Record>, WorkflowRepositoryError> {
    let has_next = rows.len() > limit as usize;
    if has_next {
        rows.pop();
    }
    let next_cursor = has_next.then(|| cursor(rows.last().expect("non-empty retained page")));
    Ok(Page {
        items: rows.into_iter().map(map).collect::<Result<_, _>>()?,
        next_cursor,
    })
}

fn parse_auction_format(value: &str) -> Result<AuctionFormat, WorkflowRepositoryError> {
    match value {
        "TEN_PERSON" => Ok(AuctionFormat::TenPerson),
        "TWENTY_PERSON" => Ok(AuctionFormat::TwentyPerson),
        _ => Err(WorkflowRepositoryError::InvalidProjection(
            "auction format is not canonical",
        )),
    }
}

#[derive(Debug, FromRow)]
struct OperationRow {
    entity_id: Uuid,
    source_kind: String,
    title: String,
    updated_at: OffsetDateTime,
    first_count: i64,
    second_count: i64,
    third_count: i64,
    source_status: Option<String>,
    auxiliary_status: Option<String>,
    auxiliary_id: Option<AuctionMatchId>,
}

impl OperationRow {
    fn into_record(self, guild_id: &GuildId) -> Result<OperationRecord, WorkflowRepositoryError> {
        let guild = guild_id.as_str();
        let (id, kind, status, detail, href, priority) = match self.source_kind.as_str() {
            "ENTRY" => {
                let unassigned = (self.first_count - self.second_count).max(0);
                (
                    format!("entry:{}", self.entity_id),
                    OperationKind::Entry,
                    "편성 필요",
                    format!(
                        "{}/{}명 · {}명 미배정",
                        self.first_count, self.third_count, unassigned
                    ),
                    format!("/g/{guild}/recruitments/{}/entry", self.entity_id),
                    OperationPriority::Urgent,
                )
            }
            "PICK_BAN" => {
                let next_game = self.first_count + 1;
                let detail = if next_game <= self.second_count {
                    format!(
                        "{}세트 완료 · {}세트 입력 대기",
                        self.first_count, next_game
                    )
                } else {
                    format!("{}세트 완료 · 완료 처리 대기", self.first_count)
                };
                (
                    format!("series:{}", self.entity_id),
                    OperationKind::PickBan,
                    "픽밴/결과 입력",
                    detail,
                    format!("/g/{guild}/series/{}/pick-ban", self.entity_id),
                    OperationPriority::Urgent,
                )
            }
            "AUCTION_RECRUITMENT" => (
                format!("auction-recruitment:{}", self.entity_id),
                OperationKind::Auction,
                "팀장 설정",
                format!(
                    "{}/{}명 · {}팀",
                    self.first_count, self.third_count, self.second_count
                ),
                format!("/g/{guild}/auction/recruitments/{}/draft", self.entity_id),
                OperationPriority::Normal,
            ),
            "AUCTION_TOURNAMENT" => {
                let source_status = self.source_status.as_deref().ok_or(
                    WorkflowRepositoryError::InvalidProjection(
                        "auction operation has no tournament status",
                    ),
                )?;
                let (status, detail, href) = match source_status {
                    "BIDDING" => (
                        "입찰 진행",
                        format!("{}팀 · {}명 편성", self.second_count, self.first_count),
                        format!("/g/{guild}/auction/tournaments/{}/draft", self.entity_id),
                    ),
                    "BRACKET_SETUP" => (
                        "대진 설정",
                        format!("{}팀 · 대진 확정 대기", self.second_count),
                        format!("/g/{guild}/auction/tournaments/{}/bracket", self.entity_id),
                    ),
                    "IN_GAME" => {
                        let wants_result = matches!(
                            self.auxiliary_status.as_deref(),
                            Some("READY" | "IN_PROGRESS")
                        );
                        let href = if wants_result {
                            let match_id = self.auxiliary_id.ok_or(
                                WorkflowRepositoryError::InvalidProjection(
                                    "auction operation has no next match id",
                                ),
                            )?;
                            format!("/g/{guild}/auction/matches/{match_id}")
                        } else {
                            format!("/g/{guild}/auction/tournaments/{}/bracket", self.entity_id)
                        };
                        (
                            if wants_result {
                                "결과 입력"
                            } else {
                                "대진 진행"
                            },
                            format!("{}경기 진행 또는 대기", self.third_count),
                            href,
                        )
                    }
                    _ => {
                        return Err(WorkflowRepositoryError::InvalidProjection(
                            "auction operation status is not active",
                        ));
                    }
                };
                (
                    format!("auction-tournament:{}", self.entity_id),
                    OperationKind::Auction,
                    status,
                    detail,
                    href,
                    OperationPriority::Normal,
                )
            }
            _ => {
                return Err(WorkflowRepositoryError::InvalidProjection(
                    "operation kind is not canonical",
                ));
            }
        };

        Ok(OperationRecord {
            id,
            kind,
            title: self.title,
            status,
            detail,
            updated_at: self.updated_at,
            href,
            priority,
        })
    }
}

#[derive(Debug, FromRow)]
struct RecruitmentRow {
    id: RecruitmentId,
    display_number: i64,
    title: String,
    status: RecruitmentStatus,
    target_count: i16,
    best_of: i16,
    hard_fearless: bool,
    revision: i64,
    updated_at: OffsetDateTime,
    participant_count: i64,
    assigned_count: i64,
}

impl RecruitmentRow {
    fn cursor(&self) -> PageCursor {
        PageCursor {
            updated_at: self.updated_at,
            id: self.id.as_uuid(),
        }
    }

    fn into_record(self) -> RecruitmentRecord {
        RecruitmentRecord {
            id: self.id,
            display_number: self.display_number,
            title: self.title,
            status: self.status,
            target_count: self.target_count,
            best_of: self.best_of,
            hard_fearless: self.hard_fearless,
            participant_count: self.participant_count,
            assigned_count: self.assigned_count,
            revision: self.revision,
            updated_at: self.updated_at,
        }
    }
}

#[derive(Debug, FromRow)]
struct SeriesRow {
    id: SeriesId,
    display_number: i64,
    title: String,
    best_of: i16,
    hard_fearless: bool,
    status: SeriesStatus,
    winning_team: Option<String>,
    terminal_reason: Option<String>,
    updated_at: OffsetDateTime,
    ended_at: Option<OffsetDateTime>,
    team_1_score: i64,
    team_2_score: i64,
}

impl SeriesRow {
    fn cursor(&self) -> PageCursor {
        PageCursor {
            updated_at: self.updated_at,
            id: self.id.as_uuid(),
        }
    }

    fn into_record(self) -> Result<SeriesRecord, WorkflowRepositoryError> {
        let (outcome, winner) = match (
            self.status,
            self.terminal_reason.as_deref(),
            self.winning_team.as_deref(),
        ) {
            (SeriesStatus::InProgress, None, None) => (None, None),
            (SeriesStatus::Completed, Some("SCORE_REACHED" | "EARLY_WIN"), Some("TEAM_1")) => {
                (Some(SeriesOutcome::Team1), Some("TEAM_1"))
            }
            (SeriesStatus::Completed, Some("SCORE_REACHED" | "EARLY_WIN"), Some("TEAM_2")) => {
                (Some(SeriesOutcome::Team2), Some("TEAM_2"))
            }
            (SeriesStatus::Completed, Some("EARLY_DRAW"), None) => {
                (Some(SeriesOutcome::Draw), None)
            }
            (SeriesStatus::Cancelled, Some("CANCELLED"), None) => {
                (Some(SeriesOutcome::Cancel), None)
            }
            _ => {
                return Err(WorkflowRepositoryError::InvalidProjection(
                    "series terminal projection is inconsistent",
                ));
            }
        };

        Ok(SeriesRecord {
            id: self.id,
            display_number: self.display_number,
            title: self.title,
            best_of: self.best_of,
            hard_fearless: self.hard_fearless,
            status: self.status,
            team_1_score: self.team_1_score,
            team_2_score: self.team_2_score,
            outcome,
            winner,
            terminal_reason: self.terminal_reason,
            current_set_number: (self.status == SeriesStatus::InProgress)
                .then(|| self.team_1_score + self.team_2_score + 1)
                .filter(|set_number| *set_number <= i64::from(self.best_of))
                .and_then(|set_number| i16::try_from(set_number).ok()),
            updated_at: self.updated_at,
            played_at: self.ended_at,
        })
    }
}

#[derive(Debug, FromRow)]
struct AuctionRecruitmentRow {
    id: AuctionRecruitmentId,
    display_number: i64,
    title: String,
    format: String,
    target_count: i16,
    semifinal_best_of: Option<i16>,
    final_best_of: i16,
    hard_fearless: bool,
    status: RecruitmentStatus,
    revision: i64,
    updated_at: OffsetDateTime,
    participant_count: i64,
}

impl AuctionRecruitmentRow {
    fn cursor(&self) -> PageCursor {
        PageCursor {
            updated_at: self.updated_at,
            id: self.id.as_uuid(),
        }
    }

    fn into_record(self) -> Result<AuctionRecruitmentRecord, WorkflowRepositoryError> {
        let format = parse_auction_format(&self.format)?;
        if self.target_count != format.participant_count() {
            return Err(WorkflowRepositoryError::InvalidProjection(
                "auction recruitment target does not match format",
            ));
        }
        if !matches!(self.final_best_of, 1 | 3 | 5) {
            return Err(WorkflowRepositoryError::InvalidProjection(
                "auction recruitment final best-of is invalid",
            ));
        }
        let semifinal_is_valid = match format {
            AuctionFormat::TenPerson => self.semifinal_best_of.is_none(),
            AuctionFormat::TwentyPerson => self
                .semifinal_best_of
                .is_some_and(|best_of| matches!(best_of, 1 | 3 | 5)),
        };
        if !semifinal_is_valid {
            return Err(WorkflowRepositoryError::InvalidProjection(
                "auction recruitment semifinal best-of does not match format",
            ));
        }
        Ok(AuctionRecruitmentRecord {
            id: self.id,
            display_number: self.display_number,
            title: self.title,
            format,
            semifinal_best_of: self.semifinal_best_of,
            final_best_of: self.final_best_of,
            hard_fearless: self.hard_fearless,
            target_count: self.target_count,
            participant_count: self.participant_count,
            team_count: format.team_count(),
            status: self.status,
            revision: self.revision,
            updated_at: self.updated_at,
        })
    }
}

#[derive(Debug, FromRow)]
struct AuctionTournamentRow {
    id: AuctionTournamentId,
    display_number: i64,
    title: String,
    format: String,
    status: AuctionTournamentStatus,
    revision: i64,
    updated_at: OffsetDateTime,
    ended_at: Option<OffsetDateTime>,
    active_member_count: i64,
    bid_member_count: i64,
    incomplete_match_count: i64,
    champion_team_id: Option<Uuid>,
    champion_team_name: Option<String>,
    next_match_id: Option<AuctionMatchId>,
    next_match_key: Option<String>,
    next_match_status: Option<String>,
    next_match_confirmed_game_count: Option<i64>,
}

impl AuctionTournamentRow {
    fn cursor(&self) -> PageCursor {
        PageCursor {
            updated_at: self.updated_at,
            id: self.id.as_uuid(),
        }
    }

    fn into_record(self) -> Result<AuctionTournamentRecord, WorkflowRepositoryError> {
        let format = parse_auction_format(&self.format)?;
        let next_match = match (
            self.next_match_id,
            self.next_match_key,
            self.next_match_status,
            self.next_match_confirmed_game_count,
        ) {
            (Some(id), Some(match_key), Some(status), Some(confirmed_game_count)) => {
                Some(AuctionMatchSummaryRecord {
                    id,
                    match_key,
                    status,
                    confirmed_game_count,
                })
            }
            (None, None, None, None) => None,
            _ => {
                return Err(WorkflowRepositoryError::InvalidProjection(
                    "auction next match projection is incomplete",
                ));
            }
        };
        let champion_shape_is_valid = matches!(
            (&self.champion_team_id, &self.champion_team_name),
            (Some(_), Some(_)) | (None, None)
        );
        if !champion_shape_is_valid {
            return Err(WorkflowRepositoryError::InvalidProjection(
                "auction champion projection is incomplete",
            ));
        }

        Ok(AuctionTournamentRecord {
            id: self.id,
            display_number: self.display_number,
            title: self.title,
            format,
            status: self.status,
            revision: self.revision,
            participant_count: format.participant_count(),
            team_count: format.team_count(),
            active_member_count: self.active_member_count,
            bid_member_count: self.bid_member_count,
            incomplete_match_count: self.incomplete_match_count,
            champion_team_id: self.champion_team_id,
            champion_team_name: self.champion_team_name,
            next_match,
            updated_at: self.updated_at,
            ended_at: self.ended_at,
        })
    }
}

#[derive(Debug, Error)]
pub enum WorkflowRepositoryError {
    #[error("workflow read projection is invalid: {0}")]
    InvalidProjection(&'static str),
    #[error("PostgreSQL workflow read operation failed")]
    Sql(#[from] sqlx::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auction_formats_define_the_supported_roster_shapes() {
        assert_eq!(AuctionFormat::TenPerson.participant_count(), 10);
        assert_eq!(AuctionFormat::TenPerson.team_count(), 2);
        assert_eq!(AuctionFormat::TwentyPerson.participant_count(), 20);
        assert_eq!(AuctionFormat::TwentyPerson.team_count(), 4);
    }

    #[test]
    fn cursored_page_keeps_the_last_returned_row_as_the_next_boundary() {
        #[derive(Clone)]
        struct Row {
            id: Uuid,
            updated_at: OffsetDateTime,
        }

        let now = OffsetDateTime::now_utc();
        let rows = (0..3)
            .map(|_| Row {
                id: Uuid::now_v7(),
                updated_at: now,
            })
            .collect::<Vec<_>>();
        let expected = rows[1].id;
        let page = page_from_rows(
            rows,
            2,
            |row| PageCursor {
                id: row.id,
                updated_at: row.updated_at,
            },
            |row| row.id,
        );

        assert_eq!(page.items.len(), 2);
        assert_eq!(page.next_cursor.map(|cursor| cursor.id), Some(expected));
    }
}
