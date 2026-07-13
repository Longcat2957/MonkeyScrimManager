use axum::{
    Extension, Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::Response,
};
use serde::{Deserialize, Serialize};

use crate::{
    domain::{
        DiscordChannelId, DiscordInteractionId, DiscordMessageId, DiscordRoleId, DiscordUserId,
        GuildId, Lane, RecruitmentId, UserId,
    },
    error::{self, ApiError},
    http::extractors::BotServicePrincipal,
    repository::bot_workflow::{
        self, BotActor, BotRecruitmentCommand, BotWorkflowError, RegisterGuildMemberCommand,
        RegisterGuildMemberInput, SetupGuildCommand,
    },
    state::{AppState, RequestId},
};

const IDEMPOTENCY_KEY: &str = "idempotency-key";

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ActorRequest {
    id: DiscordUserId,
    global_display_name: String,
    display_name: String,
    avatar_hash: Option<String>,
    role_ids: Vec<DiscordRoleId>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetupGuildRequest {
    actor: ActorRequest,
    guild_name: String,
    guild_icon_hash: Option<String>,
    operator_role_id: DiscordRoleId,
    season_name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActorOnlyRequest {
    actor: ActorRequest,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegisterGuildMemberRequest {
    actor: ActorRequest,
    game_name: String,
    tag_line: String,
    platform: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BindMessageRequest {
    actor: ActorRequest,
    channel_id: DiscordChannelId,
    message_id: DiscordMessageId,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JoinMemberRequest {
    actor: ActorRequest,
    member: ActorRequest,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreferredLanesRequest {
    actor: ActorRequest,
    preferred_lanes: Vec<Lane>,
}

#[derive(Serialize)]
struct SetupGuildData {
    guild: SetupGuildResponse,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SetupGuildResponse {
    id: GuildId,
    display_name: String,
    operator_role_id: DiscordRoleId,
    current_season: SetupSeasonResponse,
    dashboard_href: String,
}

#[derive(Serialize)]
struct SetupSeasonResponse {
    id: String,
    name: String,
}

pub async fn setup_guild(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(guild_id): Path<GuildId>,
    headers: HeaderMap,
    _principal: BotServicePrincipal,
    Json(request): Json<SetupGuildRequest>,
) -> Result<Response, ApiError> {
    let interaction_id = interaction_id(&headers, &request_id)?;
    validate_text(&request.guild_name, 100, "guildName", &request_id)?;
    validate_text(&request.season_name, 100, "seasonName", &request_id)?;
    if request
        .guild_icon_hash
        .as_ref()
        .is_some_and(|hash| hash.is_empty() || hash.len() > 128)
    {
        return Err(invalid_request("guildIconHash", &request_id));
    }
    if request.operator_role_id.as_str() == guild_id.as_str() {
        return Err(invalid_request("operatorRoleId", &request_id));
    }
    let actor = actor(request.actor, &request_id)?;
    let result = bot_workflow::setup_guild(
        database(&state, &request_id)?,
        &guild_id,
        &interaction_id,
        &request_id,
        SetupGuildCommand {
            actor,
            guild_name: request.guild_name.trim().to_owned(),
            guild_icon_hash: request.guild_icon_hash,
            operator_role_id: request.operator_role_id,
            season_name: request.season_name.trim().to_owned(),
        },
    )
    .await
    .map_err(|error| command_error(error, &request_id))?;

    Ok(error::success(
        StatusCode::OK,
        &request_id,
        SetupGuildData {
            guild: SetupGuildResponse {
                id: result.guild_id,
                display_name: result.display_name,
                operator_role_id: result.operator_role_id,
                current_season: SetupSeasonResponse {
                    id: result.current_season.id.to_string(),
                    name: result.current_season.name,
                },
                dashboard_href: result.dashboard_href,
            },
        },
    ))
}

pub async fn register_guild_member(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(guild_id): Path<GuildId>,
    headers: HeaderMap,
    _principal: BotServicePrincipal,
    Json(request): Json<RegisterGuildMemberRequest>,
) -> Result<Response, ApiError> {
    let interaction_id = interaction_id(&headers, &request_id)?;
    let actor = actor(request.actor, &request_id)?;
    let platform = super::riot_routes::parse_platform(&request.platform, &request_id)?;
    crate::integrations::riot::RiotId::new(&request.game_name, &request.tag_line)
        .map_err(|source| super::riot_routes::riot_client_error(source, &request_id))?;
    let input = RegisterGuildMemberInput {
        actor,
        requested_game_name: request.game_name.trim().to_owned(),
        requested_tag_line: request.tag_line.trim().to_owned(),
        requested_platform: platform.as_str().to_owned(),
    };
    let pool = database(&state, &request_id)?;
    if let Some(result) =
        bot_workflow::preflight_guild_registration(pool, &guild_id, &interaction_id, &input)
            .await
            .map_err(|error| registration_command_error(error, &request_id))?
    {
        return Ok(error::success(StatusCode::OK, &request_id, result));
    }
    let resolved = super::riot_routes::riot_service(&state, &request_id)?
        .resolve_account(
            &input.requested_game_name,
            &input.requested_tag_line,
            platform,
        )
        .await
        .map_err(|source| super::riot_routes::riot_client_error(source, &request_id))?;
    let canonical = super::riot_routes::canonical_account(resolved, &request_id)?;
    let result = bot_workflow::register_guild_member(
        pool,
        &guild_id,
        &interaction_id,
        RegisterGuildMemberCommand {
            input,
            account: canonical,
        },
    )
    .await
    .map_err(|error| registration_command_error(error, &request_id))?;

    Ok(error::success(StatusCode::OK, &request_id, result))
}

pub async fn create_recruitment(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(guild_id): Path<GuildId>,
    headers: HeaderMap,
    _principal: BotServicePrincipal,
    Json(request): Json<ActorOnlyRequest>,
) -> Result<Response, ApiError> {
    recruitment_command(
        &state,
        &request_id,
        &headers,
        &guild_id,
        StatusCode::CREATED,
        BotRecruitmentCommand::Create {
            actor: actor(request.actor, &request_id)?,
        },
    )
    .await
}

pub async fn bind_recruitment_message(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, recruitment_id)): Path<(GuildId, RecruitmentId)>,
    headers: HeaderMap,
    _principal: BotServicePrincipal,
    Json(request): Json<BindMessageRequest>,
) -> Result<Response, ApiError> {
    recruitment_command(
        &state,
        &request_id,
        &headers,
        &guild_id,
        StatusCode::OK,
        BotRecruitmentCommand::BindMessage {
            actor: actor(request.actor, &request_id)?,
            recruitment_id,
            channel_id: request.channel_id,
            message_id: request.message_id,
        },
    )
    .await
}

pub async fn join_recruitment(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, recruitment_id)): Path<(GuildId, RecruitmentId)>,
    headers: HeaderMap,
    _principal: BotServicePrincipal,
    Json(request): Json<JoinMemberRequest>,
) -> Result<Response, ApiError> {
    recruitment_command(
        &state,
        &request_id,
        &headers,
        &guild_id,
        StatusCode::OK,
        BotRecruitmentCommand::Join {
            actor: actor(request.actor, &request_id)?,
            recruitment_id,
            participant: actor(request.member, &request_id)?,
        },
    )
    .await
}

pub async fn leave_recruitment(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, recruitment_id, user_id)): Path<(GuildId, RecruitmentId, UserId)>,
    headers: HeaderMap,
    _principal: BotServicePrincipal,
    Json(request): Json<ActorOnlyRequest>,
) -> Result<Response, ApiError> {
    recruitment_command(
        &state,
        &request_id,
        &headers,
        &guild_id,
        StatusCode::OK,
        BotRecruitmentCommand::Leave {
            actor: actor(request.actor, &request_id)?,
            recruitment_id,
            user_id,
        },
    )
    .await
}

pub async fn set_preferred_lanes(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, recruitment_id, user_id)): Path<(GuildId, RecruitmentId, UserId)>,
    headers: HeaderMap,
    _principal: BotServicePrincipal,
    Json(request): Json<PreferredLanesRequest>,
) -> Result<Response, ApiError> {
    if request.preferred_lanes.is_empty()
        || request.preferred_lanes.len() > 5
        || has_duplicate_lanes(&request.preferred_lanes)
    {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            &request_id,
            "invalid_preferred_lanes",
            "선호 라인은 중복 없이 1개 이상 선택해야 합니다.",
            None,
        ));
    }
    recruitment_command(
        &state,
        &request_id,
        &headers,
        &guild_id,
        StatusCode::OK,
        BotRecruitmentCommand::SetPreferredLanes {
            actor: actor(request.actor, &request_id)?,
            recruitment_id,
            user_id,
            lanes: request.preferred_lanes,
        },
    )
    .await
}

pub async fn close_recruitment(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, recruitment_id)): Path<(GuildId, RecruitmentId)>,
    headers: HeaderMap,
    _principal: BotServicePrincipal,
    Json(request): Json<ActorOnlyRequest>,
) -> Result<Response, ApiError> {
    recruitment_command(
        &state,
        &request_id,
        &headers,
        &guild_id,
        StatusCode::OK,
        BotRecruitmentCommand::Close {
            actor: actor(request.actor, &request_id)?,
            recruitment_id,
        },
    )
    .await
}

async fn recruitment_command(
    state: &AppState,
    request_id: &RequestId,
    headers: &HeaderMap,
    guild_id: &GuildId,
    success_status: StatusCode,
    command: BotRecruitmentCommand,
) -> Result<Response, ApiError> {
    let interaction_id = interaction_id(headers, request_id)?;
    let snapshot = bot_workflow::execute_recruitment_command(
        database(state, request_id)?,
        guild_id,
        &interaction_id,
        request_id,
        command,
    )
    .await
    .map_err(|error| command_error(error, request_id))?;
    Ok(error::success(success_status, request_id, snapshot))
}

fn actor(request: ActorRequest, request_id: &RequestId) -> Result<BotActor, ApiError> {
    validate_text(
        &request.global_display_name,
        100,
        "actor.globalDisplayName",
        request_id,
    )?;
    validate_text(&request.display_name, 100, "actor.displayName", request_id)?;
    if request
        .avatar_hash
        .as_ref()
        .is_some_and(|hash| hash.len() > 128)
    {
        return Err(invalid_request("actor.avatarHash", request_id));
    }
    if request.role_ids.len() > 250 {
        return Err(invalid_request("actor.roleIds", request_id));
    }
    Ok(BotActor {
        id: request.id,
        global_display_name: request.global_display_name.trim().to_owned(),
        display_name: request.display_name.trim().to_owned(),
        avatar_hash: request.avatar_hash,
        role_ids: request.role_ids,
    })
}

fn validate_text(
    value: &str,
    maximum: usize,
    field: &'static str,
    request_id: &RequestId,
) -> Result<(), ApiError> {
    if value.trim().is_empty() || value.chars().count() > maximum {
        Err(invalid_request(field, request_id))
    } else {
        Ok(())
    }
}

fn invalid_request(field: &'static str, request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        request_id,
        "invalid_bot_command",
        "Bot command 요청 형식이 올바르지 않습니다.",
        Some(serde_json::json!({ "field": field })),
    )
}

fn interaction_id(
    headers: &HeaderMap,
    request_id: &RequestId,
) -> Result<DiscordInteractionId, ApiError> {
    let mut values = headers.get_all(IDEMPOTENCY_KEY).iter();
    let value = values
        .next()
        .and_then(|value| value.to_str().ok())
        .filter(|_| values.next().is_none())
        .ok_or_else(|| invalid_idempotency_key(request_id))?;
    DiscordInteractionId::parse(value.to_owned()).map_err(|_| invalid_idempotency_key(request_id))
}

fn invalid_idempotency_key(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        request_id,
        "invalid_idempotency_key",
        "Idempotency-Key는 Discord interaction ID여야 합니다.",
        None,
    )
}

fn has_duplicate_lanes(lanes: &[Lane]) -> bool {
    lanes
        .iter()
        .enumerate()
        .any(|(index, lane)| lanes[..index].contains(lane))
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

fn command_error(error: BotWorkflowError, request_id: &RequestId) -> ApiError {
    match &error {
        BotWorkflowError::Database(_)
        | BotWorkflowError::Serialization(_)
        | BotWorkflowError::InvariantViolation => {
            tracing::error!(%error, "Bot workflow command failed");
        }
        _ => tracing::warn!(%error, "Bot workflow command rejected"),
    }
    match error {
        BotWorkflowError::InvalidCommand(_) => ApiError::new(
            StatusCode::BAD_REQUEST,
            request_id,
            "invalid_bot_command",
            "Bot command 요청 형식이 올바르지 않습니다.",
            None,
        ),
        BotWorkflowError::GuildNotFound | BotWorkflowError::RecruitmentNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_id,
            "not_found",
            "요청한 대상을 찾을 수 없습니다.",
            None,
        ),
        BotWorkflowError::GuildInactive(status) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "guild_inactive",
            "현재 길드에서는 command를 실행할 수 없습니다.",
            Some(serde_json::json!({ "currentStatus": status.as_str() })),
        ),
        BotWorkflowError::GuildAlreadyConfigured => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "guild_already_configured",
            "이미 초기 설정이 완료된 길드입니다. 길드 관리 화면에서 설정을 변경해 주세요.",
            None,
        ),
        BotWorkflowError::GuildSetupUnavailable(status) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "guild_setup_unavailable",
            "비활성화되었거나 연결이 해제된 길드는 초기 설정을 실행할 수 없습니다.",
            Some(serde_json::json!({ "currentStatus": status.as_str() })),
        ),
        BotWorkflowError::GuildNotConfigured | BotWorkflowError::NoActiveSeason => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "guild_setup_required",
            "길드 초기 설정을 먼저 완료해야 합니다.",
            None,
        ),
        BotWorkflowError::OperatorRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "operator_required",
            "길드 운영자 권한이 필요합니다.",
            None,
        ),
        BotWorkflowError::MemberInactive => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "member_inactive",
            "비활성화된 길드 멤버는 이 작업을 수행할 수 없습니다.",
            None,
        ),
        BotWorkflowError::RiotAccountAlreadyLinked => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "riot_account_already_linked",
            "이 Riot 계정은 이미 다른 사용자에게 연결되어 있습니다.",
            None,
        ),
        BotWorkflowError::RiotIdentityConflict => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "riot_id_conflict",
            "동일한 Riot ID가 다른 계정에 연결되어 있습니다.",
            None,
        ),
        BotWorkflowError::UserIdentityConflict => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "user_identity_conflict",
            "이 Discord 계정 식별자는 다른 사용자 유형에서 이미 사용 중입니다.",
            None,
        ),
        BotWorkflowError::RecruitmentNotOpen(status)
        | BotWorkflowError::RecruitmentNotBindable(status) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "invalid_recruitment_state",
            "현재 모집 상태에서는 이 작업을 수행할 수 없습니다.",
            Some(serde_json::json!({ "currentStatus": status.as_str() })),
        ),
        BotWorkflowError::RecruitmentFull => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "recruitment_full",
            "모집 정원이 가득 찼습니다.",
            None,
        ),
        BotWorkflowError::ParticipantAlreadyJoined => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "already_participant",
            "이미 참가 중인 사용자입니다.",
            None,
        ),
        BotWorkflowError::ParticipantNotJoined => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "not_participant",
            "현재 모집 참가자가 아닙니다.",
            None,
        ),
        BotWorkflowError::InvalidPreferredLanes => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "invalid_preferred_lanes",
            "선호 라인은 중복 없이 1개 이상 선택해야 합니다.",
            None,
        ),
        BotWorkflowError::CloseRequiresFullRoster { joined, target } => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "recruitment_not_full",
            "정원을 채운 뒤 엔트리 편성을 시작할 수 있습니다.",
            Some(serde_json::json!({ "joinedCount": joined, "targetCount": target })),
        ),
        BotWorkflowError::IdempotencyConflict => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_conflict",
            "같은 interaction ID가 다른 요청에 사용되었습니다.",
            None,
        ),
        BotWorkflowError::IdempotencyInProgress => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_in_progress",
            "같은 interaction 요청을 처리하고 있습니다.",
            None,
        ),
        BotWorkflowError::Database(source) => error::database_failure(&source, request_id),
        BotWorkflowError::DatabaseBusy => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            request_id,
            "database_busy",
            "데이터베이스가 사용 중입니다. 잠시 후 다시 시도해 주세요.",
            None,
        ),
        BotWorkflowError::Serialization(_) | BotWorkflowError::InvariantViolation => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            request_id,
            "internal_error",
            "요청을 처리하는 중 내부 오류가 발생했습니다.",
            None,
        ),
    }
}

fn registration_command_error(error: BotWorkflowError, request_id: &RequestId) -> ApiError {
    match error {
        BotWorkflowError::GuildNotFound | BotWorkflowError::GuildNotConfigured => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "guild_setup_required",
            "길드 초기 설정을 먼저 완료해야 합니다.",
            None,
        ),
        error => command_error(error, request_id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_validation_counts_unicode_characters_instead_of_utf8_bytes() {
        let request_id = RequestId("test-request".to_owned());

        assert!(validate_text(&"가".repeat(100), 100, "displayName", &request_id).is_ok());
        assert!(validate_text(&"가".repeat(101), 100, "displayName", &request_id).is_err());
    }
}
