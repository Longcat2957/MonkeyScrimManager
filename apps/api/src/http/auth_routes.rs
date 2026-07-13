use axum::{
    Extension,
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    auth::{
        AuthService, OAUTH_STATE_COOKIE_NAME, SESSION_COOKIE_NAME, parse_cookie, validate_next,
    },
    error::{self, ApiError},
    http::extractors::{AuthenticatedSession, require_auth, require_csrf},
    integrations::discord::{DiscordClientError, OAuthStage},
    repository::auth::{AuthRepositoryError, NewLogin, NewOAuthAttempt},
    state::{AppState, RequestId},
};

const OAUTH_ATTEMPT_TTL_SECONDS: i64 = 5 * 60;

#[derive(Debug, Default, Deserialize)]
pub struct OAuthStartQuery {
    next: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OAuthCallbackQuery {
    code: Option<String>,
    state: String,
    error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LogoutData {
    logged_out: bool,
}

pub async fn oauth_start(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Query(query): Query<OAuthStartQuery>,
) -> Result<Response, ApiError> {
    let auth = require_auth(&state, &request_id)?;
    let rate_key = oauth_rate_key(&headers);
    if !auth.oauth_start_rate_limiter().check(&rate_key) {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            &request_id,
            "rate_limited",
            "로그인 요청이 너무 많습니다. 잠시 후 다시 시도해 주세요.",
            None,
        ));
    }
    let redirect_path = validate_next(query.next.as_deref()).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            &request_id,
            "invalid_oauth_redirect",
            "로그인 후 이동할 경로가 올바르지 않습니다.",
            None,
        )
    })?;
    let attempt_id = Uuid::now_v7();
    let material = auth.service().begin_oauth(attempt_id).map_err(|error| {
        tracing::error!(%error, "failed to create OAuth material");
        internal(&request_id)
    })?;
    let authorization_url = auth
        .discord()
        .authorization_url(material.state.expose_secret(), &material.pkce_challenge)
        .map_err(|error| {
            tracing::error!(%error, "failed to build Discord authorization URL");
            internal(&request_id)
        })?;
    let attempt = NewOAuthAttempt {
        id: attempt_id,
        state_hash: material.state_hash,
        pkce_verifier_ciphertext: material.pkce_verifier_ciphertext,
        redirect_path,
        expires_at: OffsetDateTime::now_utc() + time::Duration::seconds(OAUTH_ATTEMPT_TTL_SECONDS),
    };
    auth.repository()
        .create_oauth_attempt(&attempt)
        .await
        .map_err(|repository_error| {
            tracing::error!(%repository_error, "failed to persist OAuth attempt");
            auth_repository_error(&repository_error, &request_id)
        })?;

    let mut response = StatusCode::FOUND.into_response();
    insert_location(&mut response, authorization_url.as_str(), &request_id)?;
    append_cookie(
        &mut response,
        auth.service()
            .cookies()
            .set_oauth_state(material.state.expose_secret()),
        &request_id,
    )?;
    Ok(response)
}

pub async fn oauth_callback(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Query(query): Query<OAuthCallbackQuery>,
) -> Result<Response, ApiError> {
    let auth = require_auth(&state, &request_id)?;
    let cookie_state = parse_cookie(&headers, OAUTH_STATE_COOKIE_NAME)
        .ok_or_else(|| invalid_oauth_state(&request_id))?;
    if query.state.len() != 43
        || cookie_state.len() != 43
        || !AuthService::secrets_equal(&query.state, &cookie_state)
    {
        return Err(invalid_oauth_state(&request_id));
    }

    let state_hash = AuthService::hash_secret(&query.state);
    let attempt = auth
        .repository()
        .consume_oauth_attempt(&state_hash)
        .await
        .map_err(|repository_error| {
            tracing::error!(%repository_error, "failed to consume OAuth attempt");
            auth_repository_error(&repository_error, &request_id)
        })?
        .ok_or_else(|| invalid_oauth_state(&request_id))?;
    let code = match (query.code.as_deref(), query.error.as_deref()) {
        (Some(code), None) if !code.is_empty() => code,
        _ => {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                &request_id,
                "oauth_authorization_denied",
                "Discord 로그인이 취소되었거나 승인되지 않았습니다.",
                None,
            ));
        }
    };
    let pkce_verifier = auth
        .service()
        .decrypt_pkce(attempt.id, &attempt.pkce_verifier_ciphertext)
        .map_err(|error| {
            tracing::error!(%error, "failed to decrypt OAuth verifier");
            internal(&request_id)
        })?;
    let discord_user = auth
        .discord()
        .authenticate(code, &pkce_verifier)
        .await
        .map_err(|error| provider_error(error, &request_id))?;
    let session = auth.service().new_session().map_err(|error| {
        tracing::error!(%error, "failed to create session material");
        internal(&request_id)
    })?;
    let previous_session_token_hash =
        parse_cookie(&headers, SESSION_COOKIE_NAME).map(|token| AuthService::hash_secret(&token));
    let session_ttl = i64::try_from(auth.service().cookies().session_ttl().as_secs())
        .expect("session TTL is bounded by configuration");
    let login = NewLogin {
        discord_user_id: discord_user.id,
        display_name: discord_user.display_name,
        avatar_hash: discord_user.avatar_hash,
        previous_session_token_hash,
        session_id: Uuid::now_v7(),
        token_hash: session.token_hash,
        csrf_secret_hash: session.csrf_secret_hash,
        expires_at: OffsetDateTime::now_utc() + time::Duration::seconds(session_ttl),
    };
    auth.repository()
        .complete_login(&login)
        .await
        .map_err(|repository_error| {
            tracing::error!(%repository_error, "failed to complete OAuth login");
            auth_repository_error(&repository_error, &request_id)
        })?;

    let mut response = StatusCode::SEE_OTHER.into_response();
    insert_location(&mut response, &attempt.redirect_path, &request_id)?;
    for cookie in [
        auth.service()
            .cookies()
            .set_session(session.token.expose_secret()),
        auth.service()
            .cookies()
            .set_csrf(session.csrf_token.expose_secret()),
        auth.service().cookies().clear_oauth_state(),
    ] {
        append_cookie(&mut response, cookie, &request_id)?;
    }
    Ok(response)
}

pub async fn logout(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
) -> Result<Response, ApiError> {
    require_csrf(&headers, &state, &principal, &request_id)?;
    let auth = require_auth(&state, &request_id)?;
    auth.repository()
        .revoke_session(&principal.token_hash)
        .await
        .map_err(|repository_error| {
            tracing::error!(%repository_error, "failed to revoke session");
            auth_repository_error(&repository_error, &request_id)
        })?;

    let mut response = error::success(StatusCode::OK, &request_id, LogoutData { logged_out: true });
    for cookie in [
        auth.service().cookies().clear_session(),
        auth.service().cookies().clear_csrf(),
    ] {
        append_cookie(&mut response, cookie, &request_id)?;
    }
    Ok(response)
}

fn provider_error(error: DiscordClientError, request_id: &RequestId) -> ApiError {
    tracing::warn!(%error, "Discord OAuth authentication failed");
    if matches!(
        error,
        DiscordClientError::InvalidAuthorizationCode
            | DiscordClientError::Rejected {
                stage: OAuthStage::TokenExchange,
                status: 400,
            }
    ) {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            request_id,
            "oauth_exchange_failed",
            "Discord 로그인을 완료할 수 없습니다. 다시 시도해 주세요.",
            None,
        )
    } else {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            request_id,
            "oauth_provider_unavailable",
            "Discord 인증 서비스를 일시적으로 사용할 수 없습니다.",
            None,
        )
    }
}

fn invalid_oauth_state(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        request_id,
        "invalid_oauth_state",
        "로그인 요청이 만료되었거나 올바르지 않습니다.",
        None,
    )
}

fn auth_repository_error(error: &AuthRepositoryError, request_id: &RequestId) -> ApiError {
    match error {
        AuthRepositoryError::Database(source) => error::database_failure(source, request_id),
        AuthRepositoryError::InvalidStoredSnowflake(_)
        | AuthRepositoryError::InvalidStoredUserId(_)
        | AuthRepositoryError::IdentityConflict => internal(request_id),
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

fn insert_location(
    response: &mut Response,
    location: &str,
    request_id: &RequestId,
) -> Result<(), ApiError> {
    let value = HeaderValue::from_str(location).map_err(|_| internal(request_id))?;
    response.headers_mut().insert(header::LOCATION, value);
    Ok(())
}

fn append_cookie(
    response: &mut Response,
    cookie: String,
    request_id: &RequestId,
) -> Result<(), ApiError> {
    let value = HeaderValue::from_str(&cookie).map_err(|_| internal(request_id))?;
    response.headers_mut().append(header::SET_COOKIE, value);
    Ok(())
}

fn oauth_rate_key(headers: &HeaderMap) -> String {
    ["x-real-ip", "cf-connecting-ip"]
        .into_iter()
        .filter_map(|name| headers.get(name))
        .filter_map(|value| value.to_str().ok())
        .find_map(|value| value.trim().parse::<IpAddr>().ok())
        .map_or_else(|| "unknown".to_owned(), |address| address.to_string())
}
