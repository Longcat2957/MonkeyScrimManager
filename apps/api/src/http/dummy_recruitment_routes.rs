use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::{get, post},
};

use crate::{
    domain::{GuildId, RecruitmentId, UserId},
    error::{self, ApiError},
    http::extractors::{AuthenticatedSession, require_csrf},
    repository::dummy_recruitment::{
        self, ApplyDummyParticipantsCommand, DummyRecruitmentAction, DummyRecruitmentError,
        PreviewDummyParticipantsCommand,
    },
    repository::idempotency::IdempotencyRepositoryError,
    state::{AppState, RequestId},
};

const IDEMPOTENCY_KEY: &str = "idempotency-key";
const MAX_IDEMPOTENCY_KEY_BYTES: usize = 128;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/dev/guilds/{guild_id}/recruitments/{recruitment_id}/participants",
            get(overview).post(add).delete(remove),
        )
        .route(
            "/api/dev/guilds/{guild_id}/recruitments/{recruitment_id}/participants/preview",
            post(preview),
        )
}

pub async fn overview(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    principal: AuthenticatedSession,
    Path((guild_id, recruitment_id)): Path<(GuildId, RecruitmentId)>,
) -> Result<Response, ApiError> {
    let actor_id = authorize(&state, &principal, &request_id)?;
    let data = dummy_recruitment::overview(
        database(&state, &request_id)?,
        &guild_id,
        recruitment_id,
        actor_id,
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, data))
}

pub async fn preview(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    principal: AuthenticatedSession,
    Path((guild_id, recruitment_id)): Path<(GuildId, RecruitmentId)>,
    Json(command): Json<PreviewDummyParticipantsCommand>,
) -> Result<Response, ApiError> {
    let actor_id = authorize(&state, &principal, &request_id)?;
    let data = dummy_recruitment::preview(
        database(&state, &request_id)?,
        &guild_id,
        recruitment_id,
        actor_id,
        command,
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, data))
}

pub async fn add(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Path((guild_id, recruitment_id)): Path<(GuildId, RecruitmentId)>,
    Json(command): Json<ApplyDummyParticipantsCommand>,
) -> Result<Response, ApiError> {
    apply(
        &state,
        &request_id,
        &headers,
        &principal,
        guild_id,
        recruitment_id,
        DummyRecruitmentAction::Add,
        command,
    )
    .await
}

pub async fn remove(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Path((guild_id, recruitment_id)): Path<(GuildId, RecruitmentId)>,
    Json(command): Json<ApplyDummyParticipantsCommand>,
) -> Result<Response, ApiError> {
    apply(
        &state,
        &request_id,
        &headers,
        &principal,
        guild_id,
        recruitment_id,
        DummyRecruitmentAction::Remove,
        command,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn apply(
    state: &AppState,
    request_id: &RequestId,
    headers: &HeaderMap,
    principal: &AuthenticatedSession,
    guild_id: GuildId,
    recruitment_id: RecruitmentId,
    action: DummyRecruitmentAction,
    command: ApplyDummyParticipantsCommand,
) -> Result<Response, ApiError> {
    let actor_id = authorize_mutation(state, principal, headers, request_id)?;
    let idempotency_key = idempotency_key(headers, request_id)?;
    let data = dummy_recruitment::apply(
        database(state, request_id)?,
        &guild_id,
        recruitment_id,
        actor_id,
        &idempotency_key,
        &request_id.0,
        action,
        command,
    )
    .await
    .map_err(|source| repository_error(source, request_id))?;
    Ok(error::success(StatusCode::OK, request_id, data))
}

fn authorize<'a>(
    state: &AppState,
    principal: &'a AuthenticatedSession,
    request_id: &RequestId,
) -> Result<&'a UserId, ApiError> {
    if !state.config().dummy_users_enabled() {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "dummy_users_disabled",
            "개발용 더미 사용자 기능이 비활성화되어 있습니다.",
            None,
        ));
    }
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
    Ok(&principal.session.user_id)
}

fn authorize_mutation<'a>(
    state: &AppState,
    principal: &'a AuthenticatedSession,
    headers: &HeaderMap,
    request_id: &RequestId,
) -> Result<&'a UserId, ApiError> {
    let actor_id = authorize(state, principal, request_id)?;
    require_csrf(headers, state, principal, request_id)?;
    Ok(actor_id)
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
                "Idempotency-Key 헤더가 필요합니다.",
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

fn repository_error(source: DummyRecruitmentError, request_id: &RequestId) -> ApiError {
    match source {
        DummyRecruitmentError::OperatorRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "operator_required",
            "해당 길드의 운영자 권한이 필요합니다.",
            None,
        ),
        DummyRecruitmentError::RecruitmentNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_id,
            "recruitment_not_found",
            "모집을 찾을 수 없습니다.",
            None,
        ),
        DummyRecruitmentError::RecruitmentNotEditable(status) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "recruitment_not_editable",
            "현재 모집 상태에서는 참가자를 변경할 수 없습니다.",
            Some(serde_json::json!({ "currentStatus": status.as_str() })),
        ),
        DummyRecruitmentError::InvalidExpectedRevision => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "invalid_expected_revision",
            "expectedRevision은 0 이상이어야 합니다.",
            None,
        ),
        DummyRecruitmentError::RevisionConflict { expected, current } => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "revision_conflict",
            "모집 참가자 정보가 변경되었습니다. 최신 상태를 다시 확인해 주세요.",
            Some(serde_json::json!({
                "expectedRevision": expected,
                "currentRevision": current,
            })),
        ),
        DummyRecruitmentError::InvalidSelection => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "invalid_dummy_participant_selection",
            "더미 참가자는 중복 없이 1명 이상 10명 이하로 선택해야 합니다.",
            None,
        ),
        DummyRecruitmentError::DummyUserUnavailable(user_id) => ApiError::new(
            StatusCode::NOT_FOUND,
            request_id,
            "dummy_user_not_found",
            "이 길드에 배치된 더미 사용자를 찾을 수 없습니다.",
            Some(serde_json::json!({ "userId": user_id })),
        ),
        DummyRecruitmentError::DummyUserArchived(user_id) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "dummy_user_archived",
            "보관된 더미 사용자는 모집에 추가할 수 없습니다.",
            Some(serde_json::json!({ "userId": user_id })),
        ),
        DummyRecruitmentError::DummyMembershipInactive(user_id) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "dummy_membership_inactive",
            "비활성 길드 회원은 모집에 추가할 수 없습니다.",
            Some(serde_json::json!({ "userId": user_id })),
        ),
        DummyRecruitmentError::AlreadyParticipant(user_id) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "already_participant",
            "이미 모집에 참가 중인 더미 사용자입니다.",
            Some(serde_json::json!({ "userId": user_id })),
        ),
        DummyRecruitmentError::NotParticipant(user_id) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "not_participant",
            "현재 모집 참가자가 아닌 더미 사용자입니다.",
            Some(serde_json::json!({ "userId": user_id })),
        ),
        DummyRecruitmentError::CapacityExceeded {
            joined,
            requested,
            target,
        } => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "recruitment_capacity_exceeded",
            "선택한 사용자를 추가하면 모집 정원을 초과합니다.",
            Some(serde_json::json!({
                "joinedCount": joined,
                "requestedCount": requested,
                "targetCount": target,
            })),
        ),
        DummyRecruitmentError::IdempotencyConflict => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_conflict",
            "같은 Idempotency-Key가 다른 요청에 사용되었습니다.",
            None,
        ),
        DummyRecruitmentError::IdempotencyInProgress => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_in_progress",
            "같은 더미 참가자 요청을 처리하고 있습니다.",
            None,
        ),
        DummyRecruitmentError::Database(source)
        | DummyRecruitmentError::Idempotency(IdempotencyRepositoryError::Sql(source)) => {
            tracing::error!(%source, "dummy recruitment database operation failed");
            error::database_failure(&source, request_id)
        }
        DummyRecruitmentError::Serialization(_)
        | DummyRecruitmentError::Idempotency(_)
        | DummyRecruitmentError::StoredInvariantViolation => {
            tracing::error!("dummy recruitment invariant failed");
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                request_id,
                "dummy_recruitment_invariant_failed",
                "더미 참가자 처리 결과의 일관성을 확인할 수 없습니다.",
                None,
            )
        }
    }
}
