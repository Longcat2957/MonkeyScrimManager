use std::str::FromStr as _;

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::{get, patch, post, put},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    domain::{GuildId, Lane, UserId},
    error::{self, ApiError},
    http::extractors::{AuthenticatedSession, require_csrf},
    repository::dummy_users::{
        self, CreateDummyUserCommand, CreateDummyUserGroupCommand, DummyUserListFilter,
        DummyUserState, DummyUsersRepositoryError, ExpectedRevisionCommand,
        ReplaceDummyGroupMembersCommand, UpdateDummyUserCommand, UpdateDummyUserGroupCommand,
        UpsertDummyGuildMembershipCommand, UpsertDummyRiotAccountCommand,
    },
    state::{AppState, RequestId},
};

const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 100;
const IDEMPOTENCY_KEY: &str = "idempotency-key";
const MAX_IDEMPOTENCY_KEY_BYTES: usize = 128;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/dev/dummy-users", get(list_users).post(create_user))
        .route("/api/dev/dummy-users/catalog", get(guild_catalog))
        .route("/api/dev/dummy-users/bulk", post(create_users_bulk))
        .route(
            "/api/dev/dummy-users/{user_id}",
            get(get_user).patch(update_user).delete(delete_user),
        )
        .route(
            "/api/dev/dummy-users/{user_id}/deletion-impact",
            get(deletion_impact),
        )
        .route("/api/dev/dummy-users/{user_id}/archive", post(archive_user))
        .route("/api/dev/dummy-users/{user_id}/restore", post(restore_user))
        .route(
            "/api/dev/dummy-users/{user_id}/guilds/{guild_id}",
            put(upsert_guild_membership).delete(remove_guild_membership),
        )
        .route(
            "/api/dev/dummy-users/{user_id}/champion-preferences/{lane}",
            put(replace_champion_preferences),
        )
        .route(
            "/api/dev/dummy-users/{user_id}/riot-account",
            put(upsert_riot_account).delete(remove_riot_account),
        )
        .route("/api/dev/dummy-groups", get(list_groups).post(create_group))
        .route(
            "/api/dev/dummy-groups/{group_id}",
            patch(update_group).delete(delete_group),
        )
        .route(
            "/api/dev/dummy-groups/{group_id}/members",
            put(replace_group_members),
        )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DummyUserListQuery {
    query: Option<String>,
    state: Option<DummyUserState>,
    guild_id: Option<GuildId>,
    group_id: Option<Uuid>,
    #[serde(default = "default_limit")]
    limit: i64,
    #[serde(default)]
    offset: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CreateDummyUsersBulkRequest {
    users: Vec<CreateDummyUserCommand>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ChampionPreferencesRequest {
    champion_ids: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BulkUsersData {
    total: usize,
    users: Vec<dummy_users::DummyUser>,
}

#[derive(Serialize)]
struct DeletedData {
    deleted: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeletedUserData {
    deleted: bool,
    impact: dummy_users::DummyDeletionImpact,
}

pub async fn list_users(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    principal: AuthenticatedSession,
    Query(query): Query<DummyUserListQuery>,
) -> Result<Response, ApiError> {
    authorize(&state, &principal, &request_id)?;
    if !(1..=MAX_LIMIT).contains(&query.limit) || query.offset < 0 {
        return Err(invalid_input(&request_id, "pagination"));
    }
    let data = dummy_users::list(
        database(&state, &request_id)?,
        DummyUserListFilter {
            query: query.query,
            state: query.state,
            guild_id: query.guild_id,
            group_id: query.group_id,
            limit: query.limit,
            offset: query.offset,
        },
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, data))
}

pub async fn guild_catalog(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    principal: AuthenticatedSession,
) -> Result<Response, ApiError> {
    authorize(&state, &principal, &request_id)?;
    let data = dummy_users::guild_catalog(database(&state, &request_id)?)
        .await
        .map_err(|source| repository_error(source, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, data))
}

pub async fn get_user(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    principal: AuthenticatedSession,
    Path(user_id): Path<UserId>,
) -> Result<Response, ApiError> {
    authorize(&state, &principal, &request_id)?;
    let user = dummy_users::get(database(&state, &request_id)?, &user_id)
        .await
        .map_err(|source| repository_error(source, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, user))
}

pub async fn deletion_impact(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    principal: AuthenticatedSession,
    Path(user_id): Path<UserId>,
) -> Result<Response, ApiError> {
    authorize(&state, &principal, &request_id)?;
    let impact = dummy_users::deletion_impact(database(&state, &request_id)?, &user_id)
        .await
        .map_err(|source| repository_error(source, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, impact))
}

pub async fn delete_user(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    principal: AuthenticatedSession,
    headers: HeaderMap,
    Path(user_id): Path<UserId>,
) -> Result<Response, ApiError> {
    let actor_id = authorize_mutation(&state, &principal, &headers, &request_id)?;
    let idempotency_key = idempotency_key(&headers, &request_id)?;
    let impact = dummy_users::delete_user(
        database(&state, &request_id)?,
        &user_id,
        actor_id,
        &request_id.0,
        &idempotency_key,
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    Ok(error::success(
        StatusCode::OK,
        &request_id,
        DeletedUserData {
            deleted: true,
            impact,
        },
    ))
}

pub async fn create_user(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    principal: AuthenticatedSession,
    headers: HeaderMap,
    Json(command): Json<CreateDummyUserCommand>,
) -> Result<Response, ApiError> {
    let actor_id = authorize_mutation(&state, &principal, &headers, &request_id)?;
    let idempotency_key = idempotency_key(&headers, &request_id)?;
    let user = dummy_users::create(
        database(&state, &request_id)?,
        actor_id,
        &request_id.0,
        &idempotency_key,
        command,
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    Ok(error::success(StatusCode::CREATED, &request_id, user))
}

pub async fn create_users_bulk(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    principal: AuthenticatedSession,
    headers: HeaderMap,
    Json(request): Json<CreateDummyUsersBulkRequest>,
) -> Result<Response, ApiError> {
    let actor_id = authorize_mutation(&state, &principal, &headers, &request_id)?;
    let idempotency_key = idempotency_key(&headers, &request_id)?;
    let users = dummy_users::create_bulk(
        database(&state, &request_id)?,
        actor_id,
        &request_id.0,
        &idempotency_key,
        request.users,
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    let total = users.len();
    Ok(error::success(
        StatusCode::CREATED,
        &request_id,
        BulkUsersData { total, users },
    ))
}

pub async fn update_user(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    principal: AuthenticatedSession,
    headers: HeaderMap,
    Path(user_id): Path<UserId>,
    Json(command): Json<UpdateDummyUserCommand>,
) -> Result<Response, ApiError> {
    let actor_id = authorize_mutation(&state, &principal, &headers, &request_id)?;
    let idempotency_key = idempotency_key(&headers, &request_id)?;
    let user = dummy_users::update(
        database(&state, &request_id)?,
        &user_id,
        actor_id,
        &request_id.0,
        &idempotency_key,
        command,
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, user))
}

pub async fn archive_user(
    state: State<AppState>,
    request_id: Extension<RequestId>,
    principal: AuthenticatedSession,
    headers: HeaderMap,
    path: Path<UserId>,
    command: Json<ExpectedRevisionCommand>,
) -> Result<Response, ApiError> {
    set_archived(state, request_id, principal, headers, path, command, true).await
}

pub async fn restore_user(
    state: State<AppState>,
    request_id: Extension<RequestId>,
    principal: AuthenticatedSession,
    headers: HeaderMap,
    path: Path<UserId>,
    command: Json<ExpectedRevisionCommand>,
) -> Result<Response, ApiError> {
    set_archived(state, request_id, principal, headers, path, command, false).await
}

#[allow(clippy::too_many_arguments)]
async fn set_archived(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    principal: AuthenticatedSession,
    headers: HeaderMap,
    Path(user_id): Path<UserId>,
    Json(command): Json<ExpectedRevisionCommand>,
    archived: bool,
) -> Result<Response, ApiError> {
    let actor_id = authorize_mutation(&state, &principal, &headers, &request_id)?;
    let idempotency_key = idempotency_key(&headers, &request_id)?;
    let user = dummy_users::set_archived(
        database(&state, &request_id)?,
        &user_id,
        actor_id,
        &request_id.0,
        &idempotency_key,
        command.expected_revision,
        archived,
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, user))
}

pub async fn upsert_guild_membership(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    principal: AuthenticatedSession,
    headers: HeaderMap,
    Path((user_id, guild_id)): Path<(UserId, GuildId)>,
    Json(command): Json<UpsertDummyGuildMembershipCommand>,
) -> Result<Response, ApiError> {
    let actor_id = authorize_mutation(&state, &principal, &headers, &request_id)?;
    let idempotency_key = idempotency_key(&headers, &request_id)?;
    let user = dummy_users::upsert_guild_membership(
        database(&state, &request_id)?,
        &user_id,
        &guild_id,
        actor_id,
        &request_id.0,
        &idempotency_key,
        command,
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, user))
}

pub async fn remove_guild_membership(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    principal: AuthenticatedSession,
    headers: HeaderMap,
    Path((user_id, guild_id)): Path<(UserId, GuildId)>,
) -> Result<Response, ApiError> {
    let actor_id = authorize_mutation(&state, &principal, &headers, &request_id)?;
    let idempotency_key = idempotency_key(&headers, &request_id)?;
    let user = dummy_users::remove_guild_membership(
        database(&state, &request_id)?,
        &user_id,
        &guild_id,
        actor_id,
        &request_id.0,
        &idempotency_key,
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, user))
}

pub async fn replace_champion_preferences(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    principal: AuthenticatedSession,
    headers: HeaderMap,
    Path((user_id, lane)): Path<(UserId, String)>,
    Json(request): Json<ChampionPreferencesRequest>,
) -> Result<Response, ApiError> {
    let actor_id = authorize_mutation(&state, &principal, &headers, &request_id)?;
    let idempotency_key = idempotency_key(&headers, &request_id)?;
    let lane = Lane::from_str(&lane).map_err(|_| invalid_input(&request_id, "lane"))?;
    let user = dummy_users::replace_champion_preferences(
        database(&state, &request_id)?,
        &user_id,
        actor_id,
        &request_id.0,
        &idempotency_key,
        lane,
        request.champion_ids,
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, user))
}

pub async fn upsert_riot_account(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    principal: AuthenticatedSession,
    headers: HeaderMap,
    Path(user_id): Path<UserId>,
    Json(command): Json<UpsertDummyRiotAccountCommand>,
) -> Result<Response, ApiError> {
    let actor_id = authorize_mutation(&state, &principal, &headers, &request_id)?;
    let idempotency_key = idempotency_key(&headers, &request_id)?;
    let user = dummy_users::upsert_riot_account(
        database(&state, &request_id)?,
        &user_id,
        actor_id,
        &request_id.0,
        &idempotency_key,
        command,
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, user))
}

pub async fn remove_riot_account(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    principal: AuthenticatedSession,
    headers: HeaderMap,
    Path(user_id): Path<UserId>,
) -> Result<Response, ApiError> {
    let actor_id = authorize_mutation(&state, &principal, &headers, &request_id)?;
    let idempotency_key = idempotency_key(&headers, &request_id)?;
    let user = dummy_users::remove_riot_account(
        database(&state, &request_id)?,
        &user_id,
        actor_id,
        &request_id.0,
        &idempotency_key,
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, user))
}

pub async fn list_groups(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    principal: AuthenticatedSession,
) -> Result<Response, ApiError> {
    authorize(&state, &principal, &request_id)?;
    let groups = dummy_users::list_groups(database(&state, &request_id)?)
        .await
        .map_err(|source| repository_error(source, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, groups))
}

pub async fn create_group(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    principal: AuthenticatedSession,
    headers: HeaderMap,
    Json(command): Json<CreateDummyUserGroupCommand>,
) -> Result<Response, ApiError> {
    let actor_id = authorize_mutation(&state, &principal, &headers, &request_id)?;
    let idempotency_key = idempotency_key(&headers, &request_id)?;
    let group = dummy_users::create_group(
        database(&state, &request_id)?,
        actor_id,
        &request_id.0,
        &idempotency_key,
        command,
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    Ok(error::success(StatusCode::CREATED, &request_id, group))
}

pub async fn update_group(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    principal: AuthenticatedSession,
    headers: HeaderMap,
    Path(group_id): Path<Uuid>,
    Json(command): Json<UpdateDummyUserGroupCommand>,
) -> Result<Response, ApiError> {
    let actor_id = authorize_mutation(&state, &principal, &headers, &request_id)?;
    let idempotency_key = idempotency_key(&headers, &request_id)?;
    let group = dummy_users::update_group(
        database(&state, &request_id)?,
        group_id,
        actor_id,
        &request_id.0,
        &idempotency_key,
        command,
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, group))
}

pub async fn replace_group_members(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    principal: AuthenticatedSession,
    headers: HeaderMap,
    Path(group_id): Path<Uuid>,
    Json(command): Json<ReplaceDummyGroupMembersCommand>,
) -> Result<Response, ApiError> {
    let actor_id = authorize_mutation(&state, &principal, &headers, &request_id)?;
    let idempotency_key = idempotency_key(&headers, &request_id)?;
    let group = dummy_users::replace_group_members(
        database(&state, &request_id)?,
        group_id,
        actor_id,
        &request_id.0,
        &idempotency_key,
        command,
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, group))
}

pub async fn delete_group(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    principal: AuthenticatedSession,
    headers: HeaderMap,
    Path(group_id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let actor_id = authorize_mutation(&state, &principal, &headers, &request_id)?;
    let idempotency_key = idempotency_key(&headers, &request_id)?;
    dummy_users::delete_group(
        database(&state, &request_id)?,
        group_id,
        actor_id,
        &request_id.0,
        &idempotency_key,
    )
    .await
    .map_err(|source| repository_error(source, &request_id))?;
    Ok(error::success(
        StatusCode::OK,
        &request_id,
        DeletedData { deleted: true },
    ))
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

fn repository_error(source: DummyUsersRepositoryError, request_id: &RequestId) -> ApiError {
    match source {
        DummyUsersRepositoryError::UserNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_id,
            "dummy_user_not_found",
            "더미 사용자를 찾을 수 없습니다.",
            None,
        ),
        DummyUsersRepositoryError::GroupNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_id,
            "dummy_group_not_found",
            "더미 사용자 그룹을 찾을 수 없습니다.",
            None,
        ),
        DummyUsersRepositoryError::GuildNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_id,
            "guild_not_found",
            "배치할 길드를 찾을 수 없습니다.",
            None,
        ),
        DummyUsersRepositoryError::MembershipNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_id,
            "dummy_membership_not_found",
            "해당 길드의 더미 회원을 찾을 수 없습니다.",
            None,
        ),
        DummyUsersRepositoryError::RiotAccountNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_id,
            "dummy_riot_account_not_found",
            "합성 Riot 계정을 찾을 수 없습니다.",
            None,
        ),
        DummyUsersRepositoryError::MembershipHasHistory => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "dummy_membership_has_history",
            "내전 이력이 있는 길드 소속은 제거할 수 없습니다. 비활성 상태로 변경해 주세요.",
            None,
        ),
        DummyUsersRepositoryError::DeletionBlocked(impact) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "dummy_user_deletion_blocked",
            "연결된 내전 데이터가 있어 삭제할 수 없습니다. 더미 사용자를 보관해 주세요.",
            Some(serde_json::json!({ "impact": impact })),
        ),
        DummyUsersRepositoryError::ArchiveBlockedByActiveRecruitment {
            recruitments,
            auction_recruitments,
        } => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "dummy_user_archive_blocked_by_recruitment",
            "진행 중인 일반 또는 경매 모집에 참가 중이라 보관할 수 없습니다. 먼저 참가를 취소해 주세요.",
            Some(serde_json::json!({
                "joinedRecruitments": recruitments,
                "joinedAuctionRecruitments": auction_recruitments,
            })),
        ),
        DummyUsersRepositoryError::ArchivedUser => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "dummy_user_archived",
            "보관된 더미 사용자는 복원한 뒤 수정하거나 배치할 수 있습니다.",
            None,
        ),
        DummyUsersRepositoryError::NoActiveSeason => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "active_season_required",
            "초기 MMR을 저장하려면 길드의 활성 시즌이 필요합니다.",
            None,
        ),
        DummyUsersRepositoryError::DuplicateStableKey => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "dummy_stable_key_conflict",
            "이미 사용 중인 더미 사용자 식별 키입니다.",
            None,
        ),
        DummyUsersRepositoryError::DuplicateGroupName => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "dummy_group_name_conflict",
            "같은 이름의 더미 사용자 그룹이 이미 있습니다.",
            None,
        ),
        DummyUsersRepositoryError::GroupMemberUnavailable => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "dummy_group_member_unavailable",
            "그룹에는 활성 상태인 더미 사용자만 추가할 수 있습니다.",
            None,
        ),
        DummyUsersRepositoryError::InvalidInput { field } => invalid_input(request_id, field),
        DummyUsersRepositoryError::InvalidExpectedRevision => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "invalid_expected_revision",
            "expectedRevision은 0 이상이어야 합니다.",
            None,
        ),
        DummyUsersRepositoryError::RevisionConflict { expected, current } => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "revision_conflict",
            "더미 사용자 데이터가 변경되었습니다. 최신 상태를 다시 확인해 주세요.",
            Some(serde_json::json!({
                "expectedRevision": expected,
                "currentRevision": current,
            })),
        ),
        DummyUsersRepositoryError::IdempotencyConflict => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_conflict",
            "같은 Idempotency-Key가 다른 요청에 사용되었습니다.",
            None,
        ),
        DummyUsersRepositoryError::IdempotencyInProgress => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_in_progress",
            "같은 요청을 처리하고 있습니다.",
            None,
        ),
        DummyUsersRepositoryError::Serialization(source) => {
            tracing::error!(%source, "dummy users serialization failed");
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                request_id,
                "internal_error",
                "요청을 처리하는 중 응답 데이터를 구성하지 못했습니다.",
                None,
            )
        }
        DummyUsersRepositoryError::Idempotency(source) => {
            tracing::error!(%source, "dummy users idempotency operation failed");
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                request_id,
                "internal_error",
                "요청의 중복 처리 상태를 확인하지 못했습니다.",
                None,
            )
        }
        DummyUsersRepositoryError::Database(source) => {
            tracing::error!(%source, "dummy users database operation failed");
            error::database_failure(&source, request_id)
        }
        DummyUsersRepositoryError::StoredInvariantViolation => {
            tracing::error!("dummy users stored invariant failed");
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                request_id,
                "dummy_user_invariant_failed",
                "저장된 더미 사용자 데이터의 일관성을 확인할 수 없습니다.",
                None,
            )
        }
    }
}

fn invalid_input(request_id: &RequestId, field: &'static str) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        request_id,
        "invalid_dummy_user_input",
        "더미 사용자 입력값을 확인해 주세요.",
        Some(serde_json::json!({ "field": field })),
    )
}

const fn default_limit() -> i64 {
    DEFAULT_LIMIT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_query_uses_camel_case_and_strict_values() {
        let query: DummyUserListQuery = serde_json::from_value(serde_json::json!({
            "state": "ACTIVE",
            "limit": 25,
            "offset": 0
        }))
        .expect("valid query contract");
        assert_eq!(query.limit, 25);
        assert!(
            serde_json::from_value::<DummyUserListQuery>(serde_json::json!({
                "unknown": true
            }))
            .is_err()
        );
    }
}
