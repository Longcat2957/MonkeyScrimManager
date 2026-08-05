use axum::{
    Extension,
    extract::{Path, Query, State},
    http::StatusCode,
    response::Response,
};
use serde::Deserialize;

use crate::{
    domain::{GuildId, SeasonId},
    error::{self, ApiError},
    http::extractors::AuthenticatedSession,
    repository::guild_statistics::{self, GuildStatisticsRepositoryError, GuildStatisticsScope},
    state::{AppState, RequestId},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct GuildStatisticsQuery {
    season_id: Option<String>,
    scope: Option<String>,
}

pub async fn statistics(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(guild_id): Path<GuildId>,
    Query(query): Query<GuildStatisticsQuery>,
    principal: AuthenticatedSession,
) -> Result<Response, ApiError> {
    let season_id = query
        .season_id
        .as_deref()
        .map(str::parse::<SeasonId>)
        .transpose()
        .map_err(|_| invalid_season_id(&request_id))?;
    let scope = match query.scope.as_deref() {
        Some(value) => {
            GuildStatisticsScope::parse(value).ok_or_else(|| invalid_scope(&request_id))?
        }
        None => GuildStatisticsScope::All,
    };
    let snapshot = guild_statistics::get(
        database(&state, &request_id)?,
        &guild_id,
        &principal.session.user_id,
        season_id,
        scope,
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, snapshot))
}

fn database<'a>(state: &'a AppState, request_id: &RequestId) -> Result<&'a sqlx::PgPool, ApiError> {
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

fn repository_error(source: GuildStatisticsRepositoryError, request_id: &RequestId) -> ApiError {
    match source {
        GuildStatisticsRepositoryError::GuildNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_id,
            "not_found",
            "요청한 길드를 찾을 수 없습니다.",
            None,
        ),
        GuildStatisticsRepositoryError::SeasonNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_id,
            "season_not_found",
            "요청한 시즌을 찾을 수 없습니다.",
            None,
        ),
        GuildStatisticsRepositoryError::StoredInvariantViolation => {
            tracing::error!("guild statistics stored invariant failed");
            internal(request_id)
        }
        GuildStatisticsRepositoryError::Database(database_error) => {
            tracing::error!(%database_error, "guild statistics database operation failed");
            error::database_failure(&database_error, request_id)
        }
    }
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

fn invalid_scope(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        request_id,
        "invalid_scope",
        "scope은 ALL, REGULAR, AUCTION 중 하나여야 합니다.",
        Some(serde_json::json!({
            "allowed": ["ALL", "REGULAR", "AUCTION"],
        })),
    )
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
