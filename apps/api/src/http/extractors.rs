use crate::{
    auth::{AuthService, CSRF_COOKIE_NAME, CSRF_HEADER_NAME, SESSION_COOKIE_NAME, parse_cookie},
    error::{self, ApiError},
    repository::auth::{AuthRepositoryError, ResolvedSession},
    state::{AppState, AuthRuntime, RequestId},
};
use axum::{
    extract::FromRequestParts,
    http::{HeaderMap, StatusCode, header, request::Parts},
};
use secrecy::ExposeSecret;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BotServicePrincipal;

impl FromRequestParts<AppState> for BotServicePrincipal {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let request_id = request_id(parts);
        let bot_service = state.config().bot_service().ok_or_else(|| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                &request_id,
                "bot_service_unavailable",
                "Bot 인증 서비스를 사용할 수 없습니다.",
                None,
            )
        })?;
        let provided = single_header(&parts.headers, &header::AUTHORIZATION)
            .and_then(|value| value.strip_prefix("Bearer "))
            .filter(|value| !value.is_empty())
            .ok_or_else(|| unauthorized(&request_id))?;

        if !AuthService::secrets_equal(provided, bot_service.token().expose_secret()) {
            return Err(unauthorized(&request_id));
        }

        Ok(Self)
    }
}

pub enum RecruitmentCreatePrincipal {
    Bot(BotServicePrincipal),
    Session(AuthenticatedSession),
}

impl FromRequestParts<AppState> for RecruitmentCreatePrincipal {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // An Authorization header always selects the bot path. This prevents a malformed bearer
        // token from silently falling back to an unrelated browser session.
        if parts.headers.contains_key(header::AUTHORIZATION) {
            BotServicePrincipal::from_request_parts(parts, state)
                .await
                .map(Self::Bot)
        } else {
            AuthenticatedSession::from_request_parts(parts, state)
                .await
                .map(Self::Session)
        }
    }
}

pub struct AuthenticatedSession {
    pub session: ResolvedSession,
    pub token_hash: Vec<u8>,
}

impl FromRequestParts<AppState> for AuthenticatedSession {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let request_id = request_id(parts);
        let auth = require_auth(state, &request_id)?;
        let token = parse_cookie(&parts.headers, SESSION_COOKIE_NAME).ok_or_else(|| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                &request_id,
                "unauthorized",
                "인증이 필요합니다.",
                None,
            )
        })?;
        let token_hash = AuthService::hash_secret(&token);
        let session = auth
            .repository()
            .resolve_session(&token_hash)
            .await
            .map_err(|repository_error| {
                tracing::error!(%repository_error, "failed to resolve authenticated session");
                auth_repository_error(&repository_error, &request_id)
            })?
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::UNAUTHORIZED,
                    &request_id,
                    "unauthorized",
                    "인증이 필요합니다.",
                    None,
                )
            })?;

        if session.last_seen_at < time::OffsetDateTime::now_utc() - time::Duration::minutes(5)
            && let Err(error) = auth.repository().touch_session(session.session_id).await
        {
            tracing::warn!(%error, "failed to update session activity timestamp");
        }

        Ok(Self {
            session,
            token_hash,
        })
    }
}

pub fn require_auth<'a>(
    state: &'a AppState,
    request_id: &RequestId,
) -> Result<&'a AuthRuntime, ApiError> {
    state.auth().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            request_id,
            "authentication_unavailable",
            "인증 서비스를 일시적으로 사용할 수 없습니다.",
            None,
        )
    })
}

pub fn require_csrf(
    headers: &HeaderMap,
    state: &AppState,
    principal: &AuthenticatedSession,
    request_id: &RequestId,
) -> Result<(), ApiError> {
    let expected_origin = state.config().public_origin_header();
    let origin_matches =
        single_header(headers, &header::ORIGIN).is_some_and(|value| value == expected_origin);
    let cookie_token = parse_cookie(headers, CSRF_COOKIE_NAME);
    let header_token = single_header(headers, &CSRF_HEADER_NAME);

    let valid = origin_matches
        && cookie_token
            .as_deref()
            .zip(header_token)
            .is_some_and(|(cookie, header)| {
                AuthService::secrets_equal(cookie, header)
                    && AuthService::hash_matches(cookie, &principal.session.csrf_secret_hash)
            });

    if valid {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "csrf_validation_failed",
            "요청 출처를 확인할 수 없습니다.",
            None,
        ))
    }
}

fn request_id(parts: &Parts) -> RequestId {
    parts
        .extensions
        .get::<RequestId>()
        .cloned()
        .unwrap_or_else(|| RequestId("unknown".to_owned()))
}

fn single_header<'a>(headers: &'a HeaderMap, name: &axum::http::HeaderName) -> Option<&'a str> {
    let mut values = headers.get_all(name).iter();
    let value = values.next()?.to_str().ok()?;
    values.next().is_none().then_some(value)
}

fn auth_repository_error(error: &AuthRepositoryError, request_id: &RequestId) -> ApiError {
    match error {
        AuthRepositoryError::Database(source) => error::database_failure(source, request_id),
        AuthRepositoryError::InvalidStoredSnowflake(_) => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            request_id,
            "internal_error",
            "요청을 처리하는 중 내부 오류가 발생했습니다.",
            None,
        ),
        AuthRepositoryError::InvalidStoredUserId(_) | AuthRepositoryError::IdentityConflict => {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                request_id,
                "internal_error",
                "요청을 처리하는 중 내부 오류가 발생했습니다.",
                None,
            )
        }
    }
}

fn unauthorized(request_id: &RequestId) -> ApiError {
    ApiError::new(
        StatusCode::UNAUTHORIZED,
        request_id,
        "unauthorized",
        "인증이 필요합니다.",
        None,
    )
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use axum::{
        extract::FromRequestParts as _,
        http::{HeaderValue, Request, header},
        response::IntoResponse,
    };

    use super::*;
    use crate::config::Config;

    const BOT_TOKEN: &str = "bot-service-token-that-is-at-least-32-characters";

    fn state(token: Option<&str>) -> AppState {
        let mut values = HashMap::from([
            ("MONKEYBOT_ENV".to_owned(), "test".to_owned()),
            (
                "MONKEYBOT_BIND_ADDR".to_owned(),
                "127.0.0.1:30020".to_owned(),
            ),
            (
                "MONKEYBOT_PUBLIC_ORIGIN".to_owned(),
                "http://localhost:5173".to_owned(),
            ),
        ]);
        if let Some(token) = token {
            values.insert("MONKEYBOT_BOT_SERVICE_TOKEN".to_owned(), token.to_owned());
        }
        AppState::phase_one(Config::from_map(&values).expect("test config"))
    }

    fn parts(authorization: Option<&str>, cookie: Option<&str>) -> Parts {
        let mut request = Request::builder().uri("/internal/test");
        if let Some(authorization) = authorization {
            request = request.header(header::AUTHORIZATION, authorization);
        }
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        let (mut parts, _) = request.body(()).expect("request").into_parts();
        parts
            .extensions
            .insert(RequestId("01900000-0000-7000-8000-000000000001".to_owned()));
        parts
    }

    async fn rejection_status(
        state: &AppState,
        authorization: Option<&str>,
        cookie: Option<&str>,
    ) -> StatusCode {
        BotServicePrincipal::from_request_parts(&mut parts(authorization, cookie), state)
            .await
            .expect_err("request should be rejected")
            .into_response()
            .status()
    }

    #[tokio::test]
    async fn accepts_only_the_exact_configured_bearer_token() {
        let state = state(Some(BOT_TOKEN));
        let authorization = format!("Bearer {BOT_TOKEN}");

        assert!(matches!(
            BotServicePrincipal::from_request_parts(&mut parts(Some(&authorization), None), &state)
                .await,
            Ok(BotServicePrincipal)
        ));

        for invalid in [
            None,
            Some("Bearer wrong-token-that-is-at-least-32-characters"),
            Some("bearer bot-service-token-that-is-at-least-32-characters"),
            Some("Bearer  bot-service-token-that-is-at-least-32-characters"),
        ] {
            assert_eq!(
                rejection_status(&state, invalid, None).await,
                StatusCode::UNAUTHORIZED
            );
        }
    }

    #[tokio::test]
    async fn does_not_treat_a_browser_session_cookie_as_bot_authentication() {
        let state = state(Some(BOT_TOKEN));

        assert_eq!(
            rejection_status(
                &state,
                None,
                Some("monkeybot_session=browser-session-token")
            )
            .await,
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn rejects_duplicate_authorization_headers() {
        let state = state(Some(BOT_TOKEN));
        let mut parts = parts(None, None);
        let value = HeaderValue::from_str(&format!("Bearer {BOT_TOKEN}")).expect("header");
        parts.headers.append(header::AUTHORIZATION, value.clone());
        parts.headers.append(header::AUTHORIZATION, value);

        assert_eq!(
            BotServicePrincipal::from_request_parts(&mut parts, &state)
                .await
                .expect_err("duplicate headers must be rejected")
                .into_response()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn returns_service_unavailable_when_bot_auth_is_not_configured() {
        let state = state(None);

        assert_eq!(
            rejection_status(&state, Some("Bearer any-token"), None).await,
            StatusCode::SERVICE_UNAVAILABLE
        );
    }
}
