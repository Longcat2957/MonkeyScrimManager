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
    repository::{
        guild_full_data_transfer::{
            self, ApplyFullImportCommand, GuildFullDataTransferRepositoryError,
        },
        idempotency::IdempotencyRepositoryError,
    },
    state::{AppState, RequestId},
};

const IDEMPOTENCY_KEY: &str = "idempotency-key";
const MAX_IDEMPOTENCY_KEY_BYTES: usize = 128;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreviewFullImportRequest {
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
    let result = guild_full_data_transfer::export(
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
    Json(request): Json<PreviewFullImportRequest>,
) -> Result<Response, ApiError> {
    require_csrf(&headers, &state, &principal, &request_id)?;
    let result = guild_full_data_transfer::preview(
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
    Json(command): Json<ApplyFullImportCommand>,
) -> Result<Response, ApiError> {
    require_csrf(&headers, &state, &principal, &request_id)?;
    let idempotency_key = idempotency_key(&headers, &request_id)?;
    let result = guild_full_data_transfer::apply(
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

fn repository_error(
    source: GuildFullDataTransferRepositoryError,
    request_id: &RequestId,
) -> ApiError {
    match source {
        GuildFullDataTransferRepositoryError::GuildNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_id,
            "not_found",
            "요청한 길드를 찾을 수 없습니다.",
            None,
        ),
        GuildFullDataTransferRepositoryError::OperatorRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "operator_required",
            "길드 운영자 권한이 필요합니다.",
            None,
        ),
        GuildFullDataTransferRepositoryError::InvalidArchive => ApiError::new(
            StatusCode::BAD_REQUEST,
            request_id,
            "invalid_full_archive",
            "지원되는 MonkeyBot 전체 데이터 백업 파일 형식이 아닙니다.",
            Some(serde_json::json!({ "kind": "GUILD_FULL_SNAPSHOT", "schemaVersion": 1 })),
        ),
        GuildFullDataTransferRepositoryError::ArchiveRowLimitExceeded => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "full_archive_row_limit_exceeded",
            "전체 백업의 행 수가 지원 한도인 250,000개를 초과했습니다.",
            Some(serde_json::json!({ "maxRows": 250000 })),
        ),
        GuildFullDataTransferRepositoryError::ArchiveConstraintViolation { table } => {
            ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                request_id,
                "full_archive_constraint_violation",
                constraint_message(&table),
                Some(serde_json::json!({ "table": table })),
            )
        }
        GuildFullDataTransferRepositoryError::InvalidApplyCommand => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "invalid_full_import_apply_command",
            "전체 데이터 가져오기 적용 확인 값이 올바르지 않습니다.",
            None,
        ),
        GuildFullDataTransferRepositoryError::PreviewNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_id,
            "preview_not_found",
            "전체 데이터 가져오기 검토 결과를 찾을 수 없습니다. 파일을 다시 검증해 주세요.",
            None,
        ),
        GuildFullDataTransferRepositoryError::PreviewActorMismatch => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "preview_actor_mismatch",
            "가져오기 검토를 만든 운영자만 적용할 수 있습니다.",
            None,
        ),
        GuildFullDataTransferRepositoryError::PreviewExpired => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "preview_expired",
            "전체 데이터 가져오기 검토 결과가 만료되었습니다. 파일을 다시 검증해 주세요.",
            None,
        ),
        GuildFullDataTransferRepositoryError::PreviewAlreadyApplied => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "preview_already_applied",
            "이 전체 데이터 가져오기 검토 결과는 이미 적용되었습니다.",
            None,
        ),
        GuildFullDataTransferRepositoryError::PreviewDigestMismatch => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "preview_digest_mismatch",
            "검토한 전체 백업 파일과 적용 요청이 일치하지 않습니다.",
            None,
        ),
        GuildFullDataTransferRepositoryError::PreviewConflicts => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "preview_conflicts",
            "검토에서 확인된 충돌 때문에 전체 데이터를 적용할 수 없습니다.",
            None,
        ),
        GuildFullDataTransferRepositoryError::TargetStateChanged => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "full_import_target_changed",
            "검토 이후 대상 길드의 멤버 또는 데이터 상태가 변경되었습니다. 파일을 다시 검증해 주세요.",
            None,
        ),
        GuildFullDataTransferRepositoryError::IdempotencyConflict => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_conflict",
            "같은 Idempotency-Key가 다른 요청에 사용되었습니다.",
            None,
        ),
        GuildFullDataTransferRepositoryError::IdempotencyInProgress => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_in_progress",
            "같은 전체 데이터 가져오기 요청을 처리하고 있습니다.",
            None,
        ),
        GuildFullDataTransferRepositoryError::Database(database_error) => {
            tracing::error!(%database_error, "guild full data transfer database operation failed");
            error::database_failure(&database_error, request_id)
        }
        GuildFullDataTransferRepositoryError::Idempotency(IdempotencyRepositoryError::Sql(
            database_error,
        )) => {
            tracing::error!(%database_error, "guild full data transfer idempotency operation failed");
            error::database_failure(&database_error, request_id)
        }
        GuildFullDataTransferRepositoryError::Idempotency(idempotency_error) => {
            tracing::error!(%idempotency_error, "guild full data transfer idempotency invariant failed");
            internal(request_id)
        }
        GuildFullDataTransferRepositoryError::Serialization(serialization_error) => {
            tracing::error!(%serialization_error, "guild full data transfer serialization failed");
            internal(request_id)
        }
        GuildFullDataTransferRepositoryError::InvariantViolation => {
            tracing::error!("guild full data transfer stored invariant failed");
            internal(request_id)
        }
    }
}

fn table_label(table: &str) -> &'static str {
    match table {
        "users" | "user_discord_identities" | "guild_members" => "사용자·멤버",
        "riot_accounts" | "user_champion_preferences" => "Riot 계정·챔피언 선호",
        "seasons" | "user_lane_mmr" | "mmr_ledger_entries" => "시즌·MMR 이력",
        "recruitments"
        | "recruitment_participants"
        | "recruitment_preferred_lanes"
        | "recruitment_entry_drafts"
        | "recruitment_entry_assignments" => "일반 모집",
        "series" | "series_participants" => "일반 내전",
        "game_inputs" | "game_input_picks" | "game_input_bans" => "픽·밴 초안",
        "games" | "game_participants" | "game_participant_stats" | "game_bans" => "경기 전적",
        value if value.starts_with("auction_") => "경매",
        "admin_audit_log" => "관리자 감사 기록",
        _ => "보관",
    }
}

fn constraint_message(table: &str) -> &'static str {
    match table_label(table) {
        "사용자·멤버" => {
            "전체 백업의 사용자·멤버 데이터가 현재 MonkeyBot 데이터 규칙과 맞지 않습니다."
        }
        "Riot 계정·챔피언 선호" => {
            "전체 백업의 Riot 계정·챔피언 선호 데이터가 현재 MonkeyBot 데이터 규칙과 맞지 않습니다."
        }
        "시즌·MMR 이력" => {
            "전체 백업의 시즌·MMR 이력 데이터가 현재 MonkeyBot 데이터 규칙과 맞지 않습니다."
        }
        "일반 모집" => {
            "전체 백업의 일반 모집 데이터가 현재 MonkeyBot 데이터 규칙과 맞지 않습니다."
        }
        "일반 내전" => {
            "전체 백업의 일반 내전 데이터가 현재 MonkeyBot 데이터 규칙과 맞지 않습니다."
        }
        "픽·밴 초안" => {
            "전체 백업의 픽·밴 초안 데이터가 현재 MonkeyBot 데이터 규칙과 맞지 않습니다."
        }
        "경기 전적" => {
            "전체 백업의 경기 전적 데이터가 현재 MonkeyBot 데이터 규칙과 맞지 않습니다."
        }
        "경매" => "전체 백업의 경매 데이터가 현재 MonkeyBot 데이터 규칙과 맞지 않습니다.",
        "관리자 감사 기록" => {
            "전체 백업의 관리자 감사 기록이 현재 MonkeyBot 데이터 규칙과 맞지 않습니다."
        }
        _ => "전체 백업의 데이터가 현재 MonkeyBot 데이터 규칙과 맞지 않습니다.",
    }
}

fn internal(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        request_id,
        "internal_error",
        "전체 데이터 이동 요청을 처리하는 중 내부 오류가 발생했습니다.",
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constraint_errors_use_a_safe_section_label() {
        assert_eq!(table_label("games"), "경기 전적");
        assert_eq!(table_label("auction_actions"), "경매");
        assert_eq!(table_label("unknown"), "보관");
    }
}
