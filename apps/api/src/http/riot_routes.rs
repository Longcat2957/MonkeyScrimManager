use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::Response,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
    error::{self, ApiError},
    http::extractors::{AuthenticatedSession, require_csrf},
    integrations::riot::RiotClientError,
    repository::riot::{
        self, NewRiotAccount, RiotAccountRecord, UpdatedPrivacySettings, UserPrivacySettings,
    },
    state::{AppState, RequestId},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RiotRecordsQuery {
    query: String,
    region: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RiotRefreshRequest {
    query: String,
    region: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RiotAccountCreateRequest {
    puuid: String,
    platform: String,
    routing_region: String,
    game_name: String,
    tag_line: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PrivacyUpdateRequest {
    public_profile: bool,
    show_riot_ids: bool,
    allow_external_record_search: bool,
}

#[derive(Serialize)]
struct RiotRecordsData {
    payload: serde_json::Value,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RiotAccountsData {
    riot_accounts: Vec<RiotAccountData>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RiotAccountData {
    puuid: String,
    platform: String,
    routing_region: String,
    game_name: String,
    tag_line: String,
    normalized_game_name: String,
    normalized_tag_line: String,
    profile_icon_id: Option<i32>,
    is_main: bool,
    main_position: Option<String>,
    main_position_updated_at: Option<String>,
    last_synced_at: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PrivacyData {
    public_profile: bool,
    show_riot_ids: bool,
    allow_external_record_search: bool,
}

#[derive(Serialize)]
struct PrivacyUpdatedData {
    settings: PrivacyData,
}

#[derive(Serialize)]
struct SingleRiotAccountData {
    riot_account: RiotAccountData,
}

pub async fn records(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Query(query): Query<RiotRecordsQuery>,
) -> Result<Response, ApiError> {
    let riot = riot_client(&state, &request_id)?;
    let payload = riot
        .records(&query.query, &query.region)
        .await
        .map_err(|error| riot_client_error(error, &request_id))?;

    Ok(error::success(
        StatusCode::OK,
        &request_id,
        RiotRecordsData { payload: payload.0 },
    ))
}

pub async fn refresh_records(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<RiotRefreshRequest>,
) -> Result<Response, ApiError> {
    let riot = riot_client(&state, &request_id)?;
    let payload = riot
        .refresh_records(&request.query, &request.region)
        .await
        .map_err(|error| riot_client_error(error, &request_id))?;

    Ok(error::success(
        StatusCode::OK,
        &request_id,
        RiotRecordsData { payload: payload.0 },
    ))
}

pub async fn list_riot_accounts(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    principal: AuthenticatedSession,
) -> Result<Response, ApiError> {
    let pool = database(&state, &request_id)?;
    let accounts = riot::list_accounts(pool, &principal.session.user_id)
        .await
        .map_err(|error| repository_error(&error, &request_id))?;

    Ok(error::success(
        StatusCode::OK,
        &request_id,
        RiotAccountsData {
            riot_accounts: accounts.into_iter().map(map_riot_account).collect(),
        },
    ))
}

pub async fn create_riot_account(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(request): Json<RiotAccountCreateRequest>,
) -> Result<Response, ApiError> {
    require_csrf(&headers, &state, &principal, &request_id)?;
    let pool = database(&state, &request_id)?;
    let record = riot::add_account(
        pool,
        sanitize_riot_account(
            NewRiotAccount {
                user_id: principal.session.user_id,
                puuid: request.puuid,
                platform: request.platform,
                routing_region: request.routing_region,
                game_name: request.game_name,
                tag_line: request.tag_line,
            },
            &request_id,
        )?,
    )
    .await
    .map_err(|error| repository_error(&error, &request_id))?;

    Ok(error::success(
        StatusCode::OK,
        &request_id,
        SingleRiotAccountData {
            riot_account: map_riot_account(record),
        },
    ))
}

pub async fn set_main_account(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Path(puuid): Path<String>,
) -> Result<Response, ApiError> {
    require_csrf(&headers, &state, &principal, &request_id)?;
    let pool = database(&state, &request_id)?;
    let record = riot::set_main_account(pool, &principal.session.user_id, &puuid)
        .await
        .map_err(|error| repository_error(&error, &request_id))?
        .ok_or_else(|| record_not_found(&request_id))?;

    Ok(error::success(
        StatusCode::OK,
        &request_id,
        SingleRiotAccountData {
            riot_account: map_riot_account(record),
        },
    ))
}

pub async fn delete_riot_account(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Path(puuid): Path<String>,
) -> Result<Response, ApiError> {
    require_csrf(&headers, &state, &principal, &request_id)?;
    let pool = database(&state, &request_id)?;
    let deleted = riot::delete_account(pool, &principal.session.user_id, &puuid)
        .await
        .map_err(|error| repository_error(&error, &request_id))?;

    if let Some(deleted) = deleted {
        Ok(error::success(
            StatusCode::OK,
            &request_id,
            SingleRiotAccountData {
                riot_account: map_riot_account(deleted),
            },
        ))
    } else {
        Err(record_not_found(&request_id))
    }
}

pub async fn refresh_riot_account(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Path(puuid): Path<String>,
) -> Result<Response, ApiError> {
    require_csrf(&headers, &state, &principal, &request_id)?;
    let pool = database(&state, &request_id)?;
    let account = riot::get_account(pool, &principal.session.user_id, &puuid)
        .await
        .map_err(|error| repository_error(&error, &request_id))?
        .ok_or_else(|| record_not_found(&request_id))?;

    let riot = riot_client(&state, &request_id)?;
    let _ = riot
        .refresh_riot_account(&account.puuid, &account.platform, &account.routing_region)
        .await
        .map_err(|error| riot_client_error(error, &request_id))?;

    let updated = riot::touch_last_synced_at(pool, &principal.session.user_id, &account.puuid)
        .await
        .map_err(|error| repository_error(&error, &request_id))?
        .ok_or_else(|| record_not_found(&request_id))?;

    Ok(error::success(
        StatusCode::OK,
        &request_id,
        SingleRiotAccountData {
            riot_account: map_riot_account(updated),
        },
    ))
}

pub async fn privacy(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    principal: AuthenticatedSession,
) -> Result<Response, ApiError> {
    let pool = database(&state, &request_id)?;
    let settings = riot::get_privacy_settings(pool, &principal.session.user_id)
        .await
        .map_err(|error| repository_error(&error, &request_id))?;

    Ok(error::success(
        StatusCode::OK,
        &request_id,
        PrivacyUpdatedData {
            settings: map_privacy(settings),
        },
    ))
}

pub async fn update_privacy(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(request): Json<PrivacyUpdateRequest>,
) -> Result<Response, ApiError> {
    require_csrf(&headers, &state, &principal, &request_id)?;
    let pool = database(&state, &request_id)?;
    let settings = riot::upsert_privacy_settings(
        pool,
        &principal.session.user_id,
        UpdatedPrivacySettings {
            public_profile_enabled: request.public_profile,
            show_riot_ids: request.show_riot_ids,
            allow_external_record_search: request.allow_external_record_search,
        },
    )
    .await
    .map_err(|error| repository_error(&error, &request_id))?;

    Ok(error::success(
        StatusCode::OK,
        &request_id,
        PrivacyUpdatedData {
            settings: map_privacy(settings),
        },
    ))
}

fn riot_client<'a>(
    state: &'a AppState,
    request_id: &RequestId,
) -> Result<&'a dyn crate::integrations::riot::RiotClient, ApiError> {
    state
        .riot()
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                request_id,
                "riot_service_unavailable",
                "Riot 검색 서비스를 사용할 수 없습니다.",
                None,
            )
        })
        .map(|client| &**client)
}

fn database<'a>(state: &'a AppState, request_id: &RequestId) -> Result<&'a PgPool, ApiError> {
    state.postgres().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            request_id,
            "database_unavailable",
            "데이터베이스를 사용할 수 없습니다.",
            None,
        )
    })
}

fn sanitize_riot_account(
    request: NewRiotAccount,
    request_id: &RequestId,
) -> Result<NewRiotAccount, ApiError> {
    let puuid = sanitize_token(request.puuid, "puuid", request_id)?;
    let platform = sanitize_token(request.platform, "platform", request_id)?;
    let routing_region = sanitize_token(request.routing_region, "routingRegion", request_id)?;
    let game_name = sanitize_display(request.game_name, "gameName", request_id)?;
    let tag_line = sanitize_display(request.tag_line, "tagLine", request_id)?;
    Ok(NewRiotAccount {
        user_id: request.user_id,
        puuid,
        platform,
        routing_region,
        game_name,
        tag_line,
    })
}

fn sanitize_token(value: String, field: &str, request_id: &RequestId) -> Result<String, ApiError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > 64 || trimmed.contains(' ') {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "invalid_riot_field",
            "Riot 필드 값이 올바르지 않습니다.",
            Some(serde_json::json!({ "field": field })),
        ));
    }
    Ok(trimmed.to_uppercase())
}

fn sanitize_display(
    value: String,
    field: &str,
    request_id: &RequestId,
) -> Result<String, ApiError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > 128 {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "invalid_riot_field",
            "Riot 필드 값이 올바르지 않습니다.",
            Some(serde_json::json!({ "field": field })),
        ));
    }
    Ok(trimmed.to_owned())
}

fn map_riot_account(record: RiotAccountRecord) -> RiotAccountData {
    RiotAccountData {
        puuid: record.puuid,
        platform: record.platform,
        routing_region: record.routing_region,
        game_name: record.game_name,
        tag_line: record.tag_line,
        normalized_game_name: record.normalized_game_name,
        normalized_tag_line: record.normalized_tag_line,
        profile_icon_id: record.profile_icon_id,
        is_main: record.is_main,
        main_position: record.main_position,
        main_position_updated_at: record
            .main_position_updated_at
            .and_then(|value| to_rfc3339(value).ok()),
        last_synced_at: record
            .last_synced_at
            .and_then(|value| to_rfc3339(value).ok()),
        created_at: to_rfc3339(record.created_at).unwrap_or_default(),
        updated_at: to_rfc3339(record.updated_at).unwrap_or_default(),
    }
}

fn map_privacy(settings: UserPrivacySettings) -> PrivacyData {
    PrivacyData {
        public_profile: settings.public_profile_enabled,
        show_riot_ids: settings.show_riot_ids,
        allow_external_record_search: settings.allow_external_record_search,
    }
}

fn to_rfc3339(value: OffsetDateTime) -> time::Result<String> {
    Ok(value.format(&Rfc3339)?)
}

fn riot_client_error(error: RiotClientError, request_id: &RequestId) -> ApiError {
    match error {
        RiotClientError::InvalidRequest => ApiError::new(
            StatusCode::BAD_REQUEST,
            request_id,
            "riot_invalid_request",
            "Riot 검색 요청이 올바르지 않습니다.",
            None,
        ),
        RiotClientError::InvalidUrl => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            request_id,
            "riot_client_misconfigured",
            "Riot 검색 서비스 URL 설정이 잘못되었습니다.",
            None,
        ),
        RiotClientError::Request(_) => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            request_id,
            "riot_client_unavailable",
            "Riot 검색 서비스에 일시적으로 연결할 수 없습니다.",
            None,
        ),
        RiotClientError::MalformedPayload => ApiError::new(
            StatusCode::BAD_GATEWAY,
            request_id,
            "riot_client_invalid_payload",
            "Riot 검색 응답 형식이 올바르지 않습니다.",
            None,
        ),
        RiotClientError::Rejected { status, endpoint } => {
            let (status, code, message) = match status {
                400 => (
                    StatusCode::BAD_REQUEST,
                    "riot_client_bad_request",
                    "Riot 검색 요청이 거부되었습니다.",
                ),
                401 | 403 => (
                    StatusCode::BAD_GATEWAY,
                    "riot_client_authorization_failed",
                    "Riot 검색 서비스 인증에 실패했습니다.",
                ),
                404 => (
                    StatusCode::NOT_FOUND,
                    "riot_lookup_not_found",
                    "Riot 검색 대상이 없습니다.",
                ),
                429 => (
                    StatusCode::TOO_MANY_REQUESTS,
                    "riot_client_rate_limited",
                    "Riot 검색 요청이 제한되었습니다.",
                ),
                500..=599 => (
                    StatusCode::BAD_GATEWAY,
                    "riot_client_server_error",
                    "Riot 검색 서비스가 일시적으로 불안정합니다.",
                ),
                _ => (
                    StatusCode::BAD_GATEWAY,
                    "riot_client_rejected",
                    "Riot 검색 요청이 거부되었습니다.",
                ),
            };
            let _ = endpoint;
            ApiError::new(
                status,
                request_id,
                code,
                message,
                Some(serde_json::json!({ "httpStatus": status.as_u16() })),
            )
        }
    }
}

fn repository_error(error: &riot::RiotRepositoryError, request_id: &RequestId) -> ApiError {
    match error {
        riot::RiotRepositoryError::DuplicateAccount => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "duplicate_riot_account",
            "이미 등록된 Riot 계정입니다.",
            None,
        ),
        riot::RiotRepositoryError::InvalidInput => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            request_id,
            "invalid_riot_account",
            "Riot 계정 입력이 올바르지 않습니다.",
            None,
        ),
        riot::RiotRepositoryError::InvalidAccountData => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            request_id,
            "riot_repository_invalid_data",
            "Riot 계정 데이터가 올바르지 않습니다.",
            None,
        ),
        riot::RiotRepositoryError::Sql(source) => error::database_failure(source, request_id),
    }
}

fn record_not_found(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        request_id,
        "riot_account_not_found",
        "요청한 Riot 계정을 찾을 수 없습니다.",
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_token_removes_whitespace_around_value() {
        let request_id = RequestId("request-id".to_owned());
        assert_eq!(
            sanitize_token(" KR ".to_owned(), "platform", &request_id)
                .expect("ok")
                .as_str(),
            "KR"
        );
        assert!(sanitize_token("  ".to_owned(), "platform", &request_id).is_err());
    }
}
