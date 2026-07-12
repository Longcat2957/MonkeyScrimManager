mod auth_routes;
mod bot_routes;
mod context_routes;
mod extractors;
mod middleware;
mod riot_routes;
mod routes;
mod workflow_routes;

use axum::{
    Router,
    middleware::from_fn_with_state,
    routing::{delete, get, post, put},
};
use tower_http::limit::RequestBodyLimitLayer;

use crate::state::AppState;

pub fn build_router(state: AppState) -> Router {
    build_router_with_routes(state, Router::new())
}

fn build_router_with_routes(state: AppState, extra_routes: Router<AppState>) -> Router {
    let max_body_bytes = state.config().max_body_bytes();

    Router::new()
        .route("/healthz", get(routes::healthz))
        .route("/readyz", get(routes::readyz))
        .route("/oauth/discord/start", get(auth_routes::oauth_start))
        .route("/oauth/callback", get(auth_routes::oauth_callback))
        .route("/api/logout", post(auth_routes::logout))
        .route(
            "/internal/guilds/{guild_id}/setup",
            post(bot_routes::setup_guild),
        )
        .route("/api/me", get(context_routes::me))
        .route("/api/me/guilds", get(context_routes::me_guilds))
        .route(
            "/api/me/default-guild",
            put(context_routes::set_default_guild),
        )
        .route("/api/guilds/{guild_id}", get(context_routes::guild_context))
        .route(
            "/api/guilds/{guild_id}/operations",
            get(workflow_routes::operations),
        )
        .route(
            "/api/guilds/{guild_id}/recruitments",
            get(workflow_routes::recruitments).post(bot_routes::create_recruitment),
        )
        .route(
            "/api/guilds/{guild_id}/recruitments/{recruitment_id}/message",
            put(bot_routes::bind_recruitment_message),
        )
        .route(
            "/api/guilds/{guild_id}/recruitments/{recruitment_id}/members",
            post(bot_routes::join_recruitment),
        )
        .route(
            "/api/guilds/{guild_id}/recruitments/{recruitment_id}/members/{user_id}",
            delete(bot_routes::leave_recruitment),
        )
        .route(
            "/api/guilds/{guild_id}/recruitments/{recruitment_id}/members/{user_id}/preferred-lanes",
            put(bot_routes::set_preferred_lanes),
        )
        .route(
            "/api/guilds/{guild_id}/recruitments/{recruitment_id}/close",
            post(bot_routes::close_recruitment),
        )
        .route(
            "/api/guilds/{guild_id}/series",
            get(workflow_routes::series),
        )
        .route(
            "/api/guilds/{guild_id}/auction-recruitments",
            get(workflow_routes::auction_recruitments),
        )
        .route(
            "/api/guilds/{guild_id}/auction-tournaments",
            get(workflow_routes::auction_tournaments),
        )
        .route("/api/riot/records", get(riot_routes::records))
        .route("/api/riot/records/refresh", post(riot_routes::refresh_records))
        .route(
            "/api/me/riot-accounts",
            get(riot_routes::list_riot_accounts).post(riot_routes::create_riot_account),
        )
        .route(
            "/api/me/riot-accounts/{puuid}/main",
            put(riot_routes::set_main_account),
        )
        .route(
            "/api/me/riot-accounts/{puuid}/refresh",
            post(riot_routes::refresh_riot_account),
        )
        .route(
            "/api/me/riot-accounts/{puuid}",
            delete(riot_routes::delete_riot_account),
        )
        .route(
            "/api/me/privacy",
            get(riot_routes::privacy).put(riot_routes::update_privacy),
        )
        .merge(extra_routes)
        .fallback(routes::not_found)
        .method_not_allowed_fallback(routes::method_not_allowed)
        .layer(RequestBodyLimitLayer::new(max_body_bytes))
        .layer(from_fn_with_state(
            state.clone(),
            middleware::request_pipeline,
        ))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc, time::Duration};

    use axum::{
        Json, Router,
        body::{Body, to_bytes},
        http::{HeaderValue, Request, StatusCode, header},
        response::IntoResponse,
        routing::{get, post},
    };
    use serde_json::Value;
    use tower::ServiceExt;

    use super::*;
    use crate::{
        config::Config,
        error::ErrorCode,
        state::{DependencySnapshot, IdGenerator, StaticProbe},
    };

    #[derive(Debug)]
    struct FixedId;

    impl IdGenerator for FixedId {
        fn next_id(&self) -> String {
            "01900000-0000-7000-8000-000000000001".to_owned()
        }
    }

    fn state(request_timeout_ms: u64, max_body_bytes: usize) -> AppState {
        let values = HashMap::from([
            ("MONKEYBOT_ENV".to_owned(), "test".to_owned()),
            (
                "MONKEYBOT_BIND_ADDR".to_owned(),
                "127.0.0.1:30020".to_owned(),
            ),
            (
                "MONKEYBOT_PUBLIC_ORIGIN".to_owned(),
                "http://localhost:5173".to_owned(),
            ),
            (
                "MONKEYBOT_REQUEST_TIMEOUT_MS".to_owned(),
                request_timeout_ms.to_string(),
            ),
            (
                "MONKEYBOT_MAX_BODY_BYTES".to_owned(),
                max_body_bytes.to_string(),
            ),
        ]);
        AppState::new(
            Config::from_map(&values).expect("test config should parse"),
            Arc::new(StaticProbe::new(DependencySnapshot::ready())),
            Arc::new(FixedId),
        )
    }

    async fn body_json(response: axum::response::Response) -> Value {
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        serde_json::from_slice(&bytes).expect("JSON response")
    }

    async fn panic_handler() -> &'static str {
        panic!("database password should never be returned")
    }

    async fn accept_json(Json(payload): Json<HashMap<String, String>>) -> StatusCode {
        std::hint::black_box(payload);
        StatusCode::NO_CONTENT
    }

    #[tokio::test]
    async fn timeout_is_normalized_to_json_error() {
        let state = state(5, 1024);
        let extra = Router::new().route(
            "/slow",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(50)).await;
                "ok"
            }),
        );
        let response = build_router_with_routes(state, extra)
            .oneshot(Request::get("/slow").body(Body::empty()).expect("request"))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            body_json(response).await["error"]["code"],
            "request_timeout"
        );
    }

    #[tokio::test]
    async fn panic_is_normalized_without_exposing_panic_text() {
        let state = state(100, 1024);
        let extra = Router::new().route("/panic", get(panic_handler));
        let response = build_router_with_routes(state, extra)
            .oneshot(Request::get("/panic").body(Body::empty()).expect("request"))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = body_json(response).await;
        assert_eq!(body["error"]["code"], "internal_error");
        assert!(!body.to_string().contains("password"));
    }

    #[tokio::test]
    async fn oversized_body_is_normalized_to_json_error() {
        let state = state(100, 4);
        let extra = Router::new().route("/echo", post(|body: String| async move { body }));
        let request = Request::post("/echo")
            .header(header::CONTENT_TYPE, "text/plain")
            .body(Body::from("12345"))
            .expect("request");
        let response = build_router_with_routes(state, extra)
            .oneshot(request)
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let body = body_json(response).await;
        assert_eq!(body["error"]["code"], "payload_too_large");
        assert_eq!(body["error"]["details"]["maxBodyBytes"], 4);
    }

    #[tokio::test]
    async fn json_extractor_rejections_use_the_common_error_envelope() {
        let state = state(100, 1024);
        let app = build_router_with_routes(state, Router::new().route("/json", post(accept_json)));
        let cases = [
            (
                Request::post("/json")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from("{"))
                    .expect("malformed JSON request"),
                StatusCode::BAD_REQUEST,
                "bad_request",
            ),
            (
                Request::post("/json")
                    .body(Body::from(r#"{"name":"Longcat"}"#))
                    .expect("missing content type request"),
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "unsupported_media_type",
            ),
            (
                Request::post("/json")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"name":123}"#))
                    .expect("invalid JSON value request"),
                StatusCode::UNPROCESSABLE_ENTITY,
                "unprocessable_entity",
            ),
        ];

        for (request, expected_status, expected_code) in cases {
            let response = app.clone().oneshot(request).await.expect("response");
            assert_eq!(response.status(), expected_status);
            assert_eq!(
                response
                    .extensions()
                    .get::<ErrorCode>()
                    .copied()
                    .map(ErrorCode::as_str),
                Some(expected_code)
            );
            let body = body_json(response).await;
            assert_eq!(body["error"]["code"], expected_code);
            assert_eq!(
                body["error"]["requestId"],
                "01900000-0000-7000-8000-000000000001"
            );
        }
    }

    #[tokio::test]
    async fn raw_server_errors_are_redacted_and_normalized() {
        const SENSITIVE_BODY: &str = "postgres password=do-not-return";

        let state = state(100, 1024);
        let extra = Router::new().route(
            "/upstream",
            get(|| async { (StatusCode::BAD_GATEWAY, SENSITIVE_BODY) }),
        );
        let response = build_router_with_routes(state, extra)
            .oneshot(
                Request::get("/upstream")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(
            response
                .extensions()
                .get::<ErrorCode>()
                .copied()
                .map(ErrorCode::as_str),
            Some("internal_error")
        );
        let body = body_json(response).await;
        assert_eq!(body["error"]["code"], "internal_error");
        assert!(!body.to_string().contains(SENSITIVE_BODY));
    }

    #[tokio::test]
    async fn normalization_preserves_protocol_headers() {
        let state = state(100, 1024);
        let extra = Router::new().route(
            "/raw-method-error",
            get(|| async {
                let mut response = (StatusCode::METHOD_NOT_ALLOWED, "raw").into_response();
                response
                    .headers_mut()
                    .insert(header::ALLOW, HeaderValue::from_static("POST"));
                response
            }),
        );
        let response = build_router_with_routes(state, extra)
            .oneshot(
                Request::get("/raw-method-error")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(response.headers()[header::ALLOW], "POST");
        assert_eq!(
            body_json(response).await["error"]["code"],
            "method_not_allowed"
        );
    }
}
