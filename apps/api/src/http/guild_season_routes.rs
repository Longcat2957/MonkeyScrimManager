use axum::{
    Extension, Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::Response,
};

use crate::{
    domain::GuildId,
    error::{self, ApiError},
    http::extractors::{AuthenticatedSession, require_csrf},
    repository::guild_seasons::{self, GuildSeasonsRepositoryError, RotateSeasonCommand},
    state::{AppState, RequestId},
};

const IDEMPOTENCY_KEY: &str = "idempotency-key";
const MAX_IDEMPOTENCY_KEY_BYTES: usize = 128;

pub async fn seasons(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(guild_id): Path<GuildId>,
    principal: AuthenticatedSession,
) -> Result<Response, ApiError> {
    let snapshot = guild_seasons::get(
        database(&state, &request_id)?,
        &guild_id,
        &principal.session.user_id,
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, snapshot))
}

pub async fn rotate_season(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(guild_id): Path<GuildId>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(command): Json<RotateSeasonCommand>,
) -> Result<Response, ApiError> {
    require_csrf(&headers, &state, &principal, &request_id)?;
    let idempotency_key = idempotency_key(&headers, &request_id)?;
    let snapshot = guild_seasons::rotate(
        database(&state, &request_id)?,
        &guild_id,
        &principal.session.user_id,
        &idempotency_key,
        &request_id.0,
        command,
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, snapshot))
}

fn idempotency_key(headers: &HeaderMap, request_id: &RequestId) -> Result<String, ApiError> {
    let mut values = headers.get_all(IDEMPOTENCY_KEY).iter();
    let value = values
        .next()
        .and_then(|value| value.to_str().ok())
        .filter(|value| {
            values.next().is_none()
                && !value.is_empty()
                && value.len() <= MAX_IDEMPOTENCY_KEY_BYTES
                && value.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
        })
        .ok_or_else(|| invalid_idempotency_key(request_id))?;
    Ok(value.to_owned())
}

fn invalid_idempotency_key(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        request_id,
        "invalid_idempotency_key",
        "Idempotency-Key가 올바르지 않습니다.",
        Some(serde_json::json!({ "maxLength": MAX_IDEMPOTENCY_KEY_BYTES })),
    )
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

fn repository_error(source: GuildSeasonsRepositoryError, request_id: &RequestId) -> ApiError {
    match source {
        GuildSeasonsRepositoryError::GuildNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_id,
            "not_found",
            "요청한 길드를 찾을 수 없습니다.",
            None,
        ),
        GuildSeasonsRepositoryError::OperatorRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "operator_required",
            "길드 운영자 권한이 필요합니다.",
            None,
        ),
        GuildSeasonsRepositoryError::NoActiveSeason => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "no_active_season",
            "길드에 진행 중인 시즌이 없습니다.",
            None,
        ),
        GuildSeasonsRepositoryError::InvalidSeasonName => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "invalid_season_name",
            "시즌 이름을 확인해 주세요.",
            Some(serde_json::json!({
                "field": "name",
                "maxLength": 100,
            })),
        ),
        GuildSeasonsRepositoryError::CurrentSeasonConflict { expected, current } => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "current_season_conflict",
            "현재 시즌이 변경되었습니다. 최신 상태를 다시 확인해 주세요.",
            Some(serde_json::json!({
                "expectedCurrentSeasonId": expected,
                "currentSeasonId": current,
            })),
        ),
        GuildSeasonsRepositoryError::IdempotencyConflict => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_conflict",
            "같은 Idempotency-Key가 다른 요청에 사용되었습니다.",
            None,
        ),
        GuildSeasonsRepositoryError::IdempotencyInProgress => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_in_progress",
            "같은 요청을 처리하고 있습니다.",
            None,
        ),
        GuildSeasonsRepositoryError::DatabaseBusy => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            request_id,
            "database_busy",
            "데이터베이스가 사용 중입니다. 잠시 후 다시 시도해 주세요.",
            None,
        ),
        GuildSeasonsRepositoryError::Database(database_error) => {
            tracing::error!(%database_error, "guild season database operation failed");
            error::database_failure(&database_error, request_id)
        }
        GuildSeasonsRepositoryError::Serialization(serialization_error) => {
            tracing::error!(%serialization_error, "guild season response serialization failed");
            internal(request_id)
        }
        GuildSeasonsRepositoryError::InvariantViolation => {
            tracing::error!("guild season stored invariant failed");
            internal(request_id)
        }
    }
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
    use axum::http::{HeaderMap, HeaderValue};

    use super::*;

    #[test]
    fn season_idempotency_keys_require_one_visible_ascii_value() {
        let request_id = RequestId("request-id".to_owned());
        let mut headers = HeaderMap::new();
        headers.insert(
            IDEMPOTENCY_KEY,
            HeaderValue::from_static("season-rotation-1"),
        );
        assert_eq!(
            idempotency_key(&headers, &request_id).expect("valid key"),
            "season-rotation-1"
        );

        headers.insert(IDEMPOTENCY_KEY, HeaderValue::from_static("contains space"));
        assert!(idempotency_key(&headers, &request_id).is_err());

        headers.clear();
        headers.insert(
            IDEMPOTENCY_KEY,
            HeaderValue::from_str(&"x".repeat(128)).expect("boundary key"),
        );
        assert!(idempotency_key(&headers, &request_id).is_ok());
        headers.insert(
            IDEMPOTENCY_KEY,
            HeaderValue::from_str(&"x".repeat(129)).expect("oversized key"),
        );
        assert!(idempotency_key(&headers, &request_id).is_err());

        headers.clear();
        headers.append(IDEMPOTENCY_KEY, HeaderValue::from_static("first"));
        headers.append(IDEMPOTENCY_KEY, HeaderValue::from_static("second"));
        assert!(idempotency_key(&headers, &request_id).is_err());
    }
}
