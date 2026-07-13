use axum::{
    Extension,
    extract::{Path, Query, State},
    http::StatusCode,
    response::Response,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

use crate::{
    domain::{GuildId, Lane, SeasonId},
    error::{self, ApiError},
    http::extractors::AuthenticatedSession,
    repository::{
        champion_preferences::{
            self, ChampionPreferencesRepositoryError, ChampionPreferencesSnapshot,
        },
        guild_record_profiles::{
            self, GuildRecord, GuildRecordChampion, GuildRecordCursor, GuildRecordGame,
            GuildRecordKind, GuildRecordLaneMmr, GuildRecordLineup, GuildRecordProfileLookup,
            GuildRecordProfileMember, GuildRecordProfileRepositoryError, GuildRecordSeason,
            GuildRecordSeries, GuildRecordSummary, GuildRecordTournament,
            GuildRecordTournamentSeries, GuildRecordsSearchLookup, VisibleRiotAccount,
        },
    },
    state::{AppState, RequestId},
};

const MAX_QUERY_CHARS: usize = 100;
const DEFAULT_PAGE_SIZE: u16 = 20;
const MAX_PAGE_SIZE: u16 = 50;
const MAX_CURSOR_BYTES: usize = 512;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct GuildRecordProfileQuery {
    q: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct GuildRecordsSearchQuery {
    q: String,
    season_id: Option<String>,
    cursor: Option<String>,
    limit: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GuildRecordCursorToken {
    played_at_nanos: i64,
    id: Uuid,
    kind: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GuildRecordProfileData {
    member: GuildRecordProfileMemberData,
    champion_preferences: ChampionPreferencesData,
    riot_accounts: Vec<VisibleRiotAccountData>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GuildRecordProfileMemberData {
    display_name: String,
    avatar_url: Option<String>,
    main_lane: Option<Lane>,
}

#[derive(Serialize)]
struct ChampionPreferencesData {
    #[serde(rename = "TOP")]
    top: Vec<String>,
    #[serde(rename = "JUNGLE")]
    jungle: Vec<String>,
    #[serde(rename = "MID")]
    mid: Vec<String>,
    #[serde(rename = "BOTTOM")]
    bottom: Vec<String>,
    #[serde(rename = "SUPPORT")]
    support: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VisibleRiotAccountData {
    platform: String,
    game_name: String,
    tag_line: String,
    profile_icon_id: Option<i32>,
    is_main: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GuildRecordsSearchData {
    member: GuildRecordProfileMemberData,
    champion_preferences: ChampionPreferencesData,
    riot_accounts: Vec<VisibleRiotAccountData>,
    seasons: Vec<GuildRecordSeasonData>,
    selected_season_id: Option<String>,
    summary: GuildRecordSummaryData,
    lane_mmr: Vec<GuildRecordLaneMmrData>,
    champion_records: Vec<GuildRecordChampionData>,
    records: Vec<GuildRecordData>,
    next_cursor: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GuildRecordSeasonData {
    id: String,
    name: String,
    status: String,
    started_at: String,
    ended_at: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GuildRecordSummaryData {
    games: i64,
    wins: i64,
    losses: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GuildRecordLaneMmrData {
    lane: Lane,
    mmr: f64,
    games: i32,
    wins: i32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GuildRecordChampionData {
    champion_id: String,
    lane: Lane,
    games: i64,
    wins: i64,
    losses: i64,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "UPPERCASE")]
enum GuildRecordData {
    Series(GuildRecordSeriesData),
    Tournament(GuildRecordTournamentData),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GuildRecordSeriesData {
    id: String,
    source: String,
    title: String,
    participant_count: u8,
    played_at: String,
    result: &'static str,
    team_name: Option<String>,
    opponent_name: Option<String>,
    stage: Option<String>,
    wins: i64,
    losses: i64,
    mmr_delta: Option<f64>,
    games: Vec<GuildRecordGameData>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GuildRecordTournamentData {
    id: String,
    title: String,
    participant_count: u8,
    played_at: String,
    team_name: Option<String>,
    spent_points: Option<i32>,
    placement: Option<i16>,
    wins: i64,
    losses: i64,
    mmr_delta: Option<f64>,
    roster: Vec<GuildRecordLineupData>,
    series: Vec<GuildRecordTournamentSeriesData>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GuildRecordTournamentSeriesData {
    id: String,
    title: String,
    stage: String,
    opponent_name: Option<String>,
    played_at: String,
    result: &'static str,
    wins: i64,
    losses: i64,
    mmr_delta: Option<f64>,
    games: Vec<GuildRecordGameData>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GuildRecordGameData {
    id: String,
    game_number: i16,
    played_at: String,
    winning_side: String,
    target: GuildRecordTargetData,
    blue_team: Vec<GuildRecordLineupData>,
    red_team: Vec<GuildRecordLineupData>,
    blue_bans: Vec<String>,
    red_bans: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GuildRecordTargetData {
    side: String,
    lane: Lane,
    champion_id: Option<String>,
    mmr_before: Option<f64>,
    mmr_after: Option<f64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GuildRecordLineupData {
    display_name: String,
    lane: Lane,
    champion_id: Option<String>,
    is_target: bool,
}

pub async fn profile(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(guild_id): Path<GuildId>,
    principal: AuthenticatedSession,
    Query(query): Query<GuildRecordProfileQuery>,
) -> Result<Response, ApiError> {
    let query = validated_query(&query.q, &request_id)?;
    let pool = database(&state, &request_id)?;
    let member = match guild_record_profiles::find_member(
        pool,
        &principal.session.user_id,
        &guild_id,
        query,
    )
    .await
    .map_err(|source| profile_repository_error(&source, &request_id))?
    {
        GuildRecordProfileLookup::Found(member) => member,
        GuildRecordProfileLookup::Ambiguous => return Err(ambiguous_member(&request_id)),
        GuildRecordProfileLookup::NotFound | GuildRecordProfileLookup::NotMember => {
            return Err(hidden_not_found(&request_id));
        }
    };

    let preferences = champion_preferences::get(pool, &member.user_id)
        .await
        .map_err(|source| champion_repository_error(&source, &request_id))?;
    let riot_accounts = guild_record_profiles::list_visible_riot_accounts(pool, &member.user_id)
        .await
        .map_err(|source| profile_repository_error(&source, &request_id))?;
    if member.matched_by_riot_id && riot_accounts.is_empty() {
        return Err(hidden_not_found(&request_id));
    }

    Ok(error::success(
        StatusCode::OK,
        &request_id,
        GuildRecordProfileData {
            member: map_member(member),
            champion_preferences: map_preferences(preferences),
            riot_accounts: riot_accounts.into_iter().map(map_riot_account).collect(),
        },
    ))
}

pub async fn search(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(guild_id): Path<GuildId>,
    principal: AuthenticatedSession,
    Query(query): Query<GuildRecordsSearchQuery>,
) -> Result<Response, ApiError> {
    let search_query = validated_query(&query.q, &request_id)?;
    let season_id = query
        .season_id
        .as_deref()
        .map(str::parse::<SeasonId>)
        .transpose()
        .map_err(|_| invalid_season_id(&request_id))?;
    let (cursor, limit) = page_parameters(&query, &request_id)?;
    let snapshot = match guild_record_profiles::search(
        database(&state, &request_id)?,
        &principal.session.user_id,
        &guild_id,
        search_query,
        season_id,
        cursor,
        limit,
    )
    .await
    .map_err(|source| profile_repository_error(&source, &request_id))?
    {
        GuildRecordsSearchLookup::Found(snapshot) => snapshot,
        GuildRecordsSearchLookup::Ambiguous => return Err(ambiguous_member(&request_id)),
        GuildRecordsSearchLookup::NotFound | GuildRecordsSearchLookup::NotMember => {
            return Err(hidden_not_found(&request_id));
        }
        GuildRecordsSearchLookup::SeasonNotFound => return Err(season_not_found(&request_id)),
    };
    let next_cursor = snapshot
        .next_cursor
        .map(encode_cursor)
        .transpose()
        .map_err(|()| invalid_projection(&request_id))?;
    let data = GuildRecordsSearchData {
        member: map_member(snapshot.member),
        champion_preferences: map_preferences(snapshot.champion_preferences),
        riot_accounts: snapshot
            .riot_accounts
            .into_iter()
            .map(map_riot_account)
            .collect(),
        seasons: snapshot
            .seasons
            .into_iter()
            .map(|season| map_season(season, &request_id))
            .collect::<Result<_, _>>()?,
        selected_season_id: snapshot.selected_season_id.map(|id| id.to_string()),
        summary: map_summary(snapshot.summary),
        lane_mmr: snapshot.lane_mmr.into_iter().map(map_lane_mmr).collect(),
        champion_records: snapshot
            .champion_records
            .into_iter()
            .map(map_champion_record)
            .collect(),
        records: snapshot
            .records
            .into_iter()
            .map(|record| map_record(record, &request_id))
            .collect::<Result<_, _>>()?,
        next_cursor,
    };
    Ok(error::success(StatusCode::OK, &request_id, data))
}

fn page_parameters(
    query: &GuildRecordsSearchQuery,
    request_id: &RequestId,
) -> Result<(Option<GuildRecordCursor>, i64), ApiError> {
    let limit = query
        .limit
        .as_deref()
        .map(str::parse::<u16>)
        .transpose()
        .map_err(|_| invalid_limit(request_id))?
        .unwrap_or(DEFAULT_PAGE_SIZE);
    if !(1..=MAX_PAGE_SIZE).contains(&limit) {
        return Err(invalid_limit(request_id));
    }
    let cursor = query
        .cursor
        .as_deref()
        .map(decode_cursor)
        .transpose()
        .map_err(|()| invalid_cursor(request_id))?;
    Ok((cursor, i64::from(limit)))
}

fn encode_cursor(cursor: GuildRecordCursor) -> Result<String, ()> {
    let played_at_nanos = i64::try_from(cursor.played_at.unix_timestamp_nanos()).map_err(|_| ())?;
    let payload = serde_json::to_vec(&GuildRecordCursorToken {
        played_at_nanos,
        id: cursor.id,
        kind: cursor.kind.as_str().to_owned(),
    })
    .map_err(|_| ())?;
    Ok(URL_SAFE_NO_PAD.encode(payload))
}

fn decode_cursor(value: &str) -> Result<GuildRecordCursor, ()> {
    if value.is_empty() || value.len() > MAX_CURSOR_BYTES {
        return Err(());
    }
    let bytes = URL_SAFE_NO_PAD.decode(value).map_err(|_| ())?;
    if bytes.len() > MAX_CURSOR_BYTES {
        return Err(());
    }
    let token = serde_json::from_slice::<GuildRecordCursorToken>(&bytes).map_err(|_| ())?;
    let kind = match token.kind.as_str() {
        "SERIES" => GuildRecordKind::Series,
        "TOURNAMENT" => GuildRecordKind::Tournament,
        _ => return Err(()),
    };
    let played_at = OffsetDateTime::from_unix_timestamp_nanos(i128::from(token.played_at_nanos))
        .map_err(|_| ())?;
    let cursor = GuildRecordCursor {
        played_at,
        id: token.id,
        kind,
    };
    if encode_cursor(cursor).as_deref() != Ok(value) {
        return Err(());
    }
    Ok(cursor)
}

fn validated_query<'a>(value: &'a str, request_id: &RequestId) -> Result<&'a str, ApiError> {
    let query = value.trim();
    if query.is_empty() || query.chars().count() > MAX_QUERY_CHARS {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            request_id,
            "invalid_search_query",
            "검색어가 올바르지 않습니다.",
            Some(serde_json::json!({ "maxLength": MAX_QUERY_CHARS })),
        ));
    }
    Ok(query)
}

fn map_member(member: GuildRecordProfileMember) -> GuildRecordProfileMemberData {
    let avatar_url = member
        .avatar_hash
        .as_deref()
        .and_then(|hash| discord_avatar_url(member.user_id.as_str(), hash));
    GuildRecordProfileMemberData {
        display_name: member.display_name,
        avatar_url,
        main_lane: member.main_lane,
    }
}

fn map_preferences(snapshot: ChampionPreferencesSnapshot) -> ChampionPreferencesData {
    ChampionPreferencesData {
        top: snapshot.top,
        jungle: snapshot.jungle,
        mid: snapshot.mid,
        bottom: snapshot.bottom,
        support: snapshot.support,
    }
}

fn map_riot_account(account: VisibleRiotAccount) -> VisibleRiotAccountData {
    VisibleRiotAccountData {
        platform: account.platform,
        game_name: account.game_name,
        tag_line: account.tag_line,
        profile_icon_id: account.profile_icon_id,
        is_main: account.is_main,
    }
}

fn map_season(
    season: GuildRecordSeason,
    request_id: &RequestId,
) -> Result<GuildRecordSeasonData, ApiError> {
    Ok(GuildRecordSeasonData {
        id: season.id.to_string(),
        name: season.name,
        status: season.status,
        started_at: format_timestamp(season.started_at, request_id)?,
        ended_at: season
            .ended_at
            .map(|timestamp| format_timestamp(timestamp, request_id))
            .transpose()?,
    })
}

fn map_summary(summary: GuildRecordSummary) -> GuildRecordSummaryData {
    GuildRecordSummaryData {
        games: summary.games,
        wins: summary.wins,
        losses: summary.losses,
    }
}

fn map_lane_mmr(record: GuildRecordLaneMmr) -> GuildRecordLaneMmrData {
    GuildRecordLaneMmrData {
        lane: record.lane,
        mmr: record.mmr,
        games: record.games,
        wins: record.wins,
    }
}

fn map_champion_record(record: GuildRecordChampion) -> GuildRecordChampionData {
    GuildRecordChampionData {
        champion_id: record.champion_id,
        lane: record.lane,
        games: record.games,
        wins: record.wins,
        losses: record.losses,
    }
}

fn map_record(record: GuildRecord, request_id: &RequestId) -> Result<GuildRecordData, ApiError> {
    match record {
        GuildRecord::Series(series) => Ok(GuildRecordData::Series(map_series(series, request_id)?)),
        GuildRecord::Tournament(tournament) => Ok(GuildRecordData::Tournament(map_tournament(
            tournament, request_id,
        )?)),
    }
}

fn map_series(
    series: GuildRecordSeries,
    request_id: &RequestId,
) -> Result<GuildRecordSeriesData, ApiError> {
    Ok(GuildRecordSeriesData {
        id: series.id.to_string(),
        source: series.source,
        title: series.title,
        participant_count: 10,
        played_at: format_timestamp(series.played_at, request_id)?,
        result: series.result.as_str(),
        team_name: series.team_name,
        opponent_name: series.opponent_name,
        stage: series.stage,
        wins: series.wins,
        losses: series.losses,
        mmr_delta: series.mmr_delta,
        games: series
            .games
            .into_iter()
            .map(|game| map_game(game, request_id))
            .collect::<Result<_, _>>()?,
    })
}

fn map_tournament(
    tournament: GuildRecordTournament,
    request_id: &RequestId,
) -> Result<GuildRecordTournamentData, ApiError> {
    Ok(GuildRecordTournamentData {
        id: tournament.id.to_string(),
        title: tournament.title,
        participant_count: 20,
        played_at: format_timestamp(tournament.played_at, request_id)?,
        team_name: tournament.team_name,
        spent_points: tournament.spent_points,
        placement: tournament.placement,
        wins: tournament.wins,
        losses: tournament.losses,
        mmr_delta: tournament.mmr_delta,
        roster: tournament.roster.into_iter().map(map_lineup).collect(),
        series: tournament
            .series
            .into_iter()
            .map(|series| map_tournament_series(series, request_id))
            .collect::<Result<_, _>>()?,
    })
}

fn map_tournament_series(
    series: GuildRecordTournamentSeries,
    request_id: &RequestId,
) -> Result<GuildRecordTournamentSeriesData, ApiError> {
    Ok(GuildRecordTournamentSeriesData {
        id: series.id.to_string(),
        title: series.title,
        stage: series.stage,
        opponent_name: series.opponent_name,
        played_at: format_timestamp(series.played_at, request_id)?,
        result: series.result.as_str(),
        wins: series.wins,
        losses: series.losses,
        mmr_delta: series.mmr_delta,
        games: series
            .games
            .into_iter()
            .map(|game| map_game(game, request_id))
            .collect::<Result<_, _>>()?,
    })
}

fn map_game(
    game: GuildRecordGame,
    request_id: &RequestId,
) -> Result<GuildRecordGameData, ApiError> {
    Ok(GuildRecordGameData {
        id: game.id.to_string(),
        game_number: game.game_number,
        played_at: format_timestamp(game.played_at, request_id)?,
        winning_side: game.winning_side,
        target: GuildRecordTargetData {
            side: game.target_side,
            lane: game.target_lane,
            champion_id: game.target_champion_id,
            mmr_before: game.mmr_before,
            mmr_after: game.mmr_after,
        },
        blue_team: game.blue_team.into_iter().map(map_lineup).collect(),
        red_team: game.red_team.into_iter().map(map_lineup).collect(),
        blue_bans: game.blue_bans,
        red_bans: game.red_bans,
    })
}

fn map_lineup(lineup: GuildRecordLineup) -> GuildRecordLineupData {
    GuildRecordLineupData {
        display_name: lineup.display_name,
        lane: lineup.lane,
        champion_id: lineup.champion_id,
        is_target: lineup.is_target,
    }
}

fn format_timestamp(value: OffsetDateTime, request_id: &RequestId) -> Result<String, ApiError> {
    value.format(&Rfc3339).map_err(|error| {
        tracing::error!(%error, "failed to format guild record timestamp");
        invalid_projection(request_id)
    })
}

fn discord_avatar_url(user_id: &str, hash: &str) -> Option<String> {
    let valid_hash = !hash.is_empty()
        && hash.len() <= 128
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'_');
    valid_hash.then(|| format!("https://cdn.discordapp.com/avatars/{user_id}/{hash}.webp?size=128"))
}

fn database<'a>(state: &'a AppState, request_id: &RequestId) -> Result<&'a PgPool, ApiError> {
    state.postgres().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            request_id,
            "database_unavailable",
            "데이터베이스를 일시적으로 사용할 수 없습니다.",
            None,
        )
    })
}

fn profile_repository_error(
    source: &GuildRecordProfileRepositoryError,
    request_id: &RequestId,
) -> ApiError {
    match source {
        GuildRecordProfileRepositoryError::Sql(source) => {
            error::database_failure(source, request_id)
        }
        GuildRecordProfileRepositoryError::StoredInvariantViolation => {
            tracing::error!(error = ?source, "guild record profile invariant failed");
            internal(request_id)
        }
    }
}

fn champion_repository_error(
    source: &ChampionPreferencesRepositoryError,
    request_id: &RequestId,
) -> ApiError {
    match source {
        ChampionPreferencesRepositoryError::Sql(source) => {
            error::database_failure(source, request_id)
        }
        _ => {
            tracing::error!(error = ?source, "guild record champion preference read failed");
            internal(request_id)
        }
    }
}

fn hidden_not_found(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        request_id,
        "not_found",
        "요청한 길드 멤버를 찾을 수 없습니다.",
        None,
    )
}

fn ambiguous_member(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::CONFLICT,
        request_id,
        "ambiguous_guild_member",
        "같은 이름의 길드 멤버가 여러 명입니다.",
        None,
    )
}

fn season_not_found(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        request_id,
        "season_not_found",
        "요청한 시즌을 찾을 수 없습니다.",
        None,
    )
}

fn invalid_season_id(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        request_id,
        "invalid_season_id",
        "seasonId 형식이 올바르지 않습니다.",
        None,
    )
}

fn invalid_limit(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        request_id,
        "invalid_limit",
        "limit은 1 이상 50 이하이어야 합니다.",
        None,
    )
}

fn invalid_cursor(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        request_id,
        "invalid_cursor",
        "cursor 형식이 올바르지 않습니다.",
        None,
    )
}

fn invalid_projection(request_id: &RequestId) -> ApiError {
    tracing::error!("guild records projection invariant failed");
    internal(request_id)
}

fn internal(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        request_id,
        "internal_error",
        "요청을 처리하는 중 내부 오류가 발생했습니다.",
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_query_is_trimmed_bounded_and_non_empty() {
        let request_id = RequestId("request-id".to_owned());
        assert_eq!(
            validated_query("  Longcat  ", &request_id).expect("valid query"),
            "Longcat"
        );
        assert!(validated_query("   ", &request_id).is_err());
        assert!(validated_query(&"a".repeat(MAX_QUERY_CHARS + 1), &request_id).is_err());
    }

    #[test]
    fn discord_avatar_url_rejects_non_provider_hashes() {
        assert_eq!(
            discord_avatar_url("100000000000000001", "a_0123abcdef").as_deref(),
            Some(
                "https://cdn.discordapp.com/avatars/100000000000000001/a_0123abcdef.webp?size=128"
            )
        );
        assert_eq!(
            discord_avatar_url("100000000000000001", "../../secret"),
            None
        );
    }

    #[test]
    fn record_cursor_round_trips_and_rejects_unknown_kinds() {
        let cursor = GuildRecordCursor {
            played_at: OffsetDateTime::from_unix_timestamp(1_700_000_000).expect("test timestamp"),
            id: Uuid::now_v7(),
            kind: GuildRecordKind::Tournament,
        };
        let encoded = encode_cursor(cursor).expect("cursor encodes");
        assert_eq!(decode_cursor(&encoded), Ok(cursor));

        let invalid = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&GuildRecordCursorToken {
                played_at_nanos: 1,
                id: Uuid::now_v7(),
                kind: "UNKNOWN".to_owned(),
            })
            .expect("invalid token serializes"),
        );
        assert!(decode_cursor(&invalid).is_err());
        assert!(decode_cursor("not-base64!").is_err());
    }

    #[test]
    fn record_page_limit_is_bounded() {
        let request_id = RequestId("request-id".to_owned());
        let query = |limit: Option<&str>| GuildRecordsSearchQuery {
            q: "Longcat".to_owned(),
            season_id: None,
            cursor: None,
            limit: limit.map(str::to_owned),
        };
        assert_eq!(
            page_parameters(&query(None), &request_id)
                .expect("default page")
                .1,
            i64::from(DEFAULT_PAGE_SIZE)
        );
        assert_eq!(
            page_parameters(&query(Some("50")), &request_id)
                .expect("maximum page")
                .1,
            50
        );
        assert!(page_parameters(&query(Some("0")), &request_id).is_err());
        assert!(page_parameters(&query(Some("51")), &request_id).is_err());
        assert!(page_parameters(&query(Some("nope")), &request_id).is_err());
    }
}
