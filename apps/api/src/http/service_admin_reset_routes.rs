use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::get,
};
use serde::Deserialize;
use uuid::Uuid;

use super::extractors::{AuthenticatedSession, require_csrf};
use crate::{
    domain::GuildId,
    error::{self, ApiError},
    repository::service_admin_reset::{self, ServiceAdminResetRepositoryError},
    state::{AppState, RequestId},
};
use axum::Extension;

const IDEMPOTENCY_KEY: &str = "idempotency-key";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetCommand {
    confirmation: String,
}

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/admin/guilds/{guild_id}/record-reset",
        get(preview).post(reset),
    )
}

pub async fn preview(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    principal: AuthenticatedSession,
    Path(guild_id): Path<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &principal, &request_id)?;
    let guild_id = parse_guild_id(guild_id, &request_id)?;
    let preview = service_admin_reset::preview(database(&state, &request_id)?, &guild_id)
        .await
        .map_err(|source| repository_error(source, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, preview))
}

pub async fn reset(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    principal: AuthenticatedSession,
    Path(guild_id): Path<String>,
    headers: HeaderMap,
    Json(command): Json<ResetCommand>,
) -> Result<Response, ApiError> {
    authorize(&state, &principal, &request_id)?;
    require_csrf(&headers, &state, &principal, &request_id)?;
    let guild_id = parse_guild_id(guild_id, &request_id)?;
    let expected_confirmation = format!("RESET {}", guild_id.as_str());
    if command.confirmation != expected_confirmation {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            &request_id,
            "reset_confirmation_mismatch",
            "확인 문구가 대상 서버와 일치하지 않습니다.",
            None,
        ));
    }
    let idempotency_key = idempotency_key(&headers, &request_id)?;
    let result = service_admin_reset::reset(
        database(&state, &request_id)?,
        &guild_id,
        &principal.session.user_id,
        &idempotency_key,
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    tracing::warn!(
        guild_id = %guild_id,
        actor_id = %principal.session.user_id,
        deleted_rows = result.deleted.total_rows,
        request_id = %request_id.0,
        "service admin reset all test guild records"
    );
    Ok(error::success(StatusCode::OK, &request_id, result))
}

fn authorize(
    state: &AppState,
    principal: &AuthenticatedSession,
    request_id: &RequestId,
) -> Result<(), ApiError> {
    let is_service_admin = state
        .config()
        .auth()
        .is_some_and(|auth| auth.is_service_admin(&principal.session.discord_id));
    if !is_service_admin {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "service_admin_required",
            "서비스 관리자 권한이 필요합니다.",
            None,
        ));
    }
    if !state.config().dummy_users_enabled() {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "test_record_reset_disabled",
            "테스트 기록 초기화 기능이 비활성화되어 있습니다.",
            None,
        ));
    }
    Ok(())
}

fn parse_guild_id(value: String, request_id: &RequestId) -> Result<GuildId, ApiError> {
    GuildId::parse(value).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            request_id,
            "invalid_guild_id",
            "Discord 서버 ID 형식이 올바르지 않습니다.",
            None,
        )
    })
}

fn idempotency_key(headers: &HeaderMap, request_id: &RequestId) -> Result<String, ApiError> {
    let mut values = headers.get_all(IDEMPOTENCY_KEY).iter();
    let value = values
        .next()
        .and_then(|value| value.to_str().ok())
        .filter(|_| values.next().is_none())
        .filter(|value| Uuid::parse_str(value).is_ok())
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                request_id,
                "invalid_idempotency_key",
                "Idempotency-Key가 올바르지 않습니다.",
                None,
            )
        })?;
    Ok(value.to_owned())
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

fn repository_error(source: ServiceAdminResetRepositoryError, request_id: &RequestId) -> ApiError {
    match source {
        ServiceAdminResetRepositoryError::Database(ref database_error) => {
            error::database_failure(database_error, request_id)
        }
        ServiceAdminResetRepositoryError::GuildNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_id,
            "guild_not_found",
            "초기화할 서버를 찾을 수 없습니다.",
            None,
        ),
        ServiceAdminResetRepositoryError::GuildInactive => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "guild_not_active",
            "활성 상태인 서버만 테스트 기록을 초기화할 수 있습니다.",
            None,
        ),
        ServiceAdminResetRepositoryError::ActiveSeasonMissing => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "active_season_missing",
            "활성 시즌이 없어 테스트 기록을 초기화할 수 없습니다.",
            None,
        ),
        ServiceAdminResetRepositoryError::IdempotencyConflict => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_conflict",
            "같은 Idempotency-Key가 다른 초기화 요청에 사용되었습니다.",
            None,
        ),
        ServiceAdminResetRepositoryError::IdempotencyInProgress => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_in_progress",
            "같은 초기화 요청이 이미 처리 중입니다.",
            None,
        ),
        internal => {
            tracing::error!(%internal, "service-admin record reset failed");
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                request_id,
                "record_reset_internal_error",
                "테스트 기록 초기화 상태를 저장하는 중 내부 오류가 발생했습니다.",
                None,
            )
        }
    }
}
