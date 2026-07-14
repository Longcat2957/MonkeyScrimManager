use axum::{
    Extension, Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::Response,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    domain::{
        DiscordChannelId, DiscordInteractionId, DiscordMessageId, DiscordRoleId, DiscordUserId,
        GuildId, Lane, RecruitmentId, SeriesId, UserId,
    },
    error::{self, ApiError},
    http::extractors::{
        AuthenticatedSession, BotServicePrincipal, RecruitmentCreatePrincipal, require_csrf,
    },
    repository::bot_workflow::{
        self, BotActor, BotRecruitmentCommand, BotWorkflowError, EntryDraftOperation,
        RecruitmentSeriesRules, RegisterGuildMemberCommand, RegisterGuildMemberInput,
        SeriesEarlyCompleteCommand, SeriesGameRevertCommand, SeriesGameSubmitCommand,
        SeriesPickBanOperation, SetupGuildCommand,
    },
    state::{AppState, RequestId},
};

const IDEMPOTENCY_KEY: &str = "idempotency-key";
const MAX_BROWSER_IDEMPOTENCY_KEY_BYTES: usize = 128;

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
    recruitment_channel_id: DiscordChannelId,
    entry_channel_id: Option<DiscordChannelId>,
    result_channel_id: Option<DiscordChannelId>,
    log_channel_id: Option<DiscordChannelId>,
    season_name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActorOnlyRequest {
    actor: ActorRequest,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BrowserCreateRecruitmentRequest {
    best_of: Option<i16>,
    hard_fearless: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BotCreateRecruitmentRequest {
    actor: ActorRequest,
    best_of: Option<i16>,
    hard_fearless: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BrowserAddRecruitmentMemberRequest {
    member_id: UserId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BrowserRemoveRecruitmentMemberRequest {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BrowserCloseRecruitmentRequest {}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BrowserEntryDraftMutationRequest {
    expected_revision: i64,
    operation: EntryDraftOperation,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BrowserEntryDraftRevisionRequest {
    expected_revision: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BrowserSeriesCreateRequest {
    source_recruitment_id: RecruitmentId,
    expected_revision: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BrowserSeriesLifecycleRequest {
    expected_revision: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BrowserSeriesPickBanRequest {
    expected_revision: i64,
    operation: SeriesPickBanOperation,
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
            recruitment_channel_id: request.recruitment_channel_id,
            entry_channel_id: request.entry_channel_id,
            result_channel_id: request.result_channel_id,
            log_channel_id: request.log_channel_id,
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
    principal: RecruitmentCreatePrincipal,
    Json(request): Json<Value>,
) -> Result<Response, ApiError> {
    match principal {
        RecruitmentCreatePrincipal::Bot(_) => {
            let request = serde_json::from_value::<BotCreateRecruitmentRequest>(request)
                .map_err(|_| invalid_request("body", &request_id))?;
            recruitment_command(
                &state,
                &request_id,
                &headers,
                &guild_id,
                StatusCode::CREATED,
                BotRecruitmentCommand::Create {
                    actor: actor(request.actor, &request_id)?,
                    rules: RecruitmentSeriesRules::from_optional(
                        request.best_of,
                        request.hard_fearless,
                    ),
                },
            )
            .await
        }
        RecruitmentCreatePrincipal::Session(principal) => {
            let request = serde_json::from_value::<BrowserCreateRecruitmentRequest>(request)
                .map_err(|_| invalid_browser_create_request(&request_id))?;
            require_csrf(&headers, &state, &principal, &request_id)?;
            let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
            let snapshot = bot_workflow::create_web_recruitment(
                database(&state, &request_id)?,
                &guild_id,
                &principal.session.user_id,
                RecruitmentSeriesRules::from_optional(request.best_of, request.hard_fearless),
                &idempotency_key,
                &request_id,
            )
            .await
            .map_err(|error| web_create_error(error, &request_id))?;
            Ok(error::success(StatusCode::CREATED, &request_id, snapshot))
        }
    }
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
    principal: RecruitmentCreatePrincipal,
    Json(request): Json<Value>,
) -> Result<Response, ApiError> {
    match principal {
        RecruitmentCreatePrincipal::Bot(_) => {
            let request = serde_json::from_value::<JoinMemberRequest>(request)
                .map_err(|_| invalid_request("body", &request_id))?;
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
        RecruitmentCreatePrincipal::Session(principal) => {
            let request = serde_json::from_value::<BrowserAddRecruitmentMemberRequest>(request)
                .map_err(|_| invalid_browser_recruitment_member_request(&request_id))?;
            require_csrf(&headers, &state, &principal, &request_id)?;
            let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
            let snapshot = bot_workflow::add_web_recruitment_member(
                database(&state, &request_id)?,
                &guild_id,
                recruitment_id,
                &principal.session.user_id,
                request.member_id,
                &idempotency_key,
                &request_id,
            )
            .await
            .map_err(|error| web_recruitment_mutation_error(error, &request_id))?;
            Ok(error::success(StatusCode::OK, &request_id, snapshot))
        }
    }
}

pub async fn cancel_recruitment(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, recruitment_id)): Path<(GuildId, RecruitmentId)>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
) -> Result<Response, ApiError> {
    require_csrf(&headers, &state, &principal, &request_id)?;
    let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
    let snapshot = bot_workflow::cancel_web_recruitment(
        database(&state, &request_id)?,
        &guild_id,
        recruitment_id,
        &principal.session.user_id,
        &idempotency_key,
        &request_id,
    )
    .await
    .map_err(|error| web_recruitment_mutation_error(error, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, snapshot))
}

pub async fn leave_recruitment(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, recruitment_id, user_id)): Path<(GuildId, RecruitmentId, UserId)>,
    headers: HeaderMap,
    principal: RecruitmentCreatePrincipal,
    Json(request): Json<Value>,
) -> Result<Response, ApiError> {
    match principal {
        RecruitmentCreatePrincipal::Bot(_) => {
            let request = serde_json::from_value::<ActorOnlyRequest>(request)
                .map_err(|_| invalid_request("body", &request_id))?;
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
        RecruitmentCreatePrincipal::Session(principal) => {
            serde_json::from_value::<BrowserRemoveRecruitmentMemberRequest>(request)
                .map_err(|_| invalid_browser_recruitment_member_request(&request_id))?;
            require_csrf(&headers, &state, &principal, &request_id)?;
            let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
            let snapshot = bot_workflow::remove_web_recruitment_member(
                database(&state, &request_id)?,
                &guild_id,
                recruitment_id,
                &principal.session.user_id,
                user_id,
                &idempotency_key,
                &request_id,
            )
            .await
            .map_err(|error| web_recruitment_mutation_error(error, &request_id))?;
            Ok(error::success(StatusCode::OK, &request_id, snapshot))
        }
    }
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
    principal: RecruitmentCreatePrincipal,
    Json(request): Json<Value>,
) -> Result<Response, ApiError> {
    match principal {
        RecruitmentCreatePrincipal::Bot(_) => {
            let request = serde_json::from_value::<ActorOnlyRequest>(request)
                .map_err(|_| invalid_request("body", &request_id))?;
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
        RecruitmentCreatePrincipal::Session(principal) => {
            serde_json::from_value::<BrowserCloseRecruitmentRequest>(request)
                .map_err(|_| invalid_browser_recruitment_close_request(&request_id))?;
            require_csrf(&headers, &state, &principal, &request_id)?;
            let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
            let snapshot = bot_workflow::close_web_recruitment(
                database(&state, &request_id)?,
                &guild_id,
                recruitment_id,
                &principal.session.user_id,
                &idempotency_key,
                &request_id,
            )
            .await
            .map_err(|error| web_recruitment_mutation_error(error, &request_id))?;
            Ok(error::success(StatusCode::OK, &request_id, snapshot))
        }
    }
}

pub async fn create_series(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(guild_id): Path<GuildId>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(request): Json<BrowserSeriesCreateRequest>,
) -> Result<Response, ApiError> {
    if request.expected_revision < 0 {
        return Err(invalid_browser_series_create_request(&request_id));
    }
    require_csrf(&headers, &state, &principal, &request_id)?;
    let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
    let result = bot_workflow::create_web_series(
        database(&state, &request_id)?,
        &guild_id,
        &principal.session.user_id,
        request.source_recruitment_id,
        request.expected_revision,
        &idempotency_key,
        &request_id,
    )
    .await
    .map_err(|error| web_series_create_error(error, &request_id))?;
    Ok(error::success(StatusCode::CREATED, &request_id, result))
}

pub async fn revert_series(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, series_id)): Path<(GuildId, SeriesId)>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(request): Json<BrowserSeriesLifecycleRequest>,
) -> Result<Response, ApiError> {
    if request.expected_revision < 0 {
        return Err(invalid_browser_series_lifecycle_request(&request_id));
    }
    require_csrf(&headers, &state, &principal, &request_id)?;
    let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
    let result = bot_workflow::revert_web_series(
        database(&state, &request_id)?,
        &guild_id,
        series_id,
        &principal.session.user_id,
        request.expected_revision,
        &idempotency_key,
        &request_id,
    )
    .await
    .map_err(|error| web_series_lifecycle_error(error, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, result))
}

pub async fn delete_series(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, series_id)): Path<(GuildId, SeriesId)>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(request): Json<BrowserSeriesLifecycleRequest>,
) -> Result<Response, ApiError> {
    if request.expected_revision < 0 {
        return Err(invalid_browser_series_lifecycle_request(&request_id));
    }
    require_csrf(&headers, &state, &principal, &request_id)?;
    let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
    let result = bot_workflow::delete_web_series(
        database(&state, &request_id)?,
        &guild_id,
        series_id,
        &principal.session.user_id,
        request.expected_revision,
        &idempotency_key,
        &request_id,
    )
    .await
    .map_err(|error| web_series_lifecycle_error(error, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, result))
}

pub async fn update_series_pick_ban(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, series_id)): Path<(GuildId, SeriesId)>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(request): Json<BrowserSeriesPickBanRequest>,
) -> Result<Response, ApiError> {
    if request.expected_revision < 0 {
        return Err(invalid_browser_series_pick_ban_request(&request_id));
    }
    require_csrf(&headers, &state, &principal, &request_id)?;
    let snapshot = bot_workflow::update_web_series_pick_ban(
        database(&state, &request_id)?,
        &guild_id,
        series_id,
        &principal.session.user_id,
        request.expected_revision,
        request.operation,
        &request_id,
    )
    .await
    .map_err(|error| web_series_pick_ban_error(error, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, snapshot))
}

pub async fn submit_series_game(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, series_id)): Path<(GuildId, SeriesId)>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(request): Json<SeriesGameSubmitCommand>,
) -> Result<Response, ApiError> {
    if request.expected_revision < 0 {
        return Err(invalid_browser_series_game_request(&request_id));
    }
    require_csrf(&headers, &state, &principal, &request_id)?;
    let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
    let result = bot_workflow::submit_web_series_game(
        database(&state, &request_id)?,
        &guild_id,
        series_id,
        &principal.session.user_id,
        &idempotency_key,
        &request_id,
        request,
    )
    .await
    .map_err(|error| web_series_game_error(error, &request_id))?;
    Ok(error::success(StatusCode::CREATED, &request_id, result))
}

pub async fn revert_last_series_game(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, series_id)): Path<(GuildId, SeriesId)>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(request): Json<SeriesGameRevertCommand>,
) -> Result<Response, ApiError> {
    if request.expected_revision < 0 {
        return Err(invalid_browser_series_game_request(&request_id));
    }
    require_csrf(&headers, &state, &principal, &request_id)?;
    let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
    let result = bot_workflow::revert_last_web_series_game(
        database(&state, &request_id)?,
        &guild_id,
        series_id,
        &principal.session.user_id,
        &idempotency_key,
        &request_id,
        request,
    )
    .await
    .map_err(|error| web_series_game_error(error, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, result))
}

pub async fn complete_series_early(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, series_id)): Path<(GuildId, SeriesId)>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(request): Json<SeriesEarlyCompleteCommand>,
) -> Result<Response, ApiError> {
    if request.expected_revision < 0 {
        return Err(invalid_browser_series_early_complete_request(&request_id));
    }
    require_csrf(&headers, &state, &principal, &request_id)?;
    let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
    let result = bot_workflow::complete_web_series_early(
        database(&state, &request_id)?,
        &guild_id,
        series_id,
        &principal.session.user_id,
        &idempotency_key,
        &request_id,
        request,
    )
    .await
    .map_err(|error| web_series_early_complete_error(error, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, result))
}

pub async fn update_entry_draft(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, recruitment_id)): Path<(GuildId, RecruitmentId)>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(request): Json<BrowserEntryDraftMutationRequest>,
) -> Result<Response, ApiError> {
    if request.expected_revision < 0 {
        return Err(invalid_browser_entry_draft_request(&request_id));
    }
    require_csrf(&headers, &state, &principal, &request_id)?;
    let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
    let snapshot = bot_workflow::update_web_entry_draft(
        database(&state, &request_id)?,
        &guild_id,
        recruitment_id,
        &principal.session.user_id,
        request.expected_revision,
        request.operation,
        &idempotency_key,
        &request_id,
    )
    .await
    .map_err(|error| web_entry_draft_mutation_error(error, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, snapshot))
}

pub async fn reset_entry_draft(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, recruitment_id)): Path<(GuildId, RecruitmentId)>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(request): Json<BrowserEntryDraftRevisionRequest>,
) -> Result<Response, ApiError> {
    if request.expected_revision < 0 {
        return Err(invalid_browser_entry_draft_request(&request_id));
    }
    require_csrf(&headers, &state, &principal, &request_id)?;
    let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
    let snapshot = bot_workflow::reset_web_entry_draft(
        database(&state, &request_id)?,
        &guild_id,
        recruitment_id,
        &principal.session.user_id,
        request.expected_revision,
        &idempotency_key,
        &request_id,
    )
    .await
    .map_err(|error| web_entry_draft_mutation_error(error, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, snapshot))
}

pub async fn reopen_recruitment(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, recruitment_id)): Path<(GuildId, RecruitmentId)>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(request): Json<BrowserEntryDraftRevisionRequest>,
) -> Result<Response, ApiError> {
    if request.expected_revision < 0 {
        return Err(invalid_browser_entry_draft_request(&request_id));
    }
    require_csrf(&headers, &state, &principal, &request_id)?;
    let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
    let snapshot = bot_workflow::reopen_web_recruitment(
        database(&state, &request_id)?,
        &guild_id,
        recruitment_id,
        &principal.session.user_id,
        request.expected_revision,
        &idempotency_key,
        &request_id,
    )
    .await
    .map_err(|error| web_entry_draft_mutation_error(error, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, snapshot))
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

fn browser_idempotency_key(
    headers: &HeaderMap,
    request_id: &RequestId,
) -> Result<String, ApiError> {
    let mut values = headers.get_all(IDEMPOTENCY_KEY).iter();
    let value = values
        .next()
        .and_then(|value| value.to_str().ok())
        .filter(|value| {
            values.next().is_none()
                && !value.is_empty()
                && value.len() <= MAX_BROWSER_IDEMPOTENCY_KEY_BYTES
                && value.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
        })
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                request_id,
                "invalid_idempotency_key",
                "Idempotency-Key 헤더가 필요합니다.",
                Some(serde_json::json!({ "maxLength": MAX_BROWSER_IDEMPOTENCY_KEY_BYTES })),
            )
        })?;
    Ok(value.to_owned())
}

fn invalid_browser_create_request(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        request_id,
        "invalid_recruitment_create_request",
        "내전 생성 요청 형식이 올바르지 않습니다.",
        None,
    )
}

fn invalid_recruitment_best_of(request_id: &RequestId, received: i16) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        request_id,
        "invalid_recruitment_best_of",
        "시리즈 형식은 BO1, BO3 또는 BO5만 선택할 수 있습니다.",
        Some(serde_json::json!({
            "field": "bestOf",
            "allowedValues": [1, 3, 5],
            "receivedValue": received,
        })),
    )
}

fn invalid_browser_recruitment_member_request(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        request_id,
        "invalid_recruitment_member_request",
        "모집 참가자 변경 요청 형식이 올바르지 않습니다.",
        None,
    )
}

fn invalid_browser_recruitment_close_request(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        request_id,
        "invalid_recruitment_close_request",
        "엔트리 편성 시작 요청 형식이 올바르지 않습니다.",
        None,
    )
}

fn invalid_browser_entry_draft_request(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        request_id,
        "invalid_entry_draft_request",
        "엔트리 편성 요청 형식이 올바르지 않습니다.",
        None,
    )
}

fn invalid_browser_series_create_request(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        request_id,
        "invalid_series_create_request",
        "시리즈 시작 요청 형식이 올바르지 않습니다.",
        None,
    )
}

fn invalid_browser_series_lifecycle_request(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        request_id,
        "invalid_series_lifecycle_request",
        "시리즈 상태 변경 요청 형식이 올바르지 않습니다.",
        None,
    )
}

fn invalid_browser_series_pick_ban_request(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        request_id,
        "invalid_series_pickban_request",
        "픽밴 변경 요청 형식이 올바르지 않습니다.",
        None,
    )
}

fn invalid_browser_series_game_request(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        request_id,
        "invalid_series_game_request",
        "게임 결과 요청 형식이 올바르지 않습니다.",
        None,
    )
}

fn invalid_browser_series_early_complete_request(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        request_id,
        "invalid_series_early_complete_request",
        "시리즈 조기 종료 요청 형식이 올바르지 않습니다.",
        None,
    )
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
        BotWorkflowError::InvalidRecruitmentBestOf { received } => {
            invalid_recruitment_best_of(request_id, received)
        }
        BotWorkflowError::InvalidCommand(_) => ApiError::new(
            StatusCode::BAD_REQUEST,
            request_id,
            "invalid_bot_command",
            "Bot command 요청 형식이 올바르지 않습니다.",
            None,
        ),
        BotWorkflowError::GuildNotFound
        | BotWorkflowError::RecruitmentNotFound
        | BotWorkflowError::SeriesNotFound => ApiError::new(
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
        BotWorkflowError::RecruitmentChannelNotConfigured => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "recruitment_channel_not_configured",
            "Discord 모집 채널을 먼저 설정해야 내전을 만들 수 있습니다.",
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
        BotWorkflowError::RecruitmentMemberNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_id,
            "guild_member_not_found",
            "길드에 소속된 사용자를 찾을 수 없습니다.",
            None,
        ),
        BotWorkflowError::RecruitmentMemberInactive => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "guild_member_inactive",
            "비활성화된 길드 멤버는 모집에 추가할 수 없습니다.",
            None,
        ),
        BotWorkflowError::MembershipRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "guild_membership_required",
            "해당 길드의 활성 멤버만 이 작업을 수행할 수 있습니다.",
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
        BotWorkflowError::EntryDraftNotEditable(status) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "entry_draft_not_editable",
            "현재 모집 상태에서는 엔트리 편성을 수정할 수 없습니다.",
            Some(serde_json::json!({ "currentStatus": status.as_str() })),
        ),
        BotWorkflowError::EntryDraftRevisionConflict { expected, actual } => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "entry_draft_revision_conflict",
            "다른 운영자가 엔트리 편성을 변경했습니다. 최신 상태를 확인해 주세요.",
            Some(serde_json::json!({ "expectedRevision": expected, "actualRevision": actual })),
        ),
        BotWorkflowError::RecruitmentRevisionConflict { expected, actual } => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "recruitment_revision_conflict",
            "모집 상태가 변경되었습니다. 최신 상태를 확인해 주세요.",
            Some(serde_json::json!({ "expectedRevision": expected, "actualRevision": actual })),
        ),
        BotWorkflowError::SeriesSourceNotReady(status) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "series_source_not_ready",
            "모집 마감 후 엔트리 편성이 완료된 상태에서만 시리즈를 시작할 수 있습니다.",
            Some(serde_json::json!({ "currentStatus": status.as_str() })),
        ),
        BotWorkflowError::SeriesNotInProgress(status) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "series_not_in_progress",
            "진행 중인 시리즈에서만 이 작업을 수행할 수 있습니다.",
            Some(serde_json::json!({ "currentStatus": status.as_str() })),
        ),
        BotWorkflowError::SeriesRevisionConflict { expected, actual } => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "series_revision_conflict",
            "시리즈 상태가 변경되었습니다. 최신 상태를 확인해 주세요.",
            Some(serde_json::json!({ "expectedRevision": expected, "actualRevision": actual })),
        ),
        BotWorkflowError::SeriesSourceNotConverted(status) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "series_source_not_converted",
            "원본 모집 상태가 변경되어 시리즈 상태를 변경할 수 없습니다.",
            Some(serde_json::json!({ "currentStatus": status.as_str() })),
        ),
        BotWorkflowError::SeriesNoCurrentSet => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "series_no_current_set",
            "더 기록할 세트가 없습니다.",
            None,
        ),
        BotWorkflowError::SeriesSetNotCurrent { expected, actual } => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "series_set_not_current",
            "현재 진행 중인 세트만 기록할 수 있습니다. 최신 상태를 확인해 주세요.",
            Some(serde_json::json!({ "expectedSetNumber": expected, "actualSetNumber": actual })),
        ),
        BotWorkflowError::InvalidGameResult => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "invalid_series_game_result",
            "게임 결과의 승리 팀, 픽, 밴 또는 패치 버전 값이 올바르지 않습니다.",
            None,
        ),
        BotWorkflowError::SeriesGameChampionDuplicate { champion_id } => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "duplicate_series_game_champion",
            "같은 세트에서 픽과 밴은 같은 챔피언을 중복할 수 없습니다.",
            Some(serde_json::json!({ "championId": champion_id })),
        ),
        BotWorkflowError::SeriesHardFearlessChampionUsed { champion_id } => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "hard_fearless_champion_used",
            "하드 피어리스에서는 이전 세트에서 픽한 챔피언을 다시 픽할 수 없습니다.",
            Some(serde_json::json!({ "championId": champion_id })),
        ),
        BotWorkflowError::SeriesPickBanPlayerNotInRoster => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "series_pickban_player_not_in_roster",
            "고정된 시리즈 참가자만 픽을 지정할 수 있습니다.",
            None,
        ),
        BotWorkflowError::SeriesNoConfirmedGames => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "series_no_confirmed_games",
            "되돌릴 확정 게임이 없습니다.",
            None,
        ),
        BotWorkflowError::SeriesEarlyCompletionUnavailable {
            best_of,
            confirmed_game_count,
        } => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "series_early_completion_unavailable",
            "BO3 또는 BO5에서 1경기 이상 확정한 뒤에만 조기 종료할 수 있습니다.",
            Some(serde_json::json!({
                "bestOf": best_of,
                "confirmedGameCount": confirmed_game_count,
            })),
        ),
        BotWorkflowError::SeriesEarlyDrawRequiresTiedScore {
            team_1_score,
            team_2_score,
        } => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "early_draw_requires_tied_score",
            "무승부 조기 종료는 현재 스코어가 동률일 때만 가능합니다.",
            Some(serde_json::json!({
                "team1Score": team_1_score,
                "team2Score": team_2_score,
            })),
        ),
        BotWorkflowError::SeriesGameMmrHistoryAdvanced => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "series_game_reversal_not_safe",
            "이후 MMR 변경이 있어 게임 결과를 안전하게 되돌릴 수 없습니다.",
            None,
        ),
        BotWorkflowError::SeriesResultUnavailable(status) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "series_result_unavailable",
            "완료된 시리즈 결과만 확인할 수 있습니다.",
            Some(serde_json::json!({ "currentStatus": status.as_str() })),
        ),
        BotWorkflowError::EntryDraftIncomplete {
            joined,
            assigned,
            target,
        } => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "entry_draft_incomplete",
            "참가자 10명을 양 팀의 모든 라인에 배치한 뒤 시리즈를 시작할 수 있습니다.",
            Some(serde_json::json!({
                "joinedCount": joined,
                "assignedCount": assigned,
                "targetCount": target,
            })),
        ),
        BotWorkflowError::InvalidEntrySlot => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "invalid_entry_slot",
            "엔트리 배치 위치가 올바르지 않습니다.",
            None,
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

fn web_create_error(error: BotWorkflowError, request_id: &RequestId) -> ApiError {
    match error {
        BotWorkflowError::InvalidCommand(_) => invalid_browser_create_request(request_id),
        BotWorkflowError::MembershipRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "guild_membership_required",
            "해당 길드의 활성 멤버만 내전을 만들 수 있습니다.",
            None,
        ),
        BotWorkflowError::NoActiveSeason => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "no_active_season",
            "길드에 진행 중인 시즌이 없어 내전을 만들 수 없습니다.",
            None,
        ),
        BotWorkflowError::IdempotencyConflict => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_conflict",
            "같은 Idempotency-Key가 다른 요청에 사용되었습니다.",
            None,
        ),
        BotWorkflowError::IdempotencyInProgress => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_in_progress",
            "같은 내전 생성 요청을 처리하고 있습니다.",
            None,
        ),
        error => command_error(error, request_id),
    }
}

fn web_recruitment_mutation_error(error: BotWorkflowError, request_id: &RequestId) -> ApiError {
    match error {
        BotWorkflowError::MembershipRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "guild_membership_required",
            "해당 길드의 활성 멤버만 모집을 변경할 수 있습니다.",
            None,
        ),
        BotWorkflowError::IdempotencyConflict => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_conflict",
            "같은 Idempotency-Key가 다른 요청에 사용되었습니다.",
            None,
        ),
        BotWorkflowError::IdempotencyInProgress => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_in_progress",
            "같은 모집 변경 요청을 처리하고 있습니다.",
            None,
        ),
        error => command_error(error, request_id),
    }
}

fn web_entry_draft_mutation_error(error: BotWorkflowError, request_id: &RequestId) -> ApiError {
    match error {
        BotWorkflowError::MembershipRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "guild_membership_required",
            "해당 길드의 활성 멤버만 엔트리 편성을 변경할 수 있습니다.",
            None,
        ),
        BotWorkflowError::IdempotencyConflict => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_conflict",
            "같은 Idempotency-Key가 다른 요청에 사용되었습니다.",
            None,
        ),
        BotWorkflowError::IdempotencyInProgress => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_in_progress",
            "같은 엔트리 편성 요청을 처리하고 있습니다.",
            None,
        ),
        error => command_error(error, request_id),
    }
}

fn web_series_create_error(error: BotWorkflowError, request_id: &RequestId) -> ApiError {
    match error {
        BotWorkflowError::RecruitmentRevisionConflict { expected, actual } => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "recruitment_revision_conflict",
            "모집 상태가 변경되었습니다. 최신 상태를 확인해 주세요.",
            Some(serde_json::json!({ "expectedRevision": expected, "actualRevision": actual })),
        ),
        BotWorkflowError::SeriesSourceNotReady(status) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "series_source_not_ready",
            "모집 마감 후 엔트리 편성이 완료된 상태에서만 시리즈를 시작할 수 있습니다.",
            Some(serde_json::json!({ "currentStatus": status.as_str() })),
        ),
        BotWorkflowError::EntryDraftIncomplete {
            joined,
            assigned,
            target,
        } => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "entry_draft_incomplete",
            "참가자 10명을 양 팀의 모든 라인에 배치한 뒤 시리즈를 시작할 수 있습니다.",
            Some(serde_json::json!({
                "joinedCount": joined,
                "assignedCount": assigned,
                "targetCount": target,
            })),
        ),
        BotWorkflowError::IdempotencyConflict => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_conflict",
            "같은 Idempotency-Key가 다른 요청에 사용되었습니다.",
            None,
        ),
        BotWorkflowError::IdempotencyInProgress => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_in_progress",
            "같은 시리즈 시작 요청을 처리하고 있습니다.",
            None,
        ),
        error => command_error(error, request_id),
    }
}

fn web_series_lifecycle_error(error: BotWorkflowError, request_id: &RequestId) -> ApiError {
    match error {
        BotWorkflowError::SeriesRevisionConflict { expected, actual } => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "series_revision_conflict",
            "시리즈 상태가 변경되었습니다. 최신 상태를 확인해 주세요.",
            Some(serde_json::json!({ "expectedRevision": expected, "actualRevision": actual })),
        ),
        BotWorkflowError::SeriesNotInProgress(status) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "series_not_in_progress",
            "진행 중인 시리즈에서만 이 작업을 수행할 수 있습니다.",
            Some(serde_json::json!({ "currentStatus": status.as_str() })),
        ),
        BotWorkflowError::SeriesSourceNotConverted(status) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "series_source_not_converted",
            "원본 모집 상태가 변경되어 시리즈 상태를 변경할 수 없습니다.",
            Some(serde_json::json!({ "currentStatus": status.as_str() })),
        ),
        BotWorkflowError::MembershipRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "guild_membership_required",
            "해당 길드의 활성 멤버만 시리즈 상태를 변경할 수 있습니다.",
            None,
        ),
        BotWorkflowError::IdempotencyConflict => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_conflict",
            "같은 Idempotency-Key가 다른 요청에 사용되었습니다.",
            None,
        ),
        BotWorkflowError::IdempotencyInProgress => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_in_progress",
            "같은 시리즈 상태 변경 요청을 처리하고 있습니다.",
            None,
        ),
        error => command_error(error, request_id),
    }
}

fn web_series_pick_ban_error(error: BotWorkflowError, request_id: &RequestId) -> ApiError {
    match error {
        BotWorkflowError::InvalidCommand(_) => invalid_browser_series_pick_ban_request(request_id),
        BotWorkflowError::MembershipRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "guild_membership_required",
            "해당 길드의 활성 멤버만 픽밴 상태를 확인할 수 있습니다.",
            None,
        ),
        error => command_error(error, request_id),
    }
}

fn web_series_game_error(error: BotWorkflowError, request_id: &RequestId) -> ApiError {
    match error {
        BotWorkflowError::InvalidCommand(_) => invalid_browser_series_game_request(request_id),
        BotWorkflowError::IdempotencyConflict => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_conflict",
            "같은 Idempotency-Key가 다른 게임 결과 요청에 사용되었습니다.",
            None,
        ),
        BotWorkflowError::IdempotencyInProgress => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_in_progress",
            "같은 게임 결과 요청을 처리하고 있습니다.",
            None,
        ),
        error => command_error(error, request_id),
    }
}

fn web_series_early_complete_error(error: BotWorkflowError, request_id: &RequestId) -> ApiError {
    match error {
        BotWorkflowError::InvalidCommand(_) => {
            invalid_browser_series_early_complete_request(request_id)
        }
        BotWorkflowError::IdempotencyConflict => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_conflict",
            "같은 Idempotency-Key가 다른 시리즈 조기 종료 요청에 사용되었습니다.",
            None,
        ),
        BotWorkflowError::IdempotencyInProgress => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_in_progress",
            "같은 시리즈 조기 종료 요청을 처리하고 있습니다.",
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
