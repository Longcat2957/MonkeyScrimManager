use axum::{
    Extension, Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::Response,
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
    domain::GuildId,
    error::{self, ApiError},
    http::extractors::{AuthenticatedSession, BotServicePrincipal, require_csrf},
    repository::guild_admin_settings::{
        self, GuildAdminSettingsRepositoryError, ReplaceDiscordCatalogCommand,
        UpdateGuildSettingsCommand,
    },
    state::{AppState, RequestId},
};

pub async fn replace_discord_catalog(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(guild_id): Path<GuildId>,
    _principal: BotServicePrincipal,
    Json(command): Json<ReplaceDiscordCatalogCommand>,
) -> Result<Response, ApiError> {
    let result = guild_admin_settings::replace_discord_catalog(
        database(&state, &request_id)?,
        &guild_id,
        command,
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, result))
}

pub async fn settings(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(guild_id): Path<GuildId>,
    principal: AuthenticatedSession,
) -> Result<Response, ApiError> {
    let snapshot = guild_admin_settings::get(
        database(&state, &request_id)?,
        &guild_id,
        &principal.session.user_id,
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, snapshot))
}

pub async fn update_settings(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(guild_id): Path<GuildId>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(command): Json<UpdateGuildSettingsCommand>,
) -> Result<Response, ApiError> {
    require_csrf(&headers, &state, &principal, &request_id)?;
    let snapshot = guild_admin_settings::update(
        database(&state, &request_id)?,
        &guild_id,
        &principal.session.user_id,
        &request_id.0,
        command,
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

fn repository_error(source: GuildAdminSettingsRepositoryError, request_id: &RequestId) -> ApiError {
    match source {
        GuildAdminSettingsRepositoryError::GuildNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_id,
            "not_found",
            "요청한 길드를 찾을 수 없습니다.",
            None,
        ),
        GuildAdminSettingsRepositoryError::OperatorRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "operator_required",
            "길드 운영자 권한이 필요합니다.",
            None,
        ),
        GuildAdminSettingsRepositoryError::RevisionConflict {
            expected,
            current,
            updated_at,
            updated_by,
        } => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "revision_conflict",
            "최신 상태를 다시 확인해야 합니다.",
            Some(serde_json::json!({
                "expectedRevision": expected,
                "currentRevision": current,
                "updatedAt": format_timestamp(updated_at),
                "updatedBy": updated_by,
            })),
        ),
        GuildAdminSettingsRepositoryError::InvalidExpectedRevision => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "invalid_expected_revision",
            "expectedRevision은 0 이상이어야 합니다.",
            None,
        ),
        GuildAdminSettingsRepositoryError::RecruitmentChannelRequired => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "recruitment_channel_required",
            "모집 채널은 반드시 설정해야 합니다.",
            Some(serde_json::json!({ "field": "recruitmentChannelId" })),
        ),
        GuildAdminSettingsRepositoryError::InvalidSettings { field } => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "invalid_guild_settings",
            "길드 설정 값을 확인해 주세요.",
            Some(serde_json::json!({ "field": field })),
        ),
        GuildAdminSettingsRepositoryError::InvalidSelection { field } => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "invalid_settings_selection",
            "현재 Discord 서버에서 선택할 수 없는 설정입니다.",
            Some(serde_json::json!({ "field": field })),
        ),
        GuildAdminSettingsRepositoryError::InvalidCatalog { field } => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "invalid_discord_catalog",
            "Discord 역할 및 채널 목록이 올바르지 않습니다.",
            Some(serde_json::json!({ "field": field })),
        ),
        GuildAdminSettingsRepositoryError::DatabaseBusy => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            request_id,
            "database_busy",
            "데이터베이스가 사용 중입니다. 잠시 후 다시 시도해 주세요.",
            None,
        ),
        GuildAdminSettingsRepositoryError::Database(database_error) => {
            tracing::error!(%database_error, "guild admin settings database operation failed");
            error::database_failure(&database_error, request_id)
        }
        GuildAdminSettingsRepositoryError::Serialization(serialization_error) => {
            tracing::error!(%serialization_error, "guild admin settings serialization failed");
            internal(request_id)
        }
        GuildAdminSettingsRepositoryError::InvariantViolation => {
            tracing::error!("guild admin settings stored invariant failed");
            internal(request_id)
        }
    }
}

fn format_timestamp(value: OffsetDateTime) -> String {
    value.format(&Rfc3339).unwrap_or_else(|format_error| {
        tracing::error!(%format_error, "failed to format settings revision timestamp");
        String::new()
    })
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
