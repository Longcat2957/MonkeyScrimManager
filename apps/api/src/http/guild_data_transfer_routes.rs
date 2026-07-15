use axum::{
    Extension, Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::Response,
};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    domain::GuildId,
    error::{self, ApiError},
    http::extractors::{AuthenticatedSession, require_csrf},
    repository::guild_data_transfer::{self, ApplyImportCommand, GuildDataTransferRepositoryError},
    state::{AppState, RequestId},
};

const IDEMPOTENCY_KEY: &str = "idempotency-key";
const MAX_IDEMPOTENCY_KEY_BYTES: usize = 128;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreviewImportRequest {
    archive: Value,
}

pub async fn export(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(guild_id): Path<GuildId>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
) -> Result<Response, ApiError> {
    require_csrf(&headers, &state, &principal, &request_id)?;
    let result = guild_data_transfer::export(
        database(&state, &request_id)?,
        &guild_id,
        &principal.session.user_id,
        &request_id.0,
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, result))
}

pub async fn preview(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(guild_id): Path<GuildId>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(request): Json<PreviewImportRequest>,
) -> Result<Response, ApiError> {
    require_csrf(&headers, &state, &principal, &request_id)?;
    let result = guild_data_transfer::preview(
        database(&state, &request_id)?,
        &guild_id,
        &principal.session.user_id,
        request.archive,
        &request_id.0,
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, result))
}

pub async fn apply(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, import_id)): Path<(GuildId, uuid::Uuid)>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(command): Json<ApplyImportCommand>,
) -> Result<Response, ApiError> {
    require_csrf(&headers, &state, &principal, &request_id)?;
    let idempotency_key = idempotency_key(&headers, &request_id)?;
    let result = guild_data_transfer::apply(
        database(&state, &request_id)?,
        &guild_id,
        &principal.session.user_id,
        import_id,
        &idempotency_key,
        &request_id.0,
        command,
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, result))
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
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                request_id,
                "invalid_idempotency_key",
                "Idempotency-Key가 올바르지 않습니다.",
                Some(serde_json::json!({ "maxLength": MAX_IDEMPOTENCY_KEY_BYTES })),
            )
        })?;
    Ok(value.to_owned())
}

fn repository_error(source: GuildDataTransferRepositoryError, request_id: &RequestId) -> ApiError {
    match source {
        GuildDataTransferRepositoryError::GuildNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_id,
            "not_found",
            "요청한 길드를 찾을 수 없습니다.",
            None,
        ),
        GuildDataTransferRepositoryError::OperatorRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "operator_required",
            "길드 운영자 권한이 필요합니다.",
            None,
        ),
        GuildDataTransferRepositoryError::NoActiveSeason => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "no_active_season",
            "길드의 활성 시즌을 먼저 설정해야 합니다.",
            None,
        ),
        GuildDataTransferRepositoryError::InvalidArchive => ApiError::new(
            StatusCode::BAD_REQUEST,
            request_id,
            "invalid_archive",
            "지원되는 MonkeyBot MMR 백업 파일 형식이 아닙니다.",
            Some(serde_json::json!({ "maxMembers": 1000 })),
        ),
        GuildDataTransferRepositoryError::ArchiveMemberLimitExceeded => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "archive_member_limit_exceeded",
            "활성 Discord 멤버가 1,000명을 초과하여 MMR 백업을 만들 수 없습니다.",
            Some(serde_json::json!({ "maxMembers": 1000 })),
        ),
        GuildDataTransferRepositoryError::InvalidApplyCommand => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "invalid_import_apply_command",
            "가져오기 적용 확인 값이 올바르지 않습니다.",
            None,
        ),
        GuildDataTransferRepositoryError::PreviewNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_id,
            "preview_not_found",
            "가져오기 검토 결과를 찾을 수 없습니다. 파일을 다시 검증해 주세요.",
            None,
        ),
        GuildDataTransferRepositoryError::PreviewActorMismatch => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "preview_actor_mismatch",
            "가져오기 검토를 만든 운영자만 적용할 수 있습니다.",
            None,
        ),
        GuildDataTransferRepositoryError::PreviewExpired => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "preview_expired",
            "가져오기 검토 결과가 만료되었습니다. 파일을 다시 검증해 주세요.",
            None,
        ),
        GuildDataTransferRepositoryError::PreviewAlreadyApplied => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "preview_already_applied",
            "이 가져오기 검토 결과는 이미 적용되었습니다.",
            None,
        ),
        GuildDataTransferRepositoryError::PreviewDigestMismatch => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "preview_digest_mismatch",
            "검토한 백업 파일과 적용 요청이 일치하지 않습니다.",
            None,
        ),
        GuildDataTransferRepositoryError::PreviewConflicts => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "preview_conflicts",
            "대상 길드 멤버 상태가 검토 결과와 달라져 가져올 수 없습니다.",
            None,
        ),
        GuildDataTransferRepositoryError::RevisionConflict { expected, current } => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "revision_conflict",
            "길드 멤버 MMR 상태가 변경되었습니다. 파일을 다시 검증해 주세요.",
            Some(serde_json::json!({
                "expectedRevision": expected,
                "currentRevision": current,
            })),
        ),
        GuildDataTransferRepositoryError::TargetSeasonChanged => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "target_season_changed",
            "대상 길드의 활성 시즌이 변경되었습니다. 파일을 다시 검증해 주세요.",
            None,
        ),
        GuildDataTransferRepositoryError::MmrStateChanged => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "mmr_state_changed",
            "대상 길드의 MMR이 변경되었습니다. 파일을 다시 검증해 주세요.",
            None,
        ),
        GuildDataTransferRepositoryError::ActiveWorkflowConflict => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "active_workflow_conflict",
            "진행 중인 모집 또는 내전이 있어 MMR 백업을 적용할 수 없습니다.",
            None,
        ),
        GuildDataTransferRepositoryError::NoChanges => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "no_import_changes",
            "현재 길드 MMR과 동일하여 적용할 변경이 없습니다.",
            None,
        ),
        GuildDataTransferRepositoryError::InvalidMmrTransfer => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "invalid_mmr_transfer",
            "백업 파일의 MMR 값을 대상 길드에 적용할 수 없습니다.",
            None,
        ),
        GuildDataTransferRepositoryError::IdempotencyConflict => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_conflict",
            "같은 Idempotency-Key가 다른 요청에 사용되었습니다.",
            None,
        ),
        GuildDataTransferRepositoryError::IdempotencyInProgress => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_in_progress",
            "같은 가져오기 요청을 처리하고 있습니다.",
            None,
        ),
        GuildDataTransferRepositoryError::DatabaseBusy => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            request_id,
            "database_busy",
            "데이터베이스가 사용 중입니다. 잠시 후 다시 시도해 주세요.",
            None,
        ),
        GuildDataTransferRepositoryError::Database(database_error) => {
            tracing::error!(%database_error, "guild data transfer database operation failed");
            error::database_failure(&database_error, request_id)
        }
        GuildDataTransferRepositoryError::Serialization(serialization_error) => {
            tracing::error!(%serialization_error, "guild data transfer serialization failed");
            internal(request_id)
        }
        GuildDataTransferRepositoryError::InvariantViolation => {
            tracing::error!("guild data transfer stored invariant failed");
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
    fn browser_idempotency_key_rejects_duplicate_or_invalid_values() {
        let request_id = RequestId("request-id".to_owned());
        let mut headers = HeaderMap::new();
        headers.insert(
            IDEMPOTENCY_KEY,
            HeaderValue::from_static("01900000-0000-7000-8000-000000000001"),
        );
        assert!(idempotency_key(&headers, &request_id).is_ok());
        headers.append(IDEMPOTENCY_KEY, HeaderValue::from_static("duplicate"));
        assert!(idempotency_key(&headers, &request_id).is_err());
    }
}
