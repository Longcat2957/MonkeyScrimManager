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
    domain::{AuctionTournamentStatus, GuildId},
    error::{self, ApiError},
    http::extractors::AuthenticatedSession,
    repository::workflow::{
        self, AuctionMatchSummaryRecord, AuctionRecruitmentRecord, AuctionTournamentRecord,
        ListState, OperationRecord, Page, PageCursor, RecruitmentRecord, SeriesRecord,
        WorkflowRepositoryError,
    },
    state::{AppState, RequestId},
};

const DEFAULT_PAGE_SIZE: u16 = 50;
const MAX_PAGE_SIZE: u16 = 100;

#[derive(Serialize)]
struct OperationsData {
    operations: Vec<OperationData>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OperationData {
    id: String,
    kind: &'static str,
    title: String,
    status: &'static str,
    detail: String,
    updated_at: String,
    href: String,
    priority: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecruitmentListData {
    recruitments: Vec<RecruitmentData>,
    next_cursor: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecruitmentData {
    id: String,
    display_number: i64,
    title: String,
    status: &'static str,
    target_count: i16,
    joined_count: i64,
    assigned_count: i64,
    revision: i64,
    updated_at: String,
    href: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SeriesListData {
    series: Vec<SeriesData>,
    next_cursor: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SeriesData {
    id: String,
    display_number: i64,
    title: String,
    best_of: i16,
    status: &'static str,
    team_1_score: i64,
    team_2_score: i64,
    winning_team: Option<&'static str>,
    terminal_reason: Option<String>,
    current_set_number: Option<i16>,
    outcome: Option<&'static str>,
    updated_at: String,
    ended_at: Option<String>,
    href: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AuctionRecruitmentListData {
    recruitments: Vec<AuctionRecruitmentData>,
    next_cursor: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AuctionRecruitmentData {
    id: String,
    display_number: i64,
    title: String,
    format: &'static str,
    participant_count: i16,
    joined_participant_count: i64,
    team_count: i16,
    status: &'static str,
    revision: i64,
    updated_at: String,
    href: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AuctionTournamentListData {
    tournaments: Vec<AuctionTournamentData>,
    next_cursor: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AuctionTournamentData {
    id: String,
    display_number: i64,
    title: String,
    format: &'static str,
    participant_count: i16,
    team_count: i16,
    status: &'static str,
    revision: i64,
    active_member_count: i64,
    bid_member_count: i64,
    incomplete_match_count: i64,
    next_match: Option<AuctionMatchSummaryData>,
    champion_team: Option<AuctionChampionTeamData>,
    updated_at: String,
    ended_at: Option<String>,
    href: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AuctionMatchSummaryData {
    id: String,
    label: &'static str,
    match_key: String,
    status: String,
    confirmed_game_count: i64,
}

#[derive(Serialize)]
struct AuctionChampionTeamData {
    id: Uuid,
    name: String,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ActionableState {
    Actionable,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum RequestedListState {
    Active,
    Completed,
}

impl From<RequestedListState> for ListState {
    fn from(value: RequestedListState) -> Self {
        match value {
            RequestedListState::Active => Self::Active,
            RequestedListState::Completed => Self::Completed,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionableListQuery {
    state: ActionableState,
    cursor: Option<String>,
    limit: Option<u16>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateListQuery {
    state: RequestedListState,
    cursor: Option<String>,
    limit: Option<u16>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CursorToken {
    updated_at_nanos: i64,
    id: Uuid,
}

pub async fn operations(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(guild_id): Path<GuildId>,
    principal: AuthenticatedSession,
) -> Result<Response, ApiError> {
    let pool = database(&state, &request_id)?;
    require_guild_access(pool, &principal, &guild_id, &request_id).await?;
    let records = workflow::list_operations(pool, &principal.session.user_id, &guild_id)
        .await
        .map_err(|error| repository_error(error, &request_id))?;
    let operations = records
        .into_iter()
        .map(|record| map_operation(record, &request_id))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(error::success(
        StatusCode::OK,
        &request_id,
        OperationsData { operations },
    ))
}

pub async fn recruitments(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(guild_id): Path<GuildId>,
    Query(query): Query<ActionableListQuery>,
    principal: AuthenticatedSession,
) -> Result<Response, ApiError> {
    let _state = query.state;
    let (cursor, limit) = page_parameters(query.cursor.as_deref(), query.limit, &request_id)?;
    let pool = database(&state, &request_id)?;
    require_guild_access(pool, &principal, &guild_id, &request_id).await?;
    let page = workflow::list_actionable_recruitments(
        pool,
        &principal.session.user_id,
        &guild_id,
        cursor,
        limit,
    )
    .await
    .map_err(|error| repository_error(error, &request_id))?;
    let data = map_recruitment_page(page, &guild_id, &request_id)?;

    Ok(error::success(StatusCode::OK, &request_id, data))
}

pub async fn series(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(guild_id): Path<GuildId>,
    Query(query): Query<StateListQuery>,
    principal: AuthenticatedSession,
) -> Result<Response, ApiError> {
    let (cursor, limit) = page_parameters(query.cursor.as_deref(), query.limit, &request_id)?;
    let pool = database(&state, &request_id)?;
    require_guild_access(pool, &principal, &guild_id, &request_id).await?;
    let page = workflow::list_series(
        pool,
        &principal.session.user_id,
        &guild_id,
        query.state.into(),
        cursor,
        limit,
    )
    .await
    .map_err(|error| repository_error(error, &request_id))?;
    let data = map_series_page(page, &guild_id, &request_id)?;

    Ok(error::success(StatusCode::OK, &request_id, data))
}

pub async fn auction_recruitments(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(guild_id): Path<GuildId>,
    Query(query): Query<ActionableListQuery>,
    principal: AuthenticatedSession,
) -> Result<Response, ApiError> {
    let _state = query.state;
    let (cursor, limit) = page_parameters(query.cursor.as_deref(), query.limit, &request_id)?;
    let pool = database(&state, &request_id)?;
    require_guild_access(pool, &principal, &guild_id, &request_id).await?;
    let page = workflow::list_actionable_auction_recruitments(
        pool,
        &principal.session.user_id,
        &guild_id,
        cursor,
        limit,
    )
    .await
    .map_err(|error| repository_error(error, &request_id))?;
    let data = map_auction_recruitment_page(page, &guild_id, &request_id)?;

    Ok(error::success(StatusCode::OK, &request_id, data))
}

pub async fn auction_tournaments(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(guild_id): Path<GuildId>,
    Query(query): Query<StateListQuery>,
    principal: AuthenticatedSession,
) -> Result<Response, ApiError> {
    let (cursor, limit) = page_parameters(query.cursor.as_deref(), query.limit, &request_id)?;
    let pool = database(&state, &request_id)?;
    require_guild_access(pool, &principal, &guild_id, &request_id).await?;
    let page = workflow::list_auction_tournaments(
        pool,
        &principal.session.user_id,
        &guild_id,
        query.state.into(),
        cursor,
        limit,
    )
    .await
    .map_err(|error| repository_error(error, &request_id))?;
    let data = map_auction_tournament_page(page, &guild_id, &request_id)?;

    Ok(error::success(StatusCode::OK, &request_id, data))
}

async fn require_guild_access(
    pool: &PgPool,
    principal: &AuthenticatedSession,
    guild_id: &GuildId,
    request_id: &RequestId,
) -> Result<(), ApiError> {
    let accessible = workflow::has_guild_access(pool, &principal.session.user_id, guild_id)
        .await
        .map_err(|error| repository_error(error, request_id))?;
    if accessible {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::NOT_FOUND,
            request_id,
            "not_found",
            "요청한 길드를 찾을 수 없습니다.",
            None,
        ))
    }
}

fn map_operation(
    record: OperationRecord,
    request_id: &RequestId,
) -> Result<OperationData, ApiError> {
    Ok(OperationData {
        id: record.id,
        kind: record.kind.as_str(),
        title: record.title,
        status: record.status,
        detail: record.detail,
        updated_at: format_timestamp(record.updated_at, request_id)?,
        href: record.href,
        priority: record.priority.as_str(),
    })
}

fn map_recruitment_page(
    page: Page<RecruitmentRecord>,
    guild_id: &GuildId,
    request_id: &RequestId,
) -> Result<RecruitmentListData, ApiError> {
    let recruitments = page
        .items
        .into_iter()
        .map(|record| {
            let id = record.id.to_string();
            Ok(RecruitmentData {
                href: format!("/g/{}/recruitments/{id}/entry", guild_id.as_str()),
                id,
                display_number: record.display_number,
                title: record.title,
                status: "CLOSED",
                target_count: record.target_count,
                joined_count: record.participant_count,
                assigned_count: record.assigned_count,
                revision: record.revision,
                updated_at: format_timestamp(record.updated_at, request_id)?,
            })
        })
        .collect::<Result<_, ApiError>>()?;
    Ok(RecruitmentListData {
        recruitments,
        next_cursor: encode_optional_cursor(page.next_cursor, request_id)?,
    })
}

fn map_series_page(
    page: Page<SeriesRecord>,
    guild_id: &GuildId,
    request_id: &RequestId,
) -> Result<SeriesListData, ApiError> {
    let series = page
        .items
        .into_iter()
        .map(|record| {
            let id = record.id.to_string();
            let suffix = if record.status.as_str() == "IN_PROGRESS" {
                "pick-ban"
            } else {
                "result"
            };
            Ok(SeriesData {
                href: format!("/g/{}/series/{id}/{suffix}", guild_id.as_str()),
                id,
                display_number: record.display_number,
                title: record.title,
                best_of: record.best_of,
                status: record.status.as_str(),
                team_1_score: record.team_1_score,
                team_2_score: record.team_2_score,
                winning_team: record.winner,
                terminal_reason: record.terminal_reason,
                current_set_number: record.current_set_number,
                outcome: record.outcome.map(|outcome| outcome.as_str()),
                updated_at: format_timestamp(record.updated_at, request_id)?,
                ended_at: record
                    .played_at
                    .map(|timestamp| format_timestamp(timestamp, request_id))
                    .transpose()?,
            })
        })
        .collect::<Result<_, ApiError>>()?;
    Ok(SeriesListData {
        series,
        next_cursor: encode_optional_cursor(page.next_cursor, request_id)?,
    })
}

fn map_auction_recruitment_page(
    page: Page<AuctionRecruitmentRecord>,
    guild_id: &GuildId,
    request_id: &RequestId,
) -> Result<AuctionRecruitmentListData, ApiError> {
    let recruitments = page
        .items
        .into_iter()
        .map(|record| {
            let id = record.id.to_string();
            Ok(AuctionRecruitmentData {
                href: format!("/g/{}/auction/recruitments/{id}/draft", guild_id.as_str()),
                id,
                display_number: record.display_number,
                title: record.title,
                format: record.format.as_str(),
                participant_count: record.target_count,
                joined_participant_count: record.participant_count,
                team_count: record.team_count,
                status: "CLOSED",
                revision: record.revision,
                updated_at: format_timestamp(record.updated_at, request_id)?,
            })
        })
        .collect::<Result<_, ApiError>>()?;
    Ok(AuctionRecruitmentListData {
        recruitments,
        next_cursor: encode_optional_cursor(page.next_cursor, request_id)?,
    })
}

fn map_auction_tournament_page(
    page: Page<AuctionTournamentRecord>,
    guild_id: &GuildId,
    request_id: &RequestId,
) -> Result<AuctionTournamentListData, ApiError> {
    let tournaments = page
        .items
        .into_iter()
        .map(|record| map_auction_tournament(record, guild_id, request_id))
        .collect::<Result<_, _>>()?;
    Ok(AuctionTournamentListData {
        tournaments,
        next_cursor: encode_optional_cursor(page.next_cursor, request_id)?,
    })
}

fn map_auction_tournament(
    record: AuctionTournamentRecord,
    guild_id: &GuildId,
    request_id: &RequestId,
) -> Result<AuctionTournamentData, ApiError> {
    let id = record.id.to_string();
    let href_suffix = match record.status {
        AuctionTournamentStatus::Bidding => "draft",
        AuctionTournamentStatus::BracketSetup => "bracket",
        AuctionTournamentStatus::InGame => {
            if record
                .next_match
                .as_ref()
                .is_some_and(|next| matches!(next.status.as_str(), "READY" | "IN_PROGRESS"))
            {
                "bracket?view=result"
            } else {
                "bracket"
            }
        }
        AuctionTournamentStatus::Completed | AuctionTournamentStatus::Cancelled => "result",
    };
    let next_match = record
        .next_match
        .map(map_auction_match)
        .transpose()
        .map_err(|message| invalid_projection(message, request_id))?;
    let champion_team = match (record.champion_team_id, record.champion_team_name) {
        (Some(id), Some(name)) => Some(AuctionChampionTeamData { id, name }),
        (None, None) => None,
        _ => {
            return Err(invalid_projection(
                "auction champion is incomplete",
                request_id,
            ));
        }
    };

    Ok(AuctionTournamentData {
        href: format!(
            "/g/{}/auction/tournaments/{id}/{href_suffix}",
            guild_id.as_str()
        ),
        id,
        display_number: record.display_number,
        title: record.title,
        format: record.format.as_str(),
        participant_count: record.participant_count,
        team_count: record.team_count,
        status: record.status.as_str(),
        revision: record.revision,
        active_member_count: record.active_member_count,
        bid_member_count: record.bid_member_count,
        incomplete_match_count: record.incomplete_match_count,
        next_match,
        champion_team,
        updated_at: format_timestamp(record.updated_at, request_id)?,
        ended_at: record
            .ended_at
            .map(|timestamp| format_timestamp(timestamp, request_id))
            .transpose()?,
    })
}

fn map_auction_match(
    record: AuctionMatchSummaryRecord,
) -> Result<AuctionMatchSummaryData, &'static str> {
    let label = match record.match_key.as_str() {
        "SEMIFINAL_A" => "준결승 A",
        "SEMIFINAL_B" => "준결승 B",
        "FINAL" => "결승",
        _ => return Err("auction match key is not canonical"),
    };
    if !matches!(
        record.status.as_str(),
        "LINEUP_PENDING" | "READY" | "IN_PROGRESS"
    ) {
        return Err("auction next match status is not actionable");
    }
    Ok(AuctionMatchSummaryData {
        id: record.id.to_string(),
        label,
        match_key: record.match_key,
        status: record.status,
        confirmed_game_count: record.confirmed_game_count,
    })
}

fn page_parameters(
    cursor: Option<&str>,
    limit: Option<u16>,
    request_id: &RequestId,
) -> Result<(Option<PageCursor>, i64), ApiError> {
    let limit = limit.unwrap_or(DEFAULT_PAGE_SIZE);
    if !(1..=MAX_PAGE_SIZE).contains(&limit) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            request_id,
            "invalid_limit",
            "limit은 1 이상 100 이하이어야 합니다.",
            None,
        ));
    }
    let cursor = cursor.map(decode_cursor).transpose().map_err(|()| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            request_id,
            "invalid_cursor",
            "cursor 형식이 올바르지 않습니다.",
            None,
        )
    })?;
    Ok((cursor, i64::from(limit)))
}

fn encode_optional_cursor(
    cursor: Option<PageCursor>,
    request_id: &RequestId,
) -> Result<Option<String>, ApiError> {
    cursor
        .map(encode_cursor)
        .transpose()
        .map_err(|()| invalid_projection("page cursor is outside the supported range", request_id))
}

fn encode_cursor(cursor: PageCursor) -> Result<String, ()> {
    let updated_at_nanos =
        i64::try_from(cursor.updated_at.unix_timestamp_nanos()).map_err(|_| ())?;
    let payload = serde_json::to_vec(&CursorToken {
        updated_at_nanos,
        id: cursor.id,
    })
    .map_err(|_| ())?;
    Ok(URL_SAFE_NO_PAD.encode(payload))
}

fn decode_cursor(value: &str) -> Result<PageCursor, ()> {
    let bytes = URL_SAFE_NO_PAD.decode(value).map_err(|_| ())?;
    let token = serde_json::from_slice::<CursorToken>(&bytes).map_err(|_| ())?;
    let updated_at = OffsetDateTime::from_unix_timestamp_nanos(i128::from(token.updated_at_nanos))
        .map_err(|_| ())?;
    Ok(PageCursor {
        updated_at,
        id: token.id,
    })
}

fn format_timestamp(value: OffsetDateTime, request_id: &RequestId) -> Result<String, ApiError> {
    value.format(&Rfc3339).map_err(|error| {
        tracing::error!(%error, "failed to format workflow timestamp");
        invalid_projection("workflow timestamp cannot be formatted", request_id)
    })
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

fn repository_error(error: WorkflowRepositoryError, request_id: &RequestId) -> ApiError {
    tracing::error!(%error, "workflow read repository operation failed");
    match error {
        WorkflowRepositoryError::Sql(source) => error::database_failure(&source, request_id),
        WorkflowRepositoryError::InvalidProjection(message) => {
            invalid_projection(message, request_id)
        }
    }
}

fn invalid_projection(message: &'static str, request_id: &RequestId) -> ApiError {
    tracing::error!(message, "workflow read projection failed");
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
    fn opaque_cursor_roundtrips_without_losing_sort_precision() {
        let cursor = PageCursor {
            updated_at: OffsetDateTime::from_unix_timestamp_nanos(1_723_456_789_123_456_789)
                .expect("valid timestamp"),
            id: Uuid::now_v7(),
        };
        let encoded = encode_cursor(cursor).expect("cursor should encode");
        let decoded = decode_cursor(&encoded).expect("cursor should decode");

        assert_eq!(decoded, cursor);
        assert!(!encoded.contains('='));
    }

    #[test]
    fn cursor_decoder_rejects_non_canonical_payloads() {
        assert!(decode_cursor("not-base64!").is_err());
        let unknown_field = URL_SAFE_NO_PAD.encode(
            br#"{"updatedAtNanos":1,"id":"01900000-0000-7000-8000-000000000001","extra":true}"#,
        );
        assert!(decode_cursor(&unknown_field).is_err());
    }
}
