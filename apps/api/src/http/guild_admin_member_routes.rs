use axum::{
    Extension, Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::Response,
};
use time::format_description::well_known::Rfc3339;

use crate::{
    domain::{GuildId, UserId},
    error::{self, ApiError},
    http::extractors::{AuthenticatedSession, require_csrf},
    repository::guild_admin_members::{
        self, AdjustMmrCommand, GuildAdminMembersRepositoryError, SetMemberStatusCommand,
    },
    state::{AppState, RequestId},
};

const IDEMPOTENCY_KEY: &str = "idempotency-key";
const MAX_IDEMPOTENCY_KEY_BYTES: usize = 128;

pub async fn members(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(guild_id): Path<GuildId>,
    principal: AuthenticatedSession,
) -> Result<Response, ApiError> {
    let snapshot = guild_admin_members::get(
        database(&state, &request_id)?,
        &guild_id,
        &principal.session.user_id,
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, snapshot))
}

pub async fn adjust_mmr(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, member_id)): Path<(GuildId, UserId)>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(command): Json<AdjustMmrCommand>,
) -> Result<Response, ApiError> {
    require_csrf(&headers, &state, &principal, &request_id)?;
    let idempotency_key = idempotency_key(&headers, &request_id)?;
    let snapshot = guild_admin_members::adjust_mmr(
        database(&state, &request_id)?,
        &guild_id,
        &member_id,
        &principal.session.user_id,
        &idempotency_key,
        &request_id.0,
        command,
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, snapshot))
}

pub async fn set_status(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, member_id)): Path<(GuildId, UserId)>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(command): Json<SetMemberStatusCommand>,
) -> Result<Response, ApiError> {
    require_csrf(&headers, &state, &principal, &request_id)?;
    let snapshot = guild_admin_members::set_status(
        database(&state, &request_id)?,
        &guild_id,
        &member_id,
        &principal.session.user_id,
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

fn repository_error(source: GuildAdminMembersRepositoryError, request_id: &RequestId) -> ApiError {
    match source {
        GuildAdminMembersRepositoryError::GuildNotFound
        | GuildAdminMembersRepositoryError::MemberNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_id,
            "not_found",
            "요청한 길드 멤버를 찾을 수 없습니다.",
            None,
        ),
        GuildAdminMembersRepositoryError::OperatorRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "operator_required",
            "길드 운영자 권한이 필요합니다.",
            None,
        ),
        GuildAdminMembersRepositoryError::SelfDeactivationForbidden => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "self_deactivation_forbidden",
            "자기 자신은 비활성화할 수 없습니다.",
            None,
        ),
        GuildAdminMembersRepositoryError::NoActiveSeason => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "guild_setup_required",
            "길드의 활성 시즌을 먼저 설정해야 합니다.",
            None,
        ),
        GuildAdminMembersRepositoryError::RevisionConflict {
            expected,
            current,
            updated_at,
            updated_by,
        } => {
            let updated_at = updated_at.format(&Rfc3339).unwrap_or_else(|format_error| {
                tracing::error!(%format_error, "failed to format member revision timestamp");
                String::new()
            });
            ApiError::new(
                StatusCode::CONFLICT,
                request_id,
                "revision_conflict",
                "최신 상태를 다시 확인해야 합니다.",
                Some(serde_json::json!({
                    "expectedRevision": expected,
                    "currentRevision": current,
                    "updatedAt": updated_at,
                    "updatedBy": updated_by,
                })),
            )
        }
        GuildAdminMembersRepositoryError::InvalidAdjustment => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "invalid_mmr_adjustment",
            "MMR 증감값과 조정 사유를 확인해 주세요.",
            Some(serde_json::json!({ "noteMaxLength": 500 })),
        ),
        GuildAdminMembersRepositoryError::InvalidExpectedRevision => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "invalid_expected_revision",
            "expectedRevision은 0 이상이어야 합니다.",
            None,
        ),
        GuildAdminMembersRepositoryError::IdempotencyConflict => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_conflict",
            "같은 Idempotency-Key가 다른 요청에 사용되었습니다.",
            None,
        ),
        GuildAdminMembersRepositoryError::IdempotencyInProgress => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_in_progress",
            "같은 요청을 처리하고 있습니다.",
            None,
        ),
        GuildAdminMembersRepositoryError::DatabaseBusy => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            request_id,
            "database_busy",
            "데이터베이스가 사용 중입니다. 잠시 후 다시 시도해 주세요.",
            None,
        ),
        GuildAdminMembersRepositoryError::Database(database_error) => {
            tracing::error!(%database_error, "guild admin member database operation failed");
            error::database_failure(&database_error, request_id)
        }
        GuildAdminMembersRepositoryError::Serialization(serialization_error) => {
            tracing::error!(%serialization_error, "guild admin member serialization failed");
            internal(request_id)
        }
        GuildAdminMembersRepositoryError::InvariantViolation => {
            tracing::error!("guild admin member stored invariant failed");
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
    fn browser_idempotency_keys_accept_uuid_and_reject_spaces_or_duplicates() {
        let request_id = RequestId("request-id".to_owned());
        let mut headers = HeaderMap::new();
        headers.insert(
            IDEMPOTENCY_KEY,
            HeaderValue::from_static("01900000-0000-7000-8000-000000000001"),
        );
        assert_eq!(
            idempotency_key(&headers, &request_id).expect("valid idempotency key"),
            "01900000-0000-7000-8000-000000000001"
        );

        headers.insert(IDEMPOTENCY_KEY, HeaderValue::from_static("contains space"));
        assert!(idempotency_key(&headers, &request_id).is_err());

        headers.append(IDEMPOTENCY_KEY, HeaderValue::from_static("second-key"));
        assert!(idempotency_key(&headers, &request_id).is_err());
    }
}
