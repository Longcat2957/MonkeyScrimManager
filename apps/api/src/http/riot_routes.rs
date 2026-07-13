use std::str::FromStr as _;

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
    integrations::riot::{
        ResolvedRiotAccount, RiotCacheState, RiotClientError, RiotPlatform, RiotRecentMatchesPage,
        RiotRecords, RiotService,
    },
    repository::riot::{
        self, CanonicalRiotAccount, RiotAccountRecord, UpdatedPrivacySettings, UserPrivacySettings,
    },
    repository::riot_records::{
        self as records_repository, AcquireRiotRecordsRefresh, CachedRiotRecords,
        CanonicalRiotRecordsIdentity, RIOT_RECORDS_SNAPSHOT_SCHEMA_VERSION, RiotRecordsLookup,
        RiotRecordsRepositoryError,
    },
    state::{AppState, RequestId},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RiotRecordsQuery {
    game_name: String,
    tag_line: String,
    platform: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RiotRecordsRequest {
    game_name: String,
    tag_line: String,
    platform: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RiotRecentMatchesQuery {
    game_name: String,
    tag_line: String,
    platform: String,
    #[serde(default)]
    offset: usize,
    #[serde(default = "default_recent_match_count")]
    count: usize,
}

const fn default_recent_match_count() -> usize {
    10
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RiotAccountCreateRequest {
    game_name: String,
    tag_line: String,
    platform: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PrivacyUpdateRequest {
    public_profile: bool,
    show_riot_ids: bool,
    allow_external_record_search: bool,
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
    profile_icon_id: Option<i32>,
    is_main: bool,
    main_position: Option<String>,
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

pub async fn records(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Query(query): Query<RiotRecordsQuery>,
) -> Result<Response, ApiError> {
    records_response(
        &state,
        &request_id,
        &query.game_name,
        &query.tag_line,
        &query.platform,
        false,
    )
    .await
}

pub async fn refresh_records(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(request): Json<RiotRecordsRequest>,
) -> Result<Response, ApiError> {
    require_csrf(&headers, &state, &principal, &request_id)?;
    records_response(
        &state,
        &request_id,
        &request.game_name,
        &request.tag_line,
        &request.platform,
        true,
    )
    .await
}

pub async fn recent_matches(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Query(query): Query<RiotRecentMatchesQuery>,
) -> Result<Response, ApiError> {
    let platform = parse_platform(&query.platform, &request_id)?;
    if query.count == 0 || query.count > 20 || query.offset > 10_000 {
        return Err(invalid_records_request(&request_id));
    }
    let lookup = records_lookup(platform, &query.game_name, &query.tag_line, &request_id)?;
    let pool = database(&state, &request_id)?;
    let cached_snapshot = match records_repository::get_snapshot(pool, &lookup)
        .await
        .map_err(|source| records_repository_error(&source, &request_id))?
    {
        Some(snapshot) => snapshot,
        None => {
            let _ = load_records(&state, &request_id, &lookup, platform, false).await?;
            records_repository::get_snapshot(pool, &lookup)
                .await
                .map_err(|source| records_repository_error(&source, &request_id))?
                .ok_or_else(|| internal(&request_id))?
        }
    };

    if let Some(cached) =
        records_repository::get_recent_page(pool, &lookup, query.offset as i32, query.count as i32)
            .await
            .map_err(|source| records_repository_error(&source, &request_id))?
    {
        let page: RiotRecentMatchesPage = cached.decode().map_err(|_| internal(&request_id))?;
        validate_recent_page(
            &page,
            &cached_snapshot,
            platform,
            query.offset,
            query.count,
            &request_id,
        )?;
        return Ok(error::success(StatusCode::OK, &request_id, page));
    }

    let page = riot_service(&state, &request_id)?
        .recent_matches_page(
            lookup.game_name(),
            lookup.tag_line(),
            platform,
            query.offset,
            query.count,
        )
        .await
        .map_err(|source| riot_client_error(source, &request_id))?;
    validate_recent_page(
        &page,
        &cached_snapshot,
        platform,
        query.offset,
        query.count,
        &request_id,
    )?;
    records_repository::store_recent_page(
        pool,
        &lookup,
        query.offset as i32,
        query.count as i32,
        &page,
    )
    .await
    .map_err(|source| records_repository_error(&source, &request_id))?;
    Ok(error::success(StatusCode::OK, &request_id, page))
}

async fn records_response(
    state: &AppState,
    request_id: &RequestId,
    game_name: &str,
    tag_line: &str,
    platform: &str,
    refresh: bool,
) -> Result<Response, ApiError> {
    let platform = parse_platform(platform, request_id)?;
    let lookup = records_lookup(platform, game_name, tag_line, request_id)?;
    let records = load_records(state, request_id, &lookup, platform, refresh).await?;
    Ok(error::success(StatusCode::OK, request_id, records))
}

async fn load_records(
    state: &AppState,
    request_id: &RequestId,
    lookup: &RiotRecordsLookup,
    platform: RiotPlatform,
    refresh: bool,
) -> Result<RiotRecords, ApiError> {
    let pool = database(state, request_id)?;
    let riot = riot_service(state, request_id)?;
    let cached_snapshot = records_repository::get_snapshot(pool, lookup)
        .await
        .map_err(|source| records_repository_error(&source, request_id))?;
    if !refresh && let Some(cached) = cached_snapshot.clone() {
        return decode_cached_records(cached, platform, request_id);
    }

    match records_repository::acquire_refresh(pool, lookup)
        .await
        .map_err(|source| records_repository_error(&source, request_id))?
    {
        AcquireRiotRecordsRefresh::Acquired(lease) => {
            let provider_result = riot
                .records(lookup.game_name(), lookup.tag_line(), platform, refresh)
                .await;
            let records = match provider_result {
                Ok(records) => records,
                Err(source) => {
                    let _ = records_repository::abort_refresh(pool, &lease).await;
                    if let Some(cached) = cached_snapshot {
                        let mut records = decode_cached_records(cached, platform, request_id)?;
                        records.cache.state = RiotCacheState::Stale;
                        return Ok(records);
                    }
                    return Err(riot_client_error(source, request_id));
                }
            };
            let identity = match CanonicalRiotRecordsIdentity::new(
                records.platform.as_str(),
                &records.account.puuid,
                &records.account.game_name,
                &records.account.tag_line,
            ) {
                Ok(identity) => identity,
                Err(source) => {
                    let _ = records_repository::abort_refresh(pool, &lease).await;
                    return Err(records_repository_error(&source, request_id));
                }
            };
            if let Err(source) =
                records_repository::complete_refresh(pool, &lease, &identity, &records).await
            {
                let _ = records_repository::abort_refresh(pool, &lease).await;
                return Err(records_repository_error(&source, request_id));
            }
            Ok(records)
        }
        AcquireRiotRecordsRefresh::Cooldown { snapshot, retry_at } => {
            if refresh {
                Err(refresh_wait_error(
                    request_id,
                    "riot_records_refresh_cooldown",
                    "전적은 30초마다 갱신할 수 있습니다.",
                    retry_at,
                ))
            } else {
                decode_cached_records(snapshot, platform, request_id)
            }
        }
        AcquireRiotRecordsRefresh::InProgress { snapshot, retry_at } => {
            if !refresh && let Some(snapshot) = snapshot {
                return decode_cached_records(snapshot, platform, request_id);
            }
            Err(refresh_wait_error(
                request_id,
                "riot_records_refresh_in_progress",
                "다른 전적 갱신 요청을 처리하고 있습니다.",
                retry_at,
            ))
        }
    }
}

fn records_lookup(
    platform: RiotPlatform,
    game_name: &str,
    tag_line: &str,
    request_id: &RequestId,
) -> Result<RiotRecordsLookup, ApiError> {
    RiotRecordsLookup::new(platform.as_str(), game_name, tag_line)
        .map_err(|_| invalid_records_request(request_id))
}

fn decode_cached_records(
    cached: CachedRiotRecords,
    platform: RiotPlatform,
    request_id: &RequestId,
) -> Result<RiotRecords, ApiError> {
    if cached.schema_version != RIOT_RECORDS_SNAPSHOT_SCHEMA_VERSION {
        return Err(internal(request_id));
    }
    let mut records: RiotRecords = cached.decode().map_err(|_| internal(request_id))?;
    records
        .validate(platform)
        .map_err(|_| internal(request_id))?;
    records.cache.state = RiotCacheState::Fresh;
    Ok(records)
}

fn validate_recent_page(
    page: &RiotRecentMatchesPage,
    cached: &CachedRiotRecords,
    platform: RiotPlatform,
    offset: usize,
    count: usize,
    request_id: &RequestId,
) -> Result<(), ApiError> {
    page.validate(platform, offset, count)
        .map_err(|_| internal(request_id))?;
    if page.account.puuid != cached.puuid() {
        return Err(internal(request_id));
    }
    Ok(())
}

fn refresh_wait_error(
    request_id: &RequestId,
    code: &'static str,
    message: &'static str,
    retry_at: OffsetDateTime,
) -> ApiError {
    let retry_after_seconds = (retry_at - OffsetDateTime::now_utc())
        .whole_seconds()
        .max(1);
    ApiError::new(
        StatusCode::TOO_MANY_REQUESTS,
        request_id,
        code,
        message,
        Some(serde_json::json!({ "retryAfterSeconds": retry_after_seconds })),
    )
}

fn invalid_records_request(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        request_id,
        "invalid_riot_records_request",
        "Riot 전적 조회 조건이 올바르지 않습니다.",
        None,
    )
}

pub async fn list_riot_accounts(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    principal: AuthenticatedSession,
) -> Result<Response, ApiError> {
    let accounts = riot::list_accounts(database(&state, &request_id)?, &principal.session.user_id)
        .await
        .map_err(|source| repository_error(&source, &request_id))?;
    account_snapshot_response(StatusCode::OK, &request_id, accounts)
}

pub async fn create_riot_account(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Json(request): Json<RiotAccountCreateRequest>,
) -> Result<Response, ApiError> {
    require_csrf(&headers, &state, &principal, &request_id)?;
    let platform = parse_platform(&request.platform, &request_id)?;

    // The external lookup completes before the repository starts its transaction.
    let resolved = riot_service(&state, &request_id)?
        .resolve_account(&request.game_name, &request.tag_line, platform)
        .await
        .map_err(|source| riot_client_error(source, &request_id))?;
    let canonical = canonical_account(resolved, &request_id)?;
    let accounts = riot::link_account(
        database(&state, &request_id)?,
        &principal.session.user_id,
        &canonical,
    )
    .await
    .map_err(|source| repository_error(&source, &request_id))?;

    account_snapshot_response(StatusCode::CREATED, &request_id, accounts)
}

pub async fn set_main_account(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Path(puuid): Path<String>,
) -> Result<Response, ApiError> {
    set_main(&state, &request_id, &headers, &principal, &puuid).await
}

async fn set_main(
    state: &AppState,
    request_id: &RequestId,
    headers: &HeaderMap,
    principal: &AuthenticatedSession,
    puuid: &str,
) -> Result<Response, ApiError> {
    require_csrf(headers, state, principal, request_id)?;
    validate_puuid_path(puuid, request_id)?;
    let accounts = riot::set_main_account(
        database(state, request_id)?,
        &principal.session.user_id,
        puuid,
    )
    .await
    .map_err(|source| repository_error(&source, request_id))?;
    account_snapshot_response(StatusCode::OK, request_id, accounts)
}

pub async fn delete_riot_account(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Path(puuid): Path<String>,
) -> Result<Response, ApiError> {
    require_csrf(&headers, &state, &principal, &request_id)?;
    validate_puuid_path(&puuid, &request_id)?;
    let accounts = riot::delete_account(
        database(&state, &request_id)?,
        &principal.session.user_id,
        &puuid,
    )
    .await
    .map_err(|source| repository_error(&source, &request_id))?;
    account_snapshot_response(StatusCode::OK, &request_id, accounts)
}

pub async fn refresh_riot_account(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    Path(puuid): Path<String>,
) -> Result<Response, ApiError> {
    require_csrf(&headers, &state, &principal, &request_id)?;
    validate_puuid_path(&puuid, &request_id)?;
    let pool = database(&state, &request_id)?;
    let stored = riot::get_account(pool, &principal.session.user_id, &puuid)
        .await
        .map_err(|source| repository_error(&source, &request_id))?
        .ok_or_else(|| record_not_found(&request_id))?;
    let platform = RiotPlatform::from_str(&stored.platform).map_err(|_| internal(&request_id))?;

    // Ownership is checked first, then the external call runs without a DB transaction.
    let resolved = riot_service(&state, &request_id)?
        .resolve_account_by_puuid(&puuid, platform)
        .await
        .map_err(|source| riot_client_error(source, &request_id))?;
    let canonical = canonical_account(resolved, &request_id)?;
    let accounts = riot::refresh_account(pool, &principal.session.user_id, &canonical)
        .await
        .map_err(|source| repository_error(&source, &request_id))?;
    account_snapshot_response(StatusCode::OK, &request_id, accounts)
}

pub async fn privacy(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    principal: AuthenticatedSession,
) -> Result<Response, ApiError> {
    let settings =
        riot::get_privacy_settings(database(&state, &request_id)?, &principal.session.user_id)
            .await
            .map_err(|source| repository_error(&source, &request_id))?;
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
    let settings = riot::upsert_privacy_settings(
        database(&state, &request_id)?,
        &principal.session.user_id,
        UpdatedPrivacySettings {
            public_profile_enabled: request.public_profile,
            show_riot_ids: request.show_riot_ids,
            allow_external_record_search: request.allow_external_record_search,
        },
    )
    .await
    .map_err(|source| repository_error(&source, &request_id))?;
    Ok(error::success(
        StatusCode::OK,
        &request_id,
        PrivacyUpdatedData {
            settings: map_privacy(settings),
        },
    ))
}

fn riot_service(state: &AppState, request_id: &RequestId) -> Result<RiotService, ApiError> {
    state.riot().cloned().map(RiotService::new).ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            request_id,
            "riot_unavailable",
            "Riot 검색 서비스를 사용할 수 없습니다.",
            None,
        )
    })
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

fn parse_platform(value: &str, request_id: &RequestId) -> Result<RiotPlatform, ApiError> {
    RiotPlatform::from_str(value).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            request_id,
            "invalid_region",
            "지원하지 않는 Riot 플랫폼입니다.",
            Some(serde_json::json!({ "platform": value })),
        )
    })
}

fn canonical_account(
    resolved: ResolvedRiotAccount,
    request_id: &RequestId,
) -> Result<CanonicalRiotAccount, ApiError> {
    let last_synced_at =
        OffsetDateTime::parse(&resolved.synced_at, &Rfc3339).map_err(|_| internal(request_id))?;
    Ok(CanonicalRiotAccount {
        puuid: resolved.account.puuid,
        platform: resolved.platform.as_str().to_owned(),
        routing_region: resolved.routing_region.as_str().to_owned(),
        game_name: resolved.account.game_name,
        tag_line: resolved.account.tag_line,
        profile_icon_id: resolved.profile_icon_id,
        last_synced_at,
    })
}

fn account_snapshot_response(
    status: StatusCode,
    request_id: &RequestId,
    accounts: Vec<RiotAccountRecord>,
) -> Result<Response, ApiError> {
    let riot_accounts = accounts
        .into_iter()
        .map(|record| map_riot_account(record, request_id))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(error::success(
        status,
        request_id,
        RiotAccountsData { riot_accounts },
    ))
}

fn map_riot_account(
    record: RiotAccountRecord,
    request_id: &RequestId,
) -> Result<RiotAccountData, ApiError> {
    Ok(RiotAccountData {
        puuid: record.puuid,
        platform: record.platform,
        routing_region: record.routing_region,
        game_name: record.game_name,
        tag_line: record.tag_line,
        profile_icon_id: record.profile_icon_id,
        is_main: record.is_main,
        main_position: record.main_position,
        last_synced_at: record
            .last_synced_at
            .map(|value| to_rfc3339(value, request_id))
            .transpose()?,
        created_at: to_rfc3339(record.created_at, request_id)?,
        updated_at: to_rfc3339(record.updated_at, request_id)?,
    })
}

fn map_privacy(settings: UserPrivacySettings) -> PrivacyData {
    PrivacyData {
        public_profile: settings.public_profile_enabled,
        show_riot_ids: settings.show_riot_ids,
        allow_external_record_search: settings.allow_external_record_search,
    }
}

fn to_rfc3339(value: OffsetDateTime, request_id: &RequestId) -> Result<String, ApiError> {
    value.format(&Rfc3339).map_err(|_| internal(request_id))
}

fn validate_puuid_path(value: &str, request_id: &RequestId) -> Result<(), ApiError> {
    let valid = !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
    if valid {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            request_id,
            "invalid_riot_account",
            "Riot 계정 식별자가 올바르지 않습니다.",
            None,
        ))
    }
}

fn riot_client_error(source: RiotClientError, request_id: &RequestId) -> ApiError {
    match source {
        RiotClientError::InvalidInput => ApiError::new(
            StatusCode::BAD_REQUEST,
            request_id,
            "invalid_riot_id",
            "GameName과 TagLine이 올바르지 않습니다.",
            None,
        ),
        RiotClientError::UnsupportedPlatform => ApiError::new(
            StatusCode::BAD_REQUEST,
            request_id,
            "invalid_region",
            "지원하지 않는 Riot 플랫폼입니다.",
            None,
        ),
        RiotClientError::NotFound => record_not_found(request_id),
        RiotClientError::RateLimited {
            retry_after_seconds,
        } => ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            request_id,
            "riot_rate_limited",
            "Riot 조회 요청이 제한되었습니다.",
            retry_after_seconds.map(|seconds| serde_json::json!({ "retryAfterSeconds": seconds })),
        ),
        RiotClientError::Timeout => ApiError::new(
            StatusCode::BAD_GATEWAY,
            request_id,
            "riot_upstream_timeout",
            "Riot 조회 응답 시간이 초과되었습니다.",
            None,
        ),
        RiotClientError::Request(_) => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            request_id,
            "riot_unavailable",
            "Riot 검색 서비스에 연결할 수 없습니다.",
            None,
        ),
        RiotClientError::Rejected { status } => ApiError::new(
            StatusCode::BAD_GATEWAY,
            request_id,
            "riot_upstream_failure",
            "Riot 검색 서비스가 요청을 처리하지 못했습니다.",
            Some(serde_json::json!({ "upstreamStatus": status })),
        ),
        RiotClientError::MalformedResponse => ApiError::new(
            StatusCode::BAD_GATEWAY,
            request_id,
            "riot_upstream_invalid_response",
            "Riot 검색 응답 형식이 올바르지 않습니다.",
            None,
        ),
        RiotClientError::InvalidConfiguration => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            request_id,
            "riot_unavailable",
            "Riot 검색 서비스 설정이 올바르지 않습니다.",
            None,
        ),
    }
}

fn repository_error(source: &riot::RiotRepositoryError, request_id: &RequestId) -> ApiError {
    match source {
        riot::RiotRepositoryError::Sql(source) => error::database_failure(source, request_id),
        riot::RiotRepositoryError::NotFound => record_not_found(request_id),
        riot::RiotRepositoryError::AlreadyLinked => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "riot_account_already_linked",
            "이 Riot 계정은 이미 다른 사용자에게 연결되어 있습니다.",
            None,
        ),
        riot::RiotRepositoryError::IdentityConflict => ApiError::new(
            StatusCode::CONFLICT,
            request_id,
            "riot_id_conflict",
            "동일한 Riot ID가 다른 계정에 연결되어 있습니다.",
            None,
        ),
        riot::RiotRepositoryError::UserNotFound
        | riot::RiotRepositoryError::InvalidCanonicalAccount
        | riot::RiotRepositoryError::InvariantViolation => internal(request_id),
    }
}

fn records_repository_error(
    source: &RiotRecordsRepositoryError,
    request_id: &RequestId,
) -> ApiError {
    match source {
        RiotRecordsRepositoryError::Sql(source) => {
            tracing::error!(error = ?source, "Riot records repository operation failed");
            error::database_failure(source, request_id)
        }
        RiotRecordsRepositoryError::InvalidLookup
        | RiotRecordsRepositoryError::InvalidRecentPageKey => invalid_records_request(request_id),
        RiotRecordsRepositoryError::InvalidCanonicalIdentity
        | RiotRecordsRepositoryError::InvalidSnapshot
        | RiotRecordsRepositoryError::InvalidRecentPage
        | RiotRecordsRepositoryError::SnapshotNotFound
        | RiotRecordsRepositoryError::SnapshotSerialization(_)
        | RiotRecordsRepositoryError::LeaseLost => {
            tracing::error!(error = ?source, "Riot records cache invariant failed");
            internal(request_id)
        }
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
    use super::*;

    #[test]
    fn account_create_contract_rejects_client_identity_fields() {
        let valid = serde_json::from_value::<RiotAccountCreateRequest>(serde_json::json!({
            "gameName": "Longcat",
            "tagLine": "KR1",
            "platform": "kr"
        }));
        assert!(valid.is_ok());

        let injected = serde_json::from_value::<RiotAccountCreateRequest>(serde_json::json!({
            "gameName": "Longcat",
            "tagLine": "KR1",
            "platform": "kr",
            "puuid": "untrusted"
        }));
        assert!(injected.is_err());
    }
}
