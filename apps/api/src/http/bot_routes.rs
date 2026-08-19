use axum::{
    Extension, Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::Response,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
    domain::{
        AuctionMatchId, AuctionRecruitmentId, AuctionTournamentId, DiscordChannelId,
        DiscordInteractionId, DiscordMessageId, DiscordRoleId, DiscordUserId, GuildId, Lane,
        RecruitmentId, SeriesId, UserId,
    },
    error::{self, ApiError},
    http::extractors::{
        AuthenticatedSession, BotServicePrincipal, RecruitmentCreatePrincipal, require_csrf,
    },
    repository::bot_workflow::{
        self, AuctionMatchGameRevertCommand, AuctionMatchGameSubmitCommand,
        AuctionMatchLineupAssignmentCommand, AuctionMatchLineupLockCommand,
        AuctionRecruitmentForceCancelCommand, AuctionRecruitmentSeriesRules,
        AuctionSetupDraftTeamInput, AuctionTournamentBracketCommand,
        AuctionTournamentCancelCommand, AuctionTournamentDraftCommand, BotActor,
        BotAuctionRecruitmentParticipationCommand, BotRecruitmentCommand, BotWorkflowError,
        EntryDraftOperation, RecruitmentSeriesRules, RegisterGuildMemberCommand,
        RegisterGuildMemberInput, SeriesEarlyCompleteCommand, SeriesGameRevertCommand,
        SeriesGameSubmitCommand, SeriesPickBanOperation, SetupGuildCommand,
    },
    repository::workflow::AuctionFormat,
    state::{AppState, RequestId},
};

const IDEMPOTENCY_KEY: &str = "idempotency-key";
const MAX_BROWSER_IDEMPOTENCY_KEY_BYTES: usize = 128;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ActorRequest {
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
    expected_start_at: Option<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BotCreateRecruitmentRequest {
    actor: ActorRequest,
    best_of: Option<i16>,
    hard_fearless: Option<bool>,
    expected_start_at: Option<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BrowserCreateAuctionRecruitmentRequest {
    format: AuctionFormat,
    semifinal_best_of: Option<i16>,
    final_best_of: Option<i16>,
    hard_fearless: Option<bool>,
    expected_start_at: Option<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BotCreateAuctionRecruitmentRequest {
    actor: ActorRequest,
    format: AuctionFormat,
    semifinal_best_of: Option<i16>,
    final_best_of: Option<i16>,
    hard_fearless: Option<bool>,
    expected_start_at: Option<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AuctionRecruitmentMemberRequest {
    actor: ActorRequest,
    member: Option<ActorRequest>,
    declared_lane: Option<Lane>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BrowserAddAuctionRecruitmentMemberRequest {
    member_id: UserId,
    declared_lane: Lane,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BrowserRemoveAuctionRecruitmentMemberRequest {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BrowserAuctionRecruitmentLifecycleRequest {}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BrowserAuctionRecruitmentReopenRequest {
    expected_revision: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BrowserParticipantConfirmationDmRequest {}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BrowserAuctionSetupDraftRequest {
    expected_revision: i64,
    teams: Vec<AuctionSetupDraftTeamInput>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BrowserAuctionTournamentCreateRequest {
    source_recruitment_id: AuctionRecruitmentId,
    expected_revision: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BotAuctionTournamentCancelRequest {
    actor: ActorRequest,
    expected_revision: i64,
    reason: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BrowserAuctionTournamentCancelRequest {
    expected_revision: i64,
    reason: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BrowserAuctionMatchLineupAssignmentRequest {
    expected_revision: i64,
    team_id: uuid::Uuid,
    lane: Lane,
    team_member_id: Option<uuid::Uuid>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BrowserAuctionMatchLineupLockRequest {
    expected_revision: i64,
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
pub(crate) struct BrowserEntryDraftAuctionConversionRequest {
    expected_revision: i64,
    expected_start_at: Option<Value>,
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
            let expected_start_at =
                parse_expected_start_at(request.expected_start_at.as_ref(), &request_id)?;
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
                    expected_start_at,
                },
            )
            .await
        }
        RecruitmentCreatePrincipal::Session(principal) => {
            let request = serde_json::from_value::<BrowserCreateRecruitmentRequest>(request)
                .map_err(|_| invalid_browser_create_request(&request_id))?;
            let expected_start_at =
                parse_expected_start_at(request.expected_start_at.as_ref(), &request_id)?;
            require_csrf(&headers, &state, &principal, &request_id)?;
            let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
            let snapshot = bot_workflow::create_web_recruitment(
                database(&state, &request_id)?,
                &guild_id,
                &principal.session.user_id,
                RecruitmentSeriesRules::from_optional(request.best_of, request.hard_fearless),
                expected_start_at,
                &idempotency_key,
                &request_id,
            )
            .await
            .map_err(|error| web_create_error(error, &request_id))?;
            Ok(error::success(StatusCode::CREATED, &request_id, snapshot))
        }
    }
}

pub async fn create_auction_recruitment(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(guild_id): Path<GuildId>,
    headers: HeaderMap,
    principal: RecruitmentCreatePrincipal,
    Json(request): Json<Value>,
) -> Result<Response, ApiError> {
    match principal {
        RecruitmentCreatePrincipal::Bot(_) => {
            let request = serde_json::from_value::<BotCreateAuctionRecruitmentRequest>(request)
                .map_err(|_| invalid_request("body", &request_id))?;
            let expected_start_at =
                parse_expected_start_at(request.expected_start_at.as_ref(), &request_id)?;
            let rules = AuctionRecruitmentSeriesRules::from_optional(
                request.format,
                request.semifinal_best_of,
                request.final_best_of,
                request.hard_fearless,
            );
            let interaction_id = interaction_id(&headers, &request_id)?;
            let snapshot = bot_workflow::execute_auction_recruitment_command(
                database(&state, &request_id)?,
                &guild_id,
                &interaction_id,
                &request_id,
                actor(request.actor, &request_id)?,
                request.format,
                rules,
                expected_start_at,
            )
            .await
            .map_err(|error| command_error(error, &request_id))?;
            Ok(error::success(StatusCode::CREATED, &request_id, snapshot))
        }
        RecruitmentCreatePrincipal::Session(principal) => {
            let request = serde_json::from_value::<BrowserCreateAuctionRecruitmentRequest>(request)
                .map_err(|_| invalid_browser_auction_recruitment_create_request(&request_id))?;
            let expected_start_at =
                parse_expected_start_at(request.expected_start_at.as_ref(), &request_id)?;
            let rules = AuctionRecruitmentSeriesRules::from_optional(
                request.format,
                request.semifinal_best_of,
                request.final_best_of,
                request.hard_fearless,
            );
            require_csrf(&headers, &state, &principal, &request_id)?;
            let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
            let snapshot = bot_workflow::create_web_auction_recruitment(
                database(&state, &request_id)?,
                &guild_id,
                &principal.session.user_id,
                request.format,
                rules,
                expected_start_at,
                &idempotency_key,
                &request_id,
            )
            .await
            .map_err(|error| web_auction_recruitment_error(error, &request_id))?;
            Ok(error::success(StatusCode::CREATED, &request_id, snapshot))
        }
    }
}

pub async fn close_auction_recruitment(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, recruitment_id)): Path<(GuildId, AuctionRecruitmentId)>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(_request): Json<BrowserAuctionRecruitmentLifecycleRequest>,
) -> Result<Response, ApiError> {
    require_csrf(&headers, &state, &principal, &request_id)?;
    let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
    let snapshot = bot_workflow::close_web_auction_recruitment(
        database(&state, &request_id)?,
        &guild_id,
        recruitment_id,
        &principal.session.user_id,
        &idempotency_key,
        &request_id,
    )
    .await
    .map_err(|error| web_auction_recruitment_error(error, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, snapshot))
}

pub async fn reopen_auction_recruitment(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, recruitment_id)): Path<(GuildId, AuctionRecruitmentId)>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(request): Json<BrowserAuctionRecruitmentReopenRequest>,
) -> Result<Response, ApiError> {
    if request.expected_revision < 0 {
        return Err(invalid_browser_auction_recruitment_reopen_request(
            &request_id,
        ));
    }
    require_csrf(&headers, &state, &principal, &request_id)?;
    let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
    let snapshot = bot_workflow::reopen_web_auction_recruitment(
        database(&state, &request_id)?,
        &guild_id,
        recruitment_id,
        &principal.session.user_id,
        request.expected_revision,
        &idempotency_key,
        &request_id,
    )
    .await
    .map_err(|error| web_auction_recruitment_error(error, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, snapshot))
}

pub async fn dispatch_auction_recruitment_participant_confirmation_dms(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, recruitment_id)): Path<(GuildId, AuctionRecruitmentId)>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(_request): Json<BrowserParticipantConfirmationDmRequest>,
) -> Result<Response, ApiError> {
    require_csrf(&headers, &state, &principal, &request_id)?;
    let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
    let result = bot_workflow::dispatch_web_auction_recruitment_participant_confirmation_dms(
        database(&state, &request_id)?,
        &guild_id,
        recruitment_id,
        &principal.session.user_id,
        &idempotency_key,
        &request_id,
    )
    .await
    .map_err(|error| web_participant_confirmation_dm_error(error, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, result))
}

pub async fn update_auction_setup_draft(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, recruitment_id)): Path<(GuildId, AuctionRecruitmentId)>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(request): Json<BrowserAuctionSetupDraftRequest>,
) -> Result<Response, ApiError> {
    if request.expected_revision < 0 {
        return Err(invalid_browser_auction_setup_draft_request(&request_id));
    }
    require_csrf(&headers, &state, &principal, &request_id)?;
    let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
    let snapshot = bot_workflow::update_web_auction_setup_draft(
        database(&state, &request_id)?,
        &guild_id,
        recruitment_id,
        &principal.session.user_id,
        request.expected_revision,
        request.teams,
        &idempotency_key,
        &request_id,
    )
    .await
    .map_err(|error| web_auction_setup_draft_error(error, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, snapshot))
}

pub async fn create_auction_tournament(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(guild_id): Path<GuildId>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(request): Json<BrowserAuctionTournamentCreateRequest>,
) -> Result<Response, ApiError> {
    if request.expected_revision < 0 {
        return Err(invalid_browser_auction_tournament_create_request(
            &request_id,
        ));
    }
    require_csrf(&headers, &state, &principal, &request_id)?;
    let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
    let snapshot = bot_workflow::create_web_auction_tournament(
        database(&state, &request_id)?,
        &guild_id,
        &principal.session.user_id,
        request.source_recruitment_id,
        request.expected_revision,
        &idempotency_key,
        &request_id,
    )
    .await
    .map_err(|error| web_auction_tournament_create_error(error, &request_id))?;
    Ok(error::success(StatusCode::CREATED, &request_id, snapshot))
}

pub async fn execute_auction_tournament_draft_command(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, tournament_id)): Path<(GuildId, AuctionTournamentId)>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(payload): Json<Value>,
) -> Result<Response, ApiError> {
    let command = serde_json::from_value::<AuctionTournamentDraftCommand>(payload)
        .map_err(|_| invalid_browser_auction_tournament_draft_command_request(&request_id))?;
    if command.expected_revision() < 0 {
        return Err(invalid_browser_auction_tournament_draft_command_request(
            &request_id,
        ));
    }
    require_csrf(&headers, &state, &principal, &request_id)?;
    let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
    let snapshot = bot_workflow::execute_web_auction_tournament_draft_command(
        database(&state, &request_id)?,
        &guild_id,
        tournament_id,
        &principal.session.user_id,
        command,
        &idempotency_key,
        &request_id,
    )
    .await
    .map_err(|error| web_auction_tournament_draft_error(error, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, snapshot))
}

pub async fn execute_auction_tournament_bracket_command(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, tournament_id)): Path<(GuildId, AuctionTournamentId)>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(payload): Json<Value>,
) -> Result<Response, ApiError> {
    let command = serde_json::from_value::<AuctionTournamentBracketCommand>(payload)
        .map_err(|_| invalid_browser_auction_tournament_bracket_command_request(&request_id))?;
    if command.expected_revision() < 0 {
        return Err(invalid_browser_auction_tournament_bracket_command_request(
            &request_id,
        ));
    }
    require_csrf(&headers, &state, &principal, &request_id)?;
    let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
    let snapshot = bot_workflow::execute_web_auction_tournament_bracket_command(
        database(&state, &request_id)?,
        &guild_id,
        tournament_id,
        &principal.session.user_id,
        command,
        &idempotency_key,
        &request_id,
    )
    .await
    .map_err(|error| web_auction_tournament_bracket_error(error, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, snapshot))
}

pub async fn cancel_auction_tournament(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, tournament_id)): Path<(GuildId, AuctionTournamentId)>,
    headers: HeaderMap,
    principal: RecruitmentCreatePrincipal,
    Json(request): Json<Value>,
) -> Result<Response, ApiError> {
    match principal {
        RecruitmentCreatePrincipal::Bot(_) => {
            let request = serde_json::from_value::<BotAuctionTournamentCancelRequest>(request)
                .map_err(|_| invalid_request("body", &request_id))?;
            if request.expected_revision < 0 {
                return Err(invalid_request("expectedRevision", &request_id));
            }
            validate_text(&request.reason, 240, "reason", &request_id)?;
            let interaction_id = interaction_id(&headers, &request_id)?;
            let result = bot_workflow::cancel_auction_tournament(
                database(&state, &request_id)?,
                &guild_id,
                tournament_id,
                &interaction_id,
                &request_id,
                AuctionTournamentCancelCommand {
                    actor: actor(request.actor, &request_id)?,
                    expected_revision: request.expected_revision,
                    reason: request.reason.trim().to_owned(),
                },
            )
            .await
            .map_err(|error| command_error(error, &request_id))?;
            Ok(error::success(StatusCode::OK, &request_id, result))
        }
        RecruitmentCreatePrincipal::Session(principal) => {
            let request = serde_json::from_value::<BrowserAuctionTournamentCancelRequest>(request)
                .map_err(|_| invalid_browser_auction_tournament_cancel_request(&request_id))?;
            if request.expected_revision < 0 {
                return Err(invalid_browser_auction_tournament_cancel_request(
                    &request_id,
                ));
            }
            validate_text(&request.reason, 240, "reason", &request_id)?;
            require_csrf(&headers, &state, &principal, &request_id)?;
            let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
            let result = bot_workflow::cancel_web_auction_tournament(
                database(&state, &request_id)?,
                &guild_id,
                tournament_id,
                &principal.session.user_id,
                request.expected_revision,
                request.reason.trim().to_owned(),
                &idempotency_key,
                &request_id,
            )
            .await
            .map_err(|error| web_auction_tournament_cancel_error(error, &request_id))?;
            Ok(error::success(StatusCode::OK, &request_id, result))
        }
    }
}

pub async fn auction_tournament_cancel_preview(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, tournament_id)): Path<(GuildId, AuctionTournamentId)>,
    principal: RecruitmentCreatePrincipal,
) -> Result<Response, ApiError> {
    let preview = match principal {
        RecruitmentCreatePrincipal::Bot(_) => bot_workflow::get_auction_tournament_cancel_preview(
            database(&state, &request_id)?,
            &guild_id,
            tournament_id,
        )
        .await
        .map_err(|error| command_error(error, &request_id))?,
        RecruitmentCreatePrincipal::Session(principal) => {
            bot_workflow::get_web_auction_tournament_cancel_preview(
                database(&state, &request_id)?,
                &guild_id,
                tournament_id,
                &principal.session.user_id,
            )
            .await
            .map_err(|error| web_auction_tournament_cancel_error(error, &request_id))?
        }
    };
    Ok(error::success(StatusCode::OK, &request_id, preview))
}

pub async fn update_auction_match_lineup(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, match_id)): Path<(GuildId, AuctionMatchId)>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(request): Json<BrowserAuctionMatchLineupAssignmentRequest>,
) -> Result<Response, ApiError> {
    if request.expected_revision < 0 {
        return Err(invalid_browser_auction_match_lineup_request(&request_id));
    }
    require_csrf(&headers, &state, &principal, &request_id)?;
    let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
    let snapshot = bot_workflow::update_web_auction_match_lineup(
        database(&state, &request_id)?,
        &guild_id,
        match_id,
        &principal.session.user_id,
        AuctionMatchLineupAssignmentCommand {
            expected_revision: request.expected_revision,
            team_id: request.team_id,
            lane: request.lane,
            team_member_id: request.team_member_id,
        },
        &idempotency_key,
        &request_id,
    )
    .await
    .map_err(|error| web_auction_match_lineup_error(error, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, snapshot))
}

pub async fn lock_auction_match_lineup(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, match_id)): Path<(GuildId, AuctionMatchId)>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(request): Json<BrowserAuctionMatchLineupLockRequest>,
) -> Result<Response, ApiError> {
    if request.expected_revision < 0 {
        return Err(invalid_browser_auction_match_lineup_request(&request_id));
    }
    require_csrf(&headers, &state, &principal, &request_id)?;
    let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
    let snapshot = bot_workflow::lock_web_auction_match_lineup(
        database(&state, &request_id)?,
        &guild_id,
        match_id,
        &principal.session.user_id,
        AuctionMatchLineupLockCommand {
            expected_revision: request.expected_revision,
        },
        &idempotency_key,
        &request_id,
    )
    .await
    .map_err(|error| web_auction_match_lineup_error(error, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, snapshot))
}

pub async fn unlock_auction_match_lineup(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, match_id)): Path<(GuildId, AuctionMatchId)>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(request): Json<BrowserAuctionMatchLineupLockRequest>,
) -> Result<Response, ApiError> {
    if request.expected_revision < 0 {
        return Err(invalid_browser_auction_match_lineup_request(&request_id));
    }
    require_csrf(&headers, &state, &principal, &request_id)?;
    let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
    let snapshot = bot_workflow::unlock_web_auction_match_lineup(
        database(&state, &request_id)?,
        &guild_id,
        match_id,
        &principal.session.user_id,
        AuctionMatchLineupLockCommand {
            expected_revision: request.expected_revision,
        },
        &idempotency_key,
        &request_id,
    )
    .await
    .map_err(|error| web_auction_match_lineup_error(error, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, snapshot))
}

pub async fn submit_auction_match_game(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, match_id)): Path<(GuildId, AuctionMatchId)>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(request): Json<AuctionMatchGameSubmitCommand>,
) -> Result<Response, ApiError> {
    if request.expected_revision < 0 {
        return Err(invalid_browser_auction_match_game_request(&request_id));
    }
    require_csrf(&headers, &state, &principal, &request_id)?;
    let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
    let snapshot = bot_workflow::submit_web_auction_match_game(
        database(&state, &request_id)?,
        &guild_id,
        match_id,
        &principal.session.user_id,
        &idempotency_key,
        &request_id,
        request,
    )
    .await
    .map_err(|error| web_auction_match_game_error(error, &request_id))?;
    Ok(error::success(StatusCode::CREATED, &request_id, snapshot))
}

pub async fn revert_last_auction_match_game(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, match_id)): Path<(GuildId, AuctionMatchId)>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(request): Json<AuctionMatchGameRevertCommand>,
) -> Result<Response, ApiError> {
    if request.expected_revision < 0 {
        return Err(invalid_browser_auction_match_game_request(&request_id));
    }
    require_csrf(&headers, &state, &principal, &request_id)?;
    let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
    let snapshot = bot_workflow::revert_last_web_auction_match_game(
        database(&state, &request_id)?,
        &guild_id,
        match_id,
        &principal.session.user_id,
        &idempotency_key,
        &request_id,
        request,
    )
    .await
    .map_err(|error| web_auction_match_game_error(error, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, snapshot))
}

pub async fn cancel_auction_recruitment(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, recruitment_id)): Path<(GuildId, AuctionRecruitmentId)>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(request): Json<BrowserAuctionRecruitmentReopenRequest>,
) -> Result<Response, ApiError> {
    if request.expected_revision < 0 {
        return Err(invalid_browser_auction_recruitment_lifecycle_request(
            &request_id,
        ));
    }
    require_csrf(&headers, &state, &principal, &request_id)?;
    let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
    let snapshot = bot_workflow::cancel_web_auction_recruitment(
        database(&state, &request_id)?,
        &guild_id,
        recruitment_id,
        &principal.session.user_id,
        request.expected_revision,
        &idempotency_key,
        &request_id,
    )
    .await
    .map_err(|error| web_auction_recruitment_error(error, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, snapshot))
}

pub async fn restore_auction_recruitment(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, recruitment_id)): Path<(GuildId, AuctionRecruitmentId)>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(request): Json<BrowserAuctionRecruitmentReopenRequest>,
) -> Result<Response, ApiError> {
    if request.expected_revision < 0 {
        return Err(invalid_browser_auction_recruitment_lifecycle_request(
            &request_id,
        ));
    }
    require_csrf(&headers, &state, &principal, &request_id)?;
    let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
    let snapshot = bot_workflow::restore_web_auction_recruitment(
        database(&state, &request_id)?,
        &guild_id,
        recruitment_id,
        &principal.session.user_id,
        request.expected_revision,
        &idempotency_key,
        &request_id,
    )
    .await
    .map_err(|error| web_auction_recruitment_error(error, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, snapshot))
}

pub async fn force_cancel_auction_recruitment(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, recruitment_id)): Path<(GuildId, AuctionRecruitmentId)>,
    headers: HeaderMap,
    _principal: BotServicePrincipal,
    Json(request): Json<ActorOnlyRequest>,
) -> Result<Response, ApiError> {
    let interaction_id = interaction_id(&headers, &request_id)?;
    let result = bot_workflow::force_cancel_auction_recruitment(
        database(&state, &request_id)?,
        &guild_id,
        recruitment_id,
        &interaction_id,
        &request_id,
        AuctionRecruitmentForceCancelCommand {
            actor: actor(request.actor, &request_id)?,
        },
    )
    .await
    .map_err(|error| command_error(error, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, result))
}

pub async fn join_auction_recruitment(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, recruitment_id)): Path<(GuildId, AuctionRecruitmentId)>,
    headers: HeaderMap,
    principal: RecruitmentCreatePrincipal,
    Json(request): Json<Value>,
) -> Result<Response, ApiError> {
    match principal {
        RecruitmentCreatePrincipal::Bot(_) => {
            let request = serde_json::from_value::<AuctionRecruitmentMemberRequest>(request)
                .map_err(|_| invalid_request("body", &request_id))?;
            let interaction_id = interaction_id(&headers, &request_id)?;
            let command_actor = actor(request.actor, &request_id)?;
            let participant = request
                .member
                .map(|member| actor(member, &request_id))
                .transpose()?
                .unwrap_or_else(|| command_actor.clone());
            let snapshot = bot_workflow::execute_auction_recruitment_participation_command(
                database(&state, &request_id)?,
                &guild_id,
                &interaction_id,
                &request_id,
                BotAuctionRecruitmentParticipationCommand::Join {
                    actor: command_actor,
                    participant,
                    recruitment_id,
                    declared_lane: request.declared_lane,
                },
            )
            .await
            .map_err(|error| command_error(error, &request_id))?;

            Ok(error::success(StatusCode::OK, &request_id, snapshot))
        }
        RecruitmentCreatePrincipal::Session(principal) => {
            let request =
                serde_json::from_value::<BrowserAddAuctionRecruitmentMemberRequest>(request)
                    .map_err(|_| invalid_browser_auction_recruitment_member_request(&request_id))?;
            require_csrf(&headers, &state, &principal, &request_id)?;
            let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
            let snapshot = bot_workflow::add_web_auction_recruitment_member(
                database(&state, &request_id)?,
                &guild_id,
                recruitment_id,
                &principal.session.user_id,
                request.member_id,
                request.declared_lane,
                &idempotency_key,
                &request_id,
            )
            .await
            .map_err(|error| web_auction_recruitment_error(error, &request_id))?;
            Ok(error::success(StatusCode::OK, &request_id, snapshot))
        }
    }
}

pub async fn leave_auction_recruitment(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, recruitment_id, user_id)): Path<(GuildId, AuctionRecruitmentId, UserId)>,
    headers: HeaderMap,
    principal: RecruitmentCreatePrincipal,
    Json(request): Json<Value>,
) -> Result<Response, ApiError> {
    match principal {
        RecruitmentCreatePrincipal::Bot(_) => {
            let request = serde_json::from_value::<ActorOnlyRequest>(request)
                .map_err(|_| invalid_request("body", &request_id))?;
            let interaction_id = interaction_id(&headers, &request_id)?;
            let snapshot = bot_workflow::execute_auction_recruitment_participation_command(
                database(&state, &request_id)?,
                &guild_id,
                &interaction_id,
                &request_id,
                BotAuctionRecruitmentParticipationCommand::Leave {
                    actor: actor(request.actor, &request_id)?,
                    recruitment_id,
                    user_id,
                },
            )
            .await
            .map_err(|error| command_error(error, &request_id))?;

            Ok(error::success(StatusCode::OK, &request_id, snapshot))
        }
        RecruitmentCreatePrincipal::Session(principal) => {
            serde_json::from_value::<BrowserRemoveAuctionRecruitmentMemberRequest>(request)
                .map_err(|_| invalid_browser_auction_recruitment_member_request(&request_id))?;
            require_csrf(&headers, &state, &principal, &request_id)?;
            let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
            let snapshot = bot_workflow::remove_web_auction_recruitment_member(
                database(&state, &request_id)?,
                &guild_id,
                recruitment_id,
                &principal.session.user_id,
                user_id,
                &idempotency_key,
                &request_id,
            )
            .await
            .map_err(|error| web_auction_recruitment_error(error, &request_id))?;
            Ok(error::success(StatusCode::OK, &request_id, snapshot))
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
    Json(request): Json<BrowserEntryDraftRevisionRequest>,
) -> Result<Response, ApiError> {
    if request.expected_revision < 0 {
        return Err(invalid_browser_recruitment_lifecycle_request(&request_id));
    }
    require_csrf(&headers, &state, &principal, &request_id)?;
    let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
    let snapshot = bot_workflow::cancel_web_recruitment(
        database(&state, &request_id)?,
        &guild_id,
        recruitment_id,
        &principal.session.user_id,
        request.expected_revision,
        &idempotency_key,
        &request_id,
    )
    .await
    .map_err(|error| web_recruitment_mutation_error(error, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, snapshot))
}

pub async fn restore_recruitment(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, recruitment_id)): Path<(GuildId, RecruitmentId)>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(request): Json<BrowserEntryDraftRevisionRequest>,
) -> Result<Response, ApiError> {
    if request.expected_revision < 0 {
        return Err(invalid_browser_recruitment_lifecycle_request(&request_id));
    }
    require_csrf(&headers, &state, &principal, &request_id)?;
    let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
    let snapshot = bot_workflow::restore_web_recruitment(
        database(&state, &request_id)?,
        &guild_id,
        recruitment_id,
        &principal.session.user_id,
        request.expected_revision,
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

pub async fn dispatch_recruitment_participant_confirmation_dms(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, recruitment_id)): Path<(GuildId, RecruitmentId)>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(_request): Json<BrowserParticipantConfirmationDmRequest>,
) -> Result<Response, ApiError> {
    require_csrf(&headers, &state, &principal, &request_id)?;
    let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
    let result = bot_workflow::dispatch_web_recruitment_participant_confirmation_dms(
        database(&state, &request_id)?,
        &guild_id,
        recruitment_id,
        &principal.session.user_id,
        &idempotency_key,
        &request_id,
    )
    .await
    .map_err(|error| web_participant_confirmation_dm_error(error, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, result))
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

pub async fn convert_entry_draft_to_auction_recruitment(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((guild_id, recruitment_id)): Path<(GuildId, RecruitmentId)>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(request): Json<BrowserEntryDraftAuctionConversionRequest>,
) -> Result<Response, ApiError> {
    if request.expected_revision < 0 {
        return Err(invalid_browser_entry_auction_conversion_request(
            &request_id,
        ));
    }
    let expected_start_at =
        parse_expected_start_at(request.expected_start_at.as_ref(), &request_id)?;
    require_csrf(&headers, &state, &principal, &request_id)?;
    let idempotency_key = browser_idempotency_key(&headers, &request_id)?;
    let snapshot = bot_workflow::convert_web_entry_draft_to_auction_recruitment(
        database(&state, &request_id)?,
        &guild_id,
        &principal.session.user_id,
        recruitment_id,
        request.expected_revision,
        expected_start_at,
        &idempotency_key,
        &request_id,
    )
    .await
    .map_err(|error| web_entry_auction_conversion_error(error, &request_id))?;
    Ok(error::success(StatusCode::CREATED, &request_id, snapshot))
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

fn parse_expected_start_at(
    value: Option<&Value>,
    request_id: &RequestId,
) -> Result<OffsetDateTime, ApiError> {
    let Some(value) = value else {
        return Ok(default_expected_start_at());
    };
    let Some(value) = value.as_str() else {
        return Err(invalid_expected_start_at(request_id));
    };
    let value = value.trim();
    if value.is_empty() {
        return Ok(default_expected_start_at());
    }

    OffsetDateTime::parse(value, &Rfc3339).map_err(|_| invalid_expected_start_at(request_id))
}

fn default_expected_start_at() -> OffsetDateTime {
    OffsetDateTime::now_utc() + Duration::minutes(30)
}

fn invalid_expected_start_at(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        request_id,
        "invalid_expected_start_at",
        "예상 시작 시간은 시간대가 포함된 ISO 8601 형식이어야 합니다.",
        Some(serde_json::json!({ "field": "expectedStartAt" })),
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

fn invalid_browser_auction_recruitment_create_request(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        request_id,
        "invalid_auction_recruitment_create_request",
        "경매 모집 생성 요청 형식이 올바르지 않습니다.",
        None,
    )
}

fn invalid_browser_auction_recruitment_reopen_request(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        request_id,
        "invalid_auction_recruitment_reopen_request",
        "경매 모집 재개 요청 형식이 올바르지 않습니다.",
        None,
    )
}

fn invalid_browser_auction_recruitment_lifecycle_request(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        request_id,
        "invalid_auction_recruitment_lifecycle_request",
        "경매 모집 취소 또는 복구 요청 형식이 올바르지 않습니다.",
        None,
    )
}

fn invalid_browser_recruitment_lifecycle_request(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        request_id,
        "invalid_recruitment_lifecycle_request",
        "모집 취소 또는 복구 요청 형식이 올바르지 않습니다.",
        None,
    )
}

fn invalid_browser_entry_auction_conversion_request(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        request_id,
        "invalid_entry_auction_conversion_request",
        "엔트리 경매 전환 요청 형식이 올바르지 않습니다.",
        None,
    )
}

fn invalid_browser_auction_recruitment_member_request(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        request_id,
        "invalid_auction_recruitment_member_request",
        "경매 모집 참가자 변경 요청 형식이 올바르지 않습니다.",
        None,
    )
}

fn invalid_browser_auction_setup_draft_request(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        request_id,
        "invalid_auction_setup_draft_request",
        "경매 팀장 설정 요청 형식이 올바르지 않습니다.",
        None,
    )
}

fn invalid_browser_auction_tournament_create_request(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        request_id,
        "invalid_auction_tournament_create_request",
        "경매 내전 시작 요청 형식이 올바르지 않습니다.",
        None,
    )
}

fn invalid_browser_auction_tournament_draft_command_request(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        request_id,
        "invalid_auction_tournament_draft_command_request",
        "경매 드래프트 명령 형식이 올바르지 않습니다.",
        None,
    )
}

fn invalid_browser_auction_tournament_bracket_command_request(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        request_id,
        "invalid_auction_tournament_bracket_command_request",
        "경매 브래킷 명령 형식이 올바르지 않습니다.",
        None,
    )
}

fn invalid_browser_auction_tournament_cancel_request(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        request_id,
        "invalid_auction_tournament_cancel_request",
        "경매 내전 조기 종료 요청 형식이 올바르지 않습니다.",
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

fn invalid_browser_auction_match_lineup_request(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        request_id,
        "invalid_auction_match_lineup_request",
        "경매 경기 라인업 요청 형식이 올바르지 않습니다.",
        None,
    )
}

fn invalid_browser_auction_match_game_request(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        request_id,
        "invalid_auction_match_game_request",
        "경매 경기 결과 입력 요청 형식이 올바르지 않습니다.",
        None,
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
        BotWorkflowError::InvalidAuctionRecruitmentBestOf { stage, received } => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "invalid_auction_recruitment_best_of",
            "경매 모집 시리즈 형식은 BO1, BO3, BO5만 선택할 수 있습니다.",
            Some(serde_json::json!({ "stage": stage, "bestOf": received })),
        ),
        BotWorkflowError::AuctionRecruitmentSemifinalBestOfUnsupported => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "auction_semifinal_best_of_unsupported",
            "10인 경매 모집에는 4강 시리즈 형식을 설정할 수 없습니다.",
            None,
        ),
        BotWorkflowError::AuctionRecruitmentSemifinalBestOfRequired => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "auction_semifinal_best_of_required",
            "20인 경매 모집에는 4강 시리즈 형식이 필요합니다.",
            None,
        ),
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
        BotWorkflowError::RecruitmentRestoreUnavailable(status) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "recruitment_restore_unavailable",
            "취소된 모집만 복구할 수 있습니다.",
            Some(serde_json::json!({ "currentStatus": status.as_str() })),
        ),
        BotWorkflowError::ParticipantConfirmationDmUnavailable(status) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "participant_confirmation_dm_unavailable",
            "마감된 모집에서만 참가 확정 DM을 발송할 수 있습니다.",
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
        BotWorkflowError::EntryDraftAuctionSourceNotReady(status) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "entry_draft_auction_source_not_ready",
            "마감된 일반 엔트리 모집만 경매 모집으로 전환할 수 있습니다.",
            Some(serde_json::json!({ "currentStatus": status.as_str() })),
        ),
        BotWorkflowError::EntryDraftAlreadyConvertedToSeries => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "entry_draft_already_converted_to_series",
            "이미 일반 시리즈로 전환된 모집은 경매 모집으로 전환할 수 없습니다.",
            None,
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
        BotWorkflowError::SeriesGameSideRequired => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "series_game_side_required",
            "픽밴을 입력하기 전에 현재 세트의 블루/레드 사이드를 먼저 선택해 주세요.",
            None,
        ),
        BotWorkflowError::SeriesGameSideMismatch { expected, actual } => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "series_game_side_mismatch",
            "이미 선택된 세트 사이드와 다른 사이드로 결과를 기록할 수 없습니다.",
            Some(serde_json::json!({ "expectedSidePreset": expected, "actualSidePreset": actual })),
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
        BotWorkflowError::AuctionDeclaredLaneRequired => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "auction_declared_lane_required",
            "주 라인을 먼저 설정하거나 모집 메시지에서 참가 라인을 선택해 주세요.",
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
        BotWorkflowError::AuctionTournamentNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_id,
            "not_found",
            "요청한 경매 내전을 찾을 수 없습니다.",
            None,
        ),
        BotWorkflowError::AuctionSetupDraftNotEditable(status)
        | BotWorkflowError::AuctionTournamentSourceNotReady(status) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "invalid_auction_recruitment_state",
            "현재 경매 모집 상태에서는 이 작업을 수행할 수 없습니다.",
            Some(serde_json::json!({ "currentStatus": status.as_str() })),
        ),
        BotWorkflowError::AuctionSetupDraftRevisionConflict { expected, actual } => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_setup_draft_revision_conflict",
            "경매 팀장 설정이 변경되었습니다. 최신 상태를 확인해 주세요.",
            Some(serde_json::json!({ "expectedRevision": expected, "actualRevision": actual })),
        ),
        BotWorkflowError::AuctionSetupTeamCountInvalid { expected, actual } => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "invalid_auction_setup_team_count",
            "경매 형식에 맞는 팀 수를 입력해야 합니다.",
            Some(serde_json::json!({ "expectedTeamCount": expected, "actualTeamCount": actual })),
        ),
        BotWorkflowError::AuctionSetupTeamIndexInvalid { expected, actual } => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "invalid_auction_setup_team_index",
            "팀 순서는 1번부터 연속해서 입력해야 합니다.",
            Some(serde_json::json!({ "expectedTeamIndex": expected, "actualTeamIndex": actual })),
        ),
        BotWorkflowError::AuctionSetupTeamNameInvalid => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "invalid_auction_setup_team_name",
            "팀 이름이 올바르지 않습니다.",
            None,
        ),
        BotWorkflowError::AuctionSetupInitialPointsInvalid => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "invalid_auction_setup_initial_points",
            "초기 포인트는 1 이상이어야 합니다.",
            None,
        ),
        BotWorkflowError::AuctionSetupDuplicateCaptain => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "duplicate_auction_setup_captain",
            "같은 참가자를 여러 팀의 팀장으로 지정할 수 없습니다.",
            None,
        ),
        BotWorkflowError::AuctionSetupCaptainNotJoined => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "auction_setup_captain_not_joined",
            "팀장은 현재 모집에 참가한 길드 멤버여야 합니다.",
            None,
        ),
        BotWorkflowError::AuctionSetupDraftIncomplete {
            configured,
            required,
        } => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "auction_setup_incomplete",
            "모든 팀의 팀장을 설정한 뒤 경매 내전을 시작할 수 있습니다.",
            Some(
                serde_json::json!({ "configuredTeamCount": configured, "requiredTeamCount": required }),
            ),
        ),
        BotWorkflowError::AuctionTournamentAlreadyExists => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_tournament_already_exists",
            "이 모집으로 이미 진행 중인 경매 내전이 있습니다.",
            None,
        ),
        error @ (BotWorkflowError::AuctionTournamentRevisionConflict { .. }
        | BotWorkflowError::AuctionTournamentDraftNotEditable(_)
        | BotWorkflowError::AuctionTournamentDraftLocked
        | BotWorkflowError::AuctionDraftCandidateNotFound
        | BotWorkflowError::AuctionDraftTeamNotFound
        | BotWorkflowError::AuctionDraftUnsoldLotNotFound
        | BotWorkflowError::AuctionDraftCurrentTargetRequired
        | BotWorkflowError::AuctionDraftCurrentTargetExists
        | BotWorkflowError::AuctionDraftCurrentTargetMismatch
        | BotWorkflowError::AuctionDraftCandidateStateMismatch
        | BotWorkflowError::AuctionDraftNoQueuedCandidate
        | BotWorkflowError::AuctionDraftTeamFull
        | BotWorkflowError::AuctionDraftBidPointsInvalid { .. }
        | BotWorkflowError::AuctionDraftNoRevertableAction
        | BotWorkflowError::AuctionDraftRevertUnavailable) => {
            web_auction_tournament_draft_error(error, request_id)
        }
        error @ (BotWorkflowError::AuctionTournamentBracketNotEditable(_)
        | BotWorkflowError::AuctionTournamentBracketLocked
        | BotWorkflowError::AuctionTournamentBracketRecoveryUnavailable
        | BotWorkflowError::AuctionTournamentBracketSlotUnavailable
        | BotWorkflowError::AuctionTournamentBracketRosterIncomplete
        | BotWorkflowError::AuctionTournamentBracketSlotsIncomplete) => {
            web_auction_tournament_bracket_error(error, request_id)
        }
        BotWorkflowError::AuctionTournamentBracketUnavailable(status) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_tournament_bracket_unavailable",
            "경매 드래프트가 완료된 후 브래킷을 확인할 수 있습니다.",
            Some(serde_json::json!({ "currentStatus": status.as_str() })),
        ),
        error @ (BotWorkflowError::AuctionMatchNotFound
        | BotWorkflowError::AuctionMatchTeamsPending
        | BotWorkflowError::AuctionMatchTournamentNotReady
        | BotWorkflowError::AuctionMatchRevisionConflict { .. }
        | BotWorkflowError::AuctionMatchLineupNotEditable(_)
        | BotWorkflowError::AuctionMatchLineupLocked
        | BotWorkflowError::AuctionMatchLineupUnlockUnavailable(_)
        | BotWorkflowError::AuctionMatchLineupResultArtifactsPresent
        | BotWorkflowError::AuctionMatchLineupTeamNotFound
        | BotWorkflowError::AuctionMatchLineupMemberNotFound
        | BotWorkflowError::AuctionMatchLineupPlayerAssigned
        | BotWorkflowError::AuctionMatchLineupIncomplete) => {
            web_auction_match_lineup_error(error, request_id)
        }
        error @ (BotWorkflowError::AuctionMatchSemifinalLineupsIncomplete
        | BotWorkflowError::AuctionMatchResultNotEditable(_)
        | BotWorkflowError::AuctionMatchSetNotCurrent { .. }
        | BotWorkflowError::AuctionMatchGameChampionDuplicate { .. }
        | BotWorkflowError::AuctionMatchHardFearlessChampionUsed { .. }
        | BotWorkflowError::AuctionMatchNoConfirmedGames
        | BotWorkflowError::AuctionMatchGameMmrHistoryAdvanced
        | BotWorkflowError::AuctionMatchDownstreamWorkExists
        | BotWorkflowError::AuctionTournamentResultDeliveryInProgress
        | BotWorkflowError::AuctionTournamentResultDeliveryUncertain) => {
            web_auction_match_game_error(error, request_id)
        }
        BotWorkflowError::AuctionTournamentResultUnavailable(status) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_tournament_result_unavailable",
            "진행 중이거나 완료된 경매 내전에서만 결과를 확인할 수 있습니다.",
            Some(serde_json::json!({ "currentStatus": status.as_str() })),
        ),
        BotWorkflowError::AuctionTournamentCancelUnavailable(status) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_tournament_cancel_unavailable",
            "완료되었거나 이미 취소된 경매 내전은 조기 종료할 수 없습니다.",
            Some(serde_json::json!({ "currentStatus": status.as_str() })),
        ),
        BotWorkflowError::AuctionTournamentCancelMmrHistoryAdvanced => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_tournament_cancel_mmr_history_advanced",
            "이후 MMR 변경이 있어 경매 내전 전체를 안전하게 취소할 수 없습니다.",
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

fn web_auction_recruitment_error(error: BotWorkflowError, request_id: &RequestId) -> ApiError {
    match error {
        BotWorkflowError::InvalidCommand(_) => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "invalid_auction_recruitment_request",
            "경매 모집 요청 형식이 올바르지 않습니다.",
            None,
        ),
        BotWorkflowError::MembershipRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "guild_membership_required",
            "해당 길드의 활성 멤버만 경매 모집을 만들거나 변경할 수 있습니다.",
            None,
        ),
        BotWorkflowError::OperatorRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "operator_required",
            "경매 모집을 변경하려면 현재 길드 운영자 권한이 필요합니다.",
            None,
        ),
        BotWorkflowError::NoActiveSeason => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "no_active_season",
            "길드에 진행 중인 시즌이 없어 경매 모집을 만들 수 없습니다.",
            None,
        ),
        BotWorkflowError::RecruitmentNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_id,
            "auction_recruitment_not_found",
            "요청한 경매 모집을 찾을 수 없습니다.",
            None,
        ),
        BotWorkflowError::RecruitmentNotOpen(status) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_recruitment_not_open",
            "모집 중인 경매에서만 이 작업을 수행할 수 있습니다.",
            Some(serde_json::json!({ "currentStatus": status.as_str() })),
        ),
        BotWorkflowError::RecruitmentRestoreUnavailable(status) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_recruitment_restore_unavailable",
            "취소된 경매 모집만 복구할 수 있습니다.",
            Some(serde_json::json!({ "currentStatus": status.as_str() })),
        ),
        BotWorkflowError::AuctionSetupDraftNotEditable(status) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_recruitment_reopen_unavailable",
            "팀장 선택 단계의 경매 모집만 모집 단계로 되돌릴 수 있습니다.",
            Some(serde_json::json!({ "currentStatus": status.as_str() })),
        ),
        BotWorkflowError::RecruitmentRevisionConflict { expected, actual } => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "recruitment_revision_conflict",
            "경매 모집 상태가 변경되었습니다. 최신 상태를 확인해 주세요.",
            Some(serde_json::json!({ "expectedRevision": expected, "actualRevision": actual })),
        ),
        BotWorkflowError::CloseRequiresFullRoster { joined, target } => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "auction_recruitment_not_full",
            "정원을 채운 뒤 모집을 마감할 수 있습니다.",
            Some(serde_json::json!({ "joinedCount": joined, "targetCount": target })),
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
            "같은 경매 모집 요청을 처리하고 있습니다.",
            None,
        ),
        error => command_error(error, request_id),
    }
}

fn web_auction_setup_draft_error(error: BotWorkflowError, request_id: &RequestId) -> ApiError {
    match error {
        BotWorkflowError::MembershipRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "guild_membership_required",
            "해당 길드의 활성 멤버만 경매 팀장 설정을 변경할 수 있습니다.",
            None,
        ),
        BotWorkflowError::OperatorRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "operator_required",
            "경매 팀장 설정을 변경하려면 현재 길드 운영자 권한이 필요합니다.",
            None,
        ),
        BotWorkflowError::RecruitmentNotFound
        | BotWorkflowError::AuctionSetupDraftNotEditable(_) => ApiError::new(
            StatusCode::NOT_FOUND,
            request_id,
            "auction_setup_draft_not_found",
            "요청한 경매 팀장 설정을 찾을 수 없습니다.",
            None,
        ),
        BotWorkflowError::AuctionSetupDraftRevisionConflict { expected, actual } => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_setup_draft_revision_conflict",
            "경매 팀장 설정이 변경되었습니다. 최신 상태를 확인해 주세요.",
            Some(serde_json::json!({ "expectedRevision": expected, "actualRevision": actual })),
        ),
        BotWorkflowError::CloseRequiresFullRoster { joined, target } => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "auction_recruitment_not_full",
            "정원을 모두 채운 뒤 팀장 설정을 저장할 수 있습니다.",
            Some(serde_json::json!({ "joinedCount": joined, "targetCount": target })),
        ),
        BotWorkflowError::AuctionSetupTeamCountInvalid { expected, actual } => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "invalid_auction_setup_team_count",
            "경매 형식에 맞는 팀 수를 입력해야 합니다.",
            Some(serde_json::json!({ "expectedTeamCount": expected, "actualTeamCount": actual })),
        ),
        BotWorkflowError::AuctionSetupTeamIndexInvalid { expected, actual } => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "invalid_auction_setup_team_index",
            "팀 순서는 1번부터 연속해서 입력해야 합니다.",
            Some(serde_json::json!({ "expectedTeamIndex": expected, "actualTeamIndex": actual })),
        ),
        BotWorkflowError::AuctionSetupTeamNameInvalid => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "invalid_auction_setup_team_name",
            "팀 이름은 비어 있지 않은 64바이트 이하의 값이어야 합니다.",
            None,
        ),
        BotWorkflowError::AuctionSetupInitialPointsInvalid => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "invalid_auction_setup_initial_points",
            "초기 포인트는 1 이상이어야 합니다.",
            None,
        ),
        BotWorkflowError::AuctionSetupDuplicateCaptain => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "duplicate_auction_setup_captain",
            "같은 참가자를 여러 팀의 팀장으로 지정할 수 없습니다.",
            None,
        ),
        BotWorkflowError::AuctionSetupCaptainNotJoined => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "auction_setup_captain_not_joined",
            "팀장은 현재 모집에 참가한 길드 멤버여야 합니다.",
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
            "같은 경매 팀장 설정 요청을 처리하고 있습니다.",
            None,
        ),
        error => command_error(error, request_id),
    }
}

fn web_auction_tournament_create_error(
    error: BotWorkflowError,
    request_id: &RequestId,
) -> ApiError {
    match error {
        BotWorkflowError::MembershipRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "guild_membership_required",
            "해당 길드의 활성 멤버만 경매 내전을 시작할 수 있습니다.",
            None,
        ),
        BotWorkflowError::OperatorRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "operator_required",
            "경매 내전을 시작하려면 현재 길드 운영자 권한이 필요합니다.",
            None,
        ),
        BotWorkflowError::RecruitmentNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_id,
            "auction_recruitment_not_found",
            "요청한 경매 모집을 찾을 수 없습니다.",
            None,
        ),
        BotWorkflowError::AuctionSetupDraftRevisionConflict { expected, actual } => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_setup_draft_revision_conflict",
            "경매 팀장 설정이 변경되었습니다. 최신 상태를 확인해 주세요.",
            Some(serde_json::json!({ "expectedRevision": expected, "actualRevision": actual })),
        ),
        BotWorkflowError::AuctionTournamentSourceNotReady(status) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_tournament_source_not_ready",
            "모집 마감 후에만 경매 내전을 시작할 수 있습니다.",
            Some(serde_json::json!({ "currentStatus": status.as_str() })),
        ),
        BotWorkflowError::AuctionSetupDraftIncomplete {
            configured,
            required,
        } => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "auction_setup_incomplete",
            "모든 팀의 팀장과 초기 포인트를 설정한 뒤 경매 내전을 시작할 수 있습니다.",
            Some(
                serde_json::json!({ "configuredTeamCount": configured, "requiredTeamCount": required }),
            ),
        ),
        BotWorkflowError::AuctionSetupCaptainNotJoined => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "auction_setup_captain_not_joined",
            "팀장은 현재 모집에 참가한 길드 멤버여야 합니다.",
            None,
        ),
        BotWorkflowError::AuctionTournamentAlreadyExists => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_tournament_already_exists",
            "이 모집으로 이미 진행 중인 경매 내전이 있습니다.",
            None,
        ),
        BotWorkflowError::CloseRequiresFullRoster { joined, target } => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "auction_recruitment_not_full",
            "정원을 모두 채운 뒤 경매 내전을 시작할 수 있습니다.",
            Some(serde_json::json!({ "joinedCount": joined, "targetCount": target })),
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
            "같은 경매 내전 시작 요청을 처리하고 있습니다.",
            None,
        ),
        error => command_error(error, request_id),
    }
}

fn web_auction_tournament_draft_error(error: BotWorkflowError, request_id: &RequestId) -> ApiError {
    match error {
        BotWorkflowError::MembershipRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "guild_membership_required",
            "해당 길드의 활성 멤버만 경매 드래프트를 확인하거나 변경할 수 있습니다.",
            None,
        ),
        BotWorkflowError::OperatorRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "operator_required",
            "경매 드래프트를 변경하려면 현재 길드 운영자 권한이 필요합니다.",
            None,
        ),
        BotWorkflowError::AuctionTournamentNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_id,
            "auction_tournament_not_found",
            "요청한 경매 내전을 찾을 수 없습니다.",
            None,
        ),
        BotWorkflowError::AuctionTournamentRevisionConflict { expected, actual } => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_tournament_revision_conflict",
            "다른 운영자가 경매 드래프트를 변경했습니다. 최신 상태를 확인해 주세요.",
            Some(serde_json::json!({ "expectedRevision": expected, "actualRevision": actual })),
        ),
        BotWorkflowError::AuctionTournamentDraftNotEditable(status) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_tournament_draft_not_editable",
            "현재 경매 내전 상태에서는 드래프트를 변경할 수 없습니다.",
            Some(serde_json::json!({ "currentStatus": status.as_str() })),
        ),
        BotWorkflowError::AuctionTournamentDraftLocked => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_tournament_draft_locked",
            "대진 또는 경기 기록이 있어 경매 드래프트를 변경할 수 없습니다.",
            None,
        ),
        BotWorkflowError::AuctionDraftCandidateNotFound => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "auction_draft_candidate_not_found",
            "경매 후보 풀에서 해당 참가자를 찾을 수 없습니다.",
            None,
        ),
        BotWorkflowError::AuctionDraftTeamNotFound => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "auction_draft_team_not_found",
            "경매 내전에서 해당 팀을 찾을 수 없습니다.",
            None,
        ),
        BotWorkflowError::AuctionDraftUnsoldLotNotFound => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_draft_unsold_lot_not_found",
            "선택한 유찰 후보의 처리 기록을 찾을 수 없습니다. 최신 상태를 확인해 주세요.",
            None,
        ),
        BotWorkflowError::AuctionDraftCurrentTargetRequired => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_draft_current_target_required",
            "현재 추첨된 후보가 있어야 이 작업을 수행할 수 있습니다.",
            None,
        ),
        BotWorkflowError::AuctionDraftCurrentTargetExists => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_draft_current_target_exists",
            "현재 추첨된 후보를 먼저 처리해야 합니다.",
            None,
        ),
        BotWorkflowError::AuctionDraftCurrentTargetMismatch => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_draft_current_target_mismatch",
            "선택한 후보가 현재 추첨 대상과 다릅니다. 최신 상태를 확인해 주세요.",
            None,
        ),
        BotWorkflowError::AuctionDraftCandidateStateMismatch => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_draft_candidate_state_mismatch",
            "선택한 후보의 현재 상태에서는 이 작업을 수행할 수 없습니다.",
            None,
        ),
        BotWorkflowError::AuctionDraftNoQueuedCandidate => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_draft_no_queued_candidate",
            "다음으로 추첨할 후보가 없습니다.",
            None,
        ),
        BotWorkflowError::AuctionDraftTeamFull => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_draft_team_full",
            "선택한 팀은 이미 정원을 채웠습니다.",
            None,
        ),
        BotWorkflowError::AuctionDraftBidPointsInvalid {
            current_points,
            submitted_points,
        } => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "invalid_auction_draft_bid_points",
            "낙찰 포인트는 팀의 현재 포인트 범위에서 입력해야 합니다.",
            Some(
                serde_json::json!({ "currentPoints": current_points, "submittedPoints": submitted_points }),
            ),
        ),
        BotWorkflowError::AuctionDraftNoRevertableAction => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_draft_no_revertable_action",
            "되돌릴 수 있는 마지막 경매 작업이 없습니다.",
            None,
        ),
        BotWorkflowError::AuctionDraftRevertUnavailable => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_draft_revert_unavailable",
            "이후 변경이 있어 마지막 경매 작업을 안전하게 되돌릴 수 없습니다.",
            None,
        ),
        BotWorkflowError::IdempotencyConflict => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_conflict",
            "같은 Idempotency-Key가 다른 경매 드래프트 요청에 사용되었습니다.",
            None,
        ),
        BotWorkflowError::IdempotencyInProgress => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_in_progress",
            "같은 경매 드래프트 요청을 처리하고 있습니다.",
            None,
        ),
        error => command_error(error, request_id),
    }
}

fn web_auction_tournament_bracket_error(
    error: BotWorkflowError,
    request_id: &RequestId,
) -> ApiError {
    match error {
        BotWorkflowError::InvalidCommand(_) => {
            invalid_browser_auction_tournament_bracket_command_request(request_id)
        }
        BotWorkflowError::MembershipRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "guild_membership_required",
            "해당 길드의 활성 멤버만 경매 브래킷을 확인하거나 변경할 수 있습니다.",
            None,
        ),
        BotWorkflowError::OperatorRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "operator_required",
            "경매 브래킷을 변경하려면 현재 길드 운영자 권한이 필요합니다.",
            None,
        ),
        BotWorkflowError::AuctionTournamentNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_id,
            "auction_tournament_not_found",
            "요청한 경매 내전을 찾을 수 없습니다.",
            None,
        ),
        BotWorkflowError::AuctionTournamentRevisionConflict { expected, actual } => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_tournament_revision_conflict",
            "다른 운영자가 경매 브래킷을 변경했습니다. 최신 상태를 확인해 주세요.",
            Some(serde_json::json!({ "expectedRevision": expected, "actualRevision": actual })),
        ),
        BotWorkflowError::AuctionTournamentBracketNotEditable(status) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_tournament_bracket_not_editable",
            "브래킷 설정 단계에서만 이 작업을 수행할 수 있습니다.",
            Some(serde_json::json!({ "currentStatus": status.as_str() })),
        ),
        BotWorkflowError::AuctionTournamentBracketLocked => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_tournament_bracket_locked",
            "브래킷이 확정되었거나 경기 기록이 있어 변경할 수 없습니다.",
            None,
        ),
        BotWorkflowError::AuctionTournamentBracketRecoveryUnavailable => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_tournament_bracket_recovery_unavailable",
            "게임 입력 또는 결과 기록이 있어 브래킷 확정을 해제할 수 없습니다.",
            None,
        ),
        BotWorkflowError::AuctionTournamentBracketSlotUnavailable => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "auction_tournament_bracket_slot_unavailable",
            "현재 경매 형식에서는 선택한 브래킷 슬롯을 사용할 수 없습니다.",
            None,
        ),
        BotWorkflowError::AuctionDraftTeamNotFound => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "auction_tournament_bracket_team_not_found",
            "경매 내전에서 선택한 팀을 찾을 수 없습니다.",
            None,
        ),
        BotWorkflowError::AuctionTournamentBracketRosterIncomplete => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_tournament_bracket_roster_incomplete",
            "모든 팀의 활성 roster가 5명이어야 브래킷을 확정할 수 있습니다.",
            None,
        ),
        BotWorkflowError::AuctionTournamentBracketSlotsIncomplete => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_tournament_bracket_slots_incomplete",
            "경매 형식에 필요한 모든 브래킷 슬롯에 팀을 배치해야 합니다.",
            None,
        ),
        BotWorkflowError::IdempotencyConflict => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_conflict",
            "같은 Idempotency-Key가 다른 경매 브래킷 요청에 사용되었습니다.",
            None,
        ),
        BotWorkflowError::IdempotencyInProgress => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_in_progress",
            "같은 경매 브래킷 요청을 처리하고 있습니다.",
            None,
        ),
        error => command_error(error, request_id),
    }
}

fn web_auction_tournament_cancel_error(
    error: BotWorkflowError,
    request_id: &RequestId,
) -> ApiError {
    match error {
        BotWorkflowError::InvalidCommand(_) => {
            invalid_browser_auction_tournament_cancel_request(request_id)
        }
        BotWorkflowError::MembershipRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "guild_membership_required",
            "해당 길드의 활성 멤버만 경매 내전을 조기 종료할 수 있습니다.",
            None,
        ),
        BotWorkflowError::OperatorRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "operator_required",
            "경매 내전을 조기 종료하려면 현재 길드 운영자 권한이 필요합니다.",
            None,
        ),
        BotWorkflowError::AuctionTournamentNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_id,
            "auction_tournament_not_found",
            "요청한 경매 내전을 찾을 수 없습니다.",
            None,
        ),
        BotWorkflowError::AuctionTournamentRevisionConflict { expected, actual } => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_tournament_revision_conflict",
            "다른 운영자가 경매 내전을 변경했습니다. 최신 상태를 확인해 주세요.",
            Some(serde_json::json!({ "expectedRevision": expected, "actualRevision": actual })),
        ),
        BotWorkflowError::AuctionTournamentCancelUnavailable(status) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_tournament_cancel_unavailable",
            "완료되었거나 이미 취소된 경매 내전은 조기 종료할 수 없습니다.",
            Some(serde_json::json!({ "currentStatus": status.as_str() })),
        ),
        BotWorkflowError::AuctionTournamentCancelMmrHistoryAdvanced => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_tournament_cancel_mmr_history_advanced",
            "이후 MMR 변경이 있어 경매 내전 전체를 안전하게 취소할 수 없습니다.",
            None,
        ),
        BotWorkflowError::IdempotencyConflict => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_conflict",
            "같은 Idempotency-Key가 다른 경매 내전 조기 종료 요청에 사용되었습니다.",
            None,
        ),
        BotWorkflowError::IdempotencyInProgress => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_in_progress",
            "같은 경매 내전 조기 종료 요청을 처리하고 있습니다.",
            None,
        ),
        error => command_error(error, request_id),
    }
}

fn web_auction_match_lineup_error(error: BotWorkflowError, request_id: &RequestId) -> ApiError {
    match error {
        BotWorkflowError::InvalidCommand(_) => {
            invalid_browser_auction_match_lineup_request(request_id)
        }
        BotWorkflowError::MembershipRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "guild_membership_required",
            "해당 길드의 활성 멤버만 경매 경기 라인업을 확인하거나 변경할 수 있습니다.",
            None,
        ),
        BotWorkflowError::OperatorRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "operator_required",
            "경매 경기 라인업을 변경하려면 현재 길드 운영자 권한이 필요합니다.",
            None,
        ),
        BotWorkflowError::AuctionMatchNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_id,
            "auction_match_not_found",
            "요청한 경매 경기를 찾을 수 없습니다.",
            None,
        ),
        BotWorkflowError::AuctionMatchTeamsPending => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_match_teams_pending",
            "양 팀이 확정된 뒤에만 이 경기를 편성할 수 있습니다.",
            None,
        ),
        BotWorkflowError::AuctionMatchTournamentNotReady => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_match_tournament_not_ready",
            "대진이 확정된 진행 중 경매 내전에서만 라인업을 변경할 수 있습니다.",
            None,
        ),
        BotWorkflowError::AuctionMatchRevisionConflict { expected, actual } => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_match_revision_conflict",
            "다른 운영자가 경매 경기 라인업을 변경했습니다. 최신 상태를 확인해 주세요.",
            Some(serde_json::json!({ "expectedRevision": expected, "actualRevision": actual })),
        ),
        BotWorkflowError::AuctionMatchLineupNotEditable(status) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_match_lineup_not_editable",
            "라인업 편성 대기 상태에서만 이 작업을 수행할 수 있습니다.",
            Some(serde_json::json!({ "currentStatus": status })),
        ),
        BotWorkflowError::AuctionMatchLineupLocked => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_match_lineup_locked",
            "이 경기는 이미 라인업이 확정되어 변경할 수 없습니다.",
            None,
        ),
        BotWorkflowError::AuctionMatchLineupUnlockUnavailable(status) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_match_lineup_unlock_unavailable",
            "게임 결과를 입력하기 전 확정된 라인업만 다시 편성할 수 있습니다.",
            Some(serde_json::json!({ "currentStatus": status })),
        ),
        BotWorkflowError::AuctionMatchLineupResultArtifactsPresent => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_match_lineup_result_artifacts_present",
            "게임 입력 또는 결과 기록이 있어 라인업 확정을 해제할 수 없습니다.",
            None,
        ),
        BotWorkflowError::AuctionMatchLineupTeamNotFound => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "auction_match_lineup_team_not_found",
            "선택한 팀은 이 경기에 배정되어 있지 않습니다.",
            None,
        ),
        BotWorkflowError::AuctionMatchLineupMemberNotFound => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "auction_match_lineup_member_not_found",
            "선택한 선수는 해당 팀의 활성 roster에 없습니다.",
            None,
        ),
        BotWorkflowError::AuctionMatchLineupPlayerAssigned => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_match_lineup_player_assigned",
            "같은 선수는 경매 경기 라인업의 한 라인에만 배정할 수 있습니다.",
            None,
        ),
        BotWorkflowError::AuctionMatchLineupIncomplete => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_match_lineup_incomplete",
            "양 팀의 TOP, JUNGLE, MID, BOTTOM, SUPPORT를 모두 배정해야 라인업을 확정할 수 있습니다.",
            None,
        ),
        BotWorkflowError::IdempotencyConflict => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_conflict",
            "같은 Idempotency-Key가 다른 경매 경기 라인업 요청에 사용되었습니다.",
            None,
        ),
        BotWorkflowError::IdempotencyInProgress => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_in_progress",
            "같은 경매 경기 라인업 요청을 처리하고 있습니다.",
            None,
        ),
        error => command_error(error, request_id),
    }
}

fn web_auction_match_game_error(error: BotWorkflowError, request_id: &RequestId) -> ApiError {
    match error {
        BotWorkflowError::InvalidCommand(_) | BotWorkflowError::InvalidGameResult => {
            invalid_browser_auction_match_game_request(request_id)
        }
        BotWorkflowError::MembershipRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "guild_membership_required",
            "해당 길드의 활성 멤버만 경매 경기 결과를 확인하거나 변경할 수 있습니다.",
            None,
        ),
        BotWorkflowError::OperatorRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "operator_required",
            "경매 경기 결과를 변경하려면 현재 길드 운영자 권한이 필요합니다.",
            None,
        ),
        BotWorkflowError::AuctionMatchNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_id,
            "auction_match_not_found",
            "요청한 경매 경기를 찾을 수 없습니다.",
            None,
        ),
        BotWorkflowError::AuctionMatchTeamsPending => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_match_teams_pending",
            "양 팀이 확정된 뒤에만 이 경기의 결과를 입력할 수 있습니다.",
            None,
        ),
        BotWorkflowError::AuctionMatchTournamentNotReady => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_match_tournament_not_ready",
            "대진이 확정된 진행 중 경매 내전에서만 경기 결과를 변경할 수 있습니다.",
            None,
        ),
        BotWorkflowError::AuctionMatchSemifinalLineupsIncomplete => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_match_semifinal_lineups_incomplete",
            "20인 경매 내전은 두 준결승의 라인업을 모두 확정한 뒤 세트 결과를 입력할 수 있습니다.",
            None,
        ),
        BotWorkflowError::AuctionMatchRevisionConflict { expected, actual } => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_match_revision_conflict",
            "다른 운영자가 경매 경기 결과를 변경했습니다. 최신 상태를 확인해 주세요.",
            Some(serde_json::json!({ "expectedRevision": expected, "actualRevision": actual })),
        ),
        BotWorkflowError::AuctionMatchResultNotEditable(status) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_match_result_not_editable",
            "현재 경기 상태에서는 세트 결과를 입력하거나 되돌릴 수 없습니다.",
            Some(serde_json::json!({ "currentStatus": status })),
        ),
        BotWorkflowError::AuctionMatchSetNotCurrent { expected, actual } => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_match_set_not_current",
            "현재 진행 중인 세트만 기록할 수 있습니다. 최신 상태를 확인해 주세요.",
            Some(serde_json::json!({ "expectedSetNumber": expected, "actualSetNumber": actual })),
        ),
        BotWorkflowError::AuctionMatchGameChampionDuplicate { champion_id } => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "duplicate_auction_match_game_champion",
            "같은 세트에서 픽과 밴은 같은 챔피언을 중복할 수 없습니다.",
            Some(serde_json::json!({ "championId": champion_id })),
        ),
        BotWorkflowError::AuctionMatchHardFearlessChampionUsed { champion_id } => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "auction_match_hard_fearless_champion_used",
            "하드 피어리스에서는 이 경기의 이전 세트에서 픽한 챔피언을 다시 픽할 수 없습니다.",
            Some(serde_json::json!({ "championId": champion_id })),
        ),
        BotWorkflowError::AuctionMatchNoConfirmedGames => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_match_no_confirmed_games",
            "되돌릴 확정 세트가 없습니다.",
            None,
        ),
        BotWorkflowError::AuctionMatchGameMmrHistoryAdvanced => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_match_game_reversal_not_safe",
            "이후 MMR 변경이 있어 이 세트 결과를 안전하게 되돌릴 수 없습니다.",
            None,
        ),
        BotWorkflowError::AuctionMatchDownstreamWorkExists => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_match_downstream_work_exists",
            "결승의 라인업 또는 게임 기록이 있어 준결승 결과를 되돌릴 수 없습니다.",
            None,
        ),
        BotWorkflowError::AuctionTournamentResultDeliveryInProgress => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_tournament_result_delivery_in_progress",
            "결과 메시지를 Discord에 전송 중이라 지금은 결과를 되돌릴 수 없습니다. 잠시 후 다시 시도해 주세요.",
            None,
        ),
        BotWorkflowError::AuctionTournamentResultDeliveryUncertain => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "auction_tournament_result_delivery_unresolved",
            "Discord 결과 메시지의 전송 여부를 확인할 수 없어 결과를 되돌릴 수 없습니다. 운영 로그에서 전송 상태를 확인해 주세요.",
            None,
        ),
        BotWorkflowError::IdempotencyConflict => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_conflict",
            "같은 Idempotency-Key가 다른 경매 경기 결과 요청에 사용되었습니다.",
            None,
        ),
        BotWorkflowError::IdempotencyInProgress => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "idempotency_in_progress",
            "같은 경매 경기 결과 요청을 처리하고 있습니다.",
            None,
        ),
        error => web_auction_match_lineup_error(error, request_id),
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
        BotWorkflowError::RecruitmentRestoreUnavailable(status) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "recruitment_restore_unavailable",
            "취소된 모집만 복구할 수 있습니다.",
            Some(serde_json::json!({ "currentStatus": status.as_str() })),
        ),
        error => command_error(error, request_id),
    }
}

fn web_participant_confirmation_dm_error(
    error: BotWorkflowError,
    request_id: &RequestId,
) -> ApiError {
    match error {
        BotWorkflowError::MembershipRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "guild_membership_required",
            "해당 길드의 활성 멤버만 참가 확정 DM을 발송할 수 있습니다.",
            None,
        ),
        BotWorkflowError::OperatorRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "operator_required",
            "참가 확정 DM 발송에는 현재 길드 운영자 권한이 필요합니다.",
            None,
        ),
        BotWorkflowError::RecruitmentNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_id,
            "recruitment_not_found",
            "요청한 모집을 찾을 수 없습니다.",
            None,
        ),
        BotWorkflowError::ParticipantConfirmationDmUnavailable(status) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "participant_confirmation_dm_unavailable",
            "마감된 모집에서만 참가 확정 DM을 발송할 수 있습니다.",
            Some(serde_json::json!({ "currentStatus": status.as_str() })),
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
            "같은 참가 확정 DM 요청을 처리하고 있습니다.",
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

fn web_entry_auction_conversion_error(error: BotWorkflowError, request_id: &RequestId) -> ApiError {
    match error {
        BotWorkflowError::InvalidCommand(_) => {
            invalid_browser_entry_auction_conversion_request(request_id)
        }
        BotWorkflowError::MembershipRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "guild_membership_required",
            "해당 길드의 활성 멤버만 엔트리를 경매 모집으로 전환할 수 있습니다.",
            None,
        ),
        BotWorkflowError::OperatorRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "operator_required",
            "엔트리를 경매 모집으로 전환하려면 현재 길드 운영자 권한이 필요합니다.",
            None,
        ),
        BotWorkflowError::RecruitmentRevisionConflict { expected, actual } => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "recruitment_revision_conflict",
            "모집 상태가 변경되었습니다. 최신 상태를 확인해 주세요.",
            Some(serde_json::json!({ "expectedRevision": expected, "actualRevision": actual })),
        ),
        BotWorkflowError::EntryDraftAuctionSourceNotReady(status) => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "entry_draft_auction_source_not_ready",
            "마감된 일반 엔트리 모집만 경매 모집으로 전환할 수 있습니다.",
            Some(serde_json::json!({ "currentStatus": status.as_str() })),
        ),
        BotWorkflowError::EntryDraftAlreadyConvertedToSeries => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "entry_draft_already_converted_to_series",
            "이미 일반 시리즈로 전환된 모집은 경매 모집으로 전환할 수 없습니다.",
            None,
        ),
        BotWorkflowError::EntryDraftIncomplete {
            joined,
            assigned,
            target,
        } => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "entry_draft_incomplete",
            "참가자 10명을 채운 뒤 경매 모집으로 전환할 수 있습니다.",
            Some(serde_json::json!({
                "joinedCount": joined,
                "assignedCount": assigned,
                "targetCount": target,
            })),
        ),
        BotWorkflowError::RecruitmentChannelNotConfigured => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "recruitment_channel_not_configured",
            "Discord 모집 채널을 먼저 설정해야 경매 모집으로 전환할 수 있습니다.",
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
            "같은 엔트리 경매 전환 요청을 처리하고 있습니다.",
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

    #[test]
    fn auction_recruitment_create_request_uses_flat_optional_rules() {
        let request =
            serde_json::from_value::<BrowserCreateAuctionRecruitmentRequest>(serde_json::json!({
                "format": "TWENTY_PERSON",
                "semifinalBestOf": 1,
                "finalBestOf": 5,
                "hardFearless": true,
                "expectedStartAt": "2026-07-26T11:00:00Z",
            }))
            .expect("flat auction create request should deserialize");

        assert_eq!(request.format, AuctionFormat::TwentyPerson);
        assert_eq!(request.semifinal_best_of, Some(1));
        assert_eq!(request.final_best_of, Some(5));
        assert_eq!(request.hard_fearless, Some(true));
        assert_eq!(
            request.expected_start_at,
            Some(serde_json::json!("2026-07-26T11:00:00Z"))
        );
        assert!(
            serde_json::from_value::<BrowserCreateAuctionRecruitmentRequest>(serde_json::json!({
                "format": "TEN_PERSON",
                "rules": { "finalBestOf": 3 },
            }),)
            .is_err()
        );
    }

    #[tokio::test]
    async fn expected_start_at_has_a_specific_validation_error() {
        use axum::{body::to_bytes, response::IntoResponse};

        let request_id = RequestId("test-request".to_owned());
        let invalid_value = serde_json::json!("2026-07-26T20:00:00");
        let response = parse_expected_start_at(Some(&invalid_value), &request_id)
            .expect_err("a timezone-less expected start must be rejected")
            .into_response();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("error body should be readable");
        let body: Value = serde_json::from_slice(&body).expect("error body should be JSON");
        assert_eq!(body["error"]["code"], "invalid_expected_start_at");
        assert_eq!(
            body["error"]["message"],
            "예상 시작 시간은 시간대가 포함된 ISO 8601 형식이어야 합니다."
        );
        assert_eq!(body["error"]["details"]["field"], "expectedStartAt");
    }

    #[test]
    fn missing_or_blank_expected_start_defaults_to_thirty_minutes_from_now() {
        let request_id = RequestId("test-request".to_owned());
        let lower_bound = OffsetDateTime::now_utc() + Duration::minutes(30);
        let missing = parse_expected_start_at(None, &request_id)
            .expect("missing expected start should use the default");
        let blank = parse_expected_start_at(Some(&serde_json::json!("  ")), &request_id)
            .expect("blank expected start should use the default");
        let upper_bound = OffsetDateTime::now_utc() + Duration::minutes(30);

        assert!(missing >= lower_bound && missing <= upper_bound);
        assert!(blank >= lower_bound && blank <= upper_bound);
    }

    #[tokio::test]
    async fn auction_match_game_gate_explains_missing_semifinal_lineups() {
        use axum::{body::to_bytes, response::IntoResponse};

        let request_id = RequestId("test-request".to_owned());
        let response = web_auction_match_game_error(
            BotWorkflowError::AuctionMatchSemifinalLineupsIncomplete,
            &request_id,
        )
        .into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("error body should be readable");
        let body: Value = serde_json::from_slice(&body).expect("error body should be JSON");
        assert_eq!(
            body["error"]["code"],
            "auction_match_semifinal_lineups_incomplete"
        );
        assert_eq!(
            body["error"]["message"],
            "20인 경매 내전은 두 준결승의 라인업을 모두 확정한 뒤 세트 결과를 입력할 수 있습니다."
        );
        assert_eq!(body["error"]["requestId"], "test-request");
    }
}
