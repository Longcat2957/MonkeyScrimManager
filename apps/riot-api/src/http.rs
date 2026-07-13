use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use ring::hmac;
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use tower_http::limit::RequestBodyLimitLayer;

use crate::{
    client::RiotClientError,
    config::Config,
    domain::{InputError, RiotIdInput},
    service::{DEFAULT_RECENT_MATCH_BATCH, RiotService, ServiceError},
};

const MAX_BODY_BYTES: usize = 16 * 1024;

#[derive(Clone)]
pub struct AppState {
    config: Arc<Config>,
    service: Arc<RiotService>,
}

impl AppState {
    pub fn new(config: Config, service: Arc<RiotService>) -> Self {
        Self {
            config: Arc::new(config),
            service,
        }
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/internal/riot/accounts/resolve", post(resolve_account))
        .route("/internal/riot/records", get(records))
        .route("/internal/riot/records/matches", get(recent_matches))
        .route("/internal/riot/records/refresh", post(refresh_records))
        .fallback(not_found)
        .layer(RequestBodyLimitLayer::new(MAX_BODY_BYTES))
        .with_state(state)
}

async fn healthz() -> Json<HealthData> {
    Json(HealthData { status: "ok" })
}

async fn resolve_account(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<AccountResolveRequest>,
) -> Result<Json<crate::domain::ResolvedAccount>, InternalError> {
    authorize(&headers, &state)?;
    let account = match request.lookup {
        AccountLookup::RiotId {
            game_name,
            tag_line,
        } => {
            state
                .service
                .resolve_by_riot_id(RiotIdInput {
                    game_name,
                    tag_line,
                    platform: request.platform,
                })
                .await?
        }
        AccountLookup::Puuid { puuid } => {
            state
                .service
                .resolve_by_puuid(&request.platform, &puuid)
                .await?
        }
    };
    Ok(Json(account))
}

async fn records(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<RecordsQuery>,
) -> Result<Json<crate::domain::RecordsSnapshot>, InternalError> {
    authorize(&headers, &state)?;
    Ok(Json(
        state
            .service
            .records(
                RiotIdInput {
                    game_name: query.game_name,
                    tag_line: query.tag_line,
                    platform: query.platform,
                },
                false,
            )
            .await?,
    ))
}

async fn refresh_records(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<RiotIdInput>,
) -> Result<Json<crate::domain::RecordsSnapshot>, InternalError> {
    authorize(&headers, &state)?;
    Ok(Json(state.service.records(request, true).await?))
}

async fn recent_matches(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<RecentMatchesQuery>,
) -> Result<Json<crate::domain::RecentMatchesPage>, InternalError> {
    authorize(&headers, &state)?;
    Ok(Json(
        state
            .service
            .recent_matches_page(
                RiotIdInput {
                    game_name: query.game_name,
                    tag_line: query.tag_line,
                    platform: query.platform,
                },
                query.start,
                query.count,
            )
            .await?,
    ))
}

async fn not_found() -> InternalError {
    InternalError::new(
        StatusCode::NOT_FOUND,
        "not_found",
        "Internal Riot endpoint was not found.",
    )
}

fn authorize(headers: &HeaderMap, state: &AppState) -> Result<(), InternalError> {
    let supplied = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or_default();
    let expected = state.config.internal_token().expose_secret();
    let supplied_key = hmac::Key::new(hmac::HMAC_SHA256, supplied.as_bytes());
    let supplied_tag = hmac::sign(&supplied_key, b"monkeybot-riot-internal-auth");
    let expected_key = hmac::Key::new(hmac::HMAC_SHA256, expected.as_bytes());
    hmac::verify(
        &expected_key,
        b"monkeybot-riot-internal-auth",
        supplied_tag.as_ref(),
    )
    .map_err(|_| {
        InternalError::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "Internal service authentication failed.",
        )
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AccountResolveRequest {
    platform: String,
    lookup: AccountLookup,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum AccountLookup {
    RiotId {
        #[serde(rename = "gameName")]
        game_name: String,
        #[serde(rename = "tagLine")]
        tag_line: String,
    },
    Puuid {
        puuid: String,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecordsQuery {
    game_name: String,
    tag_line: String,
    platform: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecentMatchesQuery {
    game_name: String,
    tag_line: String,
    platform: String,
    #[serde(default)]
    start: usize,
    #[serde(default = "default_recent_match_count")]
    count: usize,
}

const fn default_recent_match_count() -> usize {
    DEFAULT_RECENT_MATCH_BATCH
}

#[derive(Serialize)]
struct HealthData {
    status: &'static str,
}

struct InternalError {
    status: StatusCode,
    code: &'static str,
    message: &'static str,
    retry_after_seconds: Option<u64>,
}

impl InternalError {
    const fn new(status: StatusCode, code: &'static str, message: &'static str) -> Self {
        Self {
            status,
            code,
            message,
            retry_after_seconds: None,
        }
    }
}

impl From<ServiceError> for InternalError {
    fn from(error: ServiceError) -> Self {
        match error {
            ServiceError::Input(
                InputError::RiotId | InputError::Platform | InputError::Pagination,
            )
            | ServiceError::Riot(RiotClientError::InvalidRequest) => Self::new(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "Riot lookup parameters are invalid.",
            ),
            ServiceError::Riot(RiotClientError::NotFound) => Self::new(
                StatusCode::NOT_FOUND,
                "riot_account_not_found",
                "The Riot account was not found.",
            ),
            ServiceError::Riot(RiotClientError::RateLimited {
                retry_after_seconds,
            }) => Self {
                status: StatusCode::TOO_MANY_REQUESTS,
                code: "riot_rate_limited",
                message: "The Riot API rate limit was exceeded.",
                retry_after_seconds,
            },
            ServiceError::Riot(RiotClientError::Timeout) => Self::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "riot_timeout",
                "The Riot API request timed out.",
            ),
            ServiceError::Riot(RiotClientError::Unavailable) => Self::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "riot_unavailable",
                "The Riot API is temporarily unavailable.",
            ),
            ServiceError::Riot(RiotClientError::Unauthorized) => Self::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "riot_credentials_rejected",
                "The Riot API credentials were rejected.",
            ),
            ServiceError::Riot(
                RiotClientError::MalformedResponse
                | RiotClientError::InvalidUrl
                | RiotClientError::Rejected(_),
            ) => Self::new(
                StatusCode::BAD_GATEWAY,
                "riot_bad_gateway",
                "The Riot API returned an unusable response.",
            ),
        }
    }
}

impl IntoResponse for InternalError {
    fn into_response(self) -> Response {
        let mut response = (
            self.status,
            Json(ErrorEnvelope {
                error: ErrorData {
                    code: self.code,
                    message: self.message,
                },
            }),
        )
            .into_response();
        if let Some(seconds) = self.retry_after_seconds
            && let Ok(value) = HeaderValue::from_str(&seconds.to_string())
        {
            response
                .headers_mut()
                .insert(axum::http::header::RETRY_AFTER, value);
        }
        response
    }
}

#[derive(Serialize)]
struct ErrorEnvelope {
    error: ErrorData,
}

#[derive(Serialize)]
struct ErrorData {
    code: &'static str,
    message: &'static str,
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use axum::{body::Body, http::Request};
    use futures_util::future::BoxFuture;
    use tower::ServiceExt;

    use super::*;
    use crate::client::{
        LeagueEntryDto, MatchDto, MatchIdsQuery, RiotAccountDto, RiotClient, SummonerDto,
    };
    use crate::domain::{Platform, RoutingRegion};

    struct UnusedClient;

    impl RiotClient for UnusedClient {
        fn account_by_riot_id<'a>(
            &'a self,
            _: RoutingRegion,
            _: &'a str,
            _: &'a str,
        ) -> BoxFuture<'a, Result<RiotAccountDto, RiotClientError>> {
            Box::pin(async { panic!("unexpected provider call") })
        }

        fn account_by_puuid<'a>(
            &'a self,
            _: RoutingRegion,
            _: &'a str,
        ) -> BoxFuture<'a, Result<RiotAccountDto, RiotClientError>> {
            Box::pin(async { panic!("unexpected provider call") })
        }

        fn summoner_by_puuid<'a>(
            &'a self,
            _: Platform,
            _: &'a str,
        ) -> BoxFuture<'a, Result<SummonerDto, RiotClientError>> {
            Box::pin(async { panic!("unexpected provider call") })
        }

        fn league_entries_by_puuid<'a>(
            &'a self,
            _: Platform,
            _: &'a str,
        ) -> BoxFuture<'a, Result<Vec<LeagueEntryDto>, RiotClientError>> {
            Box::pin(async { panic!("unexpected provider call") })
        }

        fn match_ids<'a>(
            &'a self,
            _: RoutingRegion,
            _: &'a str,
            _: MatchIdsQuery,
        ) -> BoxFuture<'a, Result<Vec<String>, RiotClientError>> {
            Box::pin(async { panic!("unexpected provider call") })
        }

        fn match_detail<'a>(
            &'a self,
            _: RoutingRegion,
            _: &'a str,
        ) -> BoxFuture<'a, Result<MatchDto, RiotClientError>> {
            Box::pin(async { panic!("unexpected provider call") })
        }
    }

    fn test_app() -> Router {
        let values = HashMap::from([
            ("MONKEYBOT_RIOT_API_KEY".into(), "RGAPI-test".into()),
            (
                "MONKEYBOT_RIOT_INTERNAL_TOKEN".into(),
                "0123456789abcdef0123456789abcdef".into(),
            ),
            (
                "MONKEYBOT_RIOT_SEASON_START_UNIX_SECONDS".into(),
                "1735689600".into(),
            ),
        ]);
        let config = Config::from_map(&values).expect("config");
        let service = Arc::new(RiotService::new(Arc::new(UnusedClient), 10, 1_735_689_600));
        router(AppState::new(config, service))
    }

    #[tokio::test]
    async fn health_is_public_but_internal_routes_require_bearer_token() {
        let app = test_app();
        let health = app
            .clone()
            .oneshot(
                Request::get("/healthz")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(health.status(), StatusCode::OK);

        let unauthorized = app
            .oneshot(
                Request::get("/internal/riot/records?gameName=Longcat&tagLine=KR1&platform=KR")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn recent_match_page_route_validates_batch_size_before_provider_access() {
        let response = test_app()
            .oneshot(
                Request::get(
                    "/internal/riot/records/matches?gameName=Longcat&tagLine=KR1&platform=KR&start=0&count=21",
                )
                .header(
                    axum::http::header::AUTHORIZATION,
                    "Bearer 0123456789abcdef0123456789abcdef",
                )
                .body(Body::empty())
                .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
