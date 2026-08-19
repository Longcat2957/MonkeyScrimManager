mod auth_routes;
mod bot_routes;
mod champion_preference_routes;
mod context_routes;
mod dummy_recruitment_routes;
mod dummy_user_routes;
mod extractors;
mod guild_admin_member_routes;
mod guild_admin_settings_routes;
mod guild_audit_log_routes;
mod guild_data_transfer_routes;
mod guild_full_data_transfer_routes;
mod guild_leaderboard_routes;
mod guild_record_profile_routes;
mod guild_season_routes;
mod guild_statistics_routes;
mod middleware;
mod notification_routes;
mod realtime_routes;
mod riot_routes;
mod routes;
mod search_suggestion_routes;
mod service_admin_reset_routes;
mod workflow_routes;

use axum::{
    Router,
    extract::DefaultBodyLimit,
    middleware::from_fn_with_state,
    routing::{delete, get, patch, post, put},
};
use tower_http::limit::RequestBodyLimitLayer;

use crate::state::AppState;

pub fn build_router(state: AppState) -> Router {
    build_router_with_routes(state, Router::new())
}

fn build_router_with_routes(state: AppState, extra_routes: Router<AppState>) -> Router {
    let max_body_bytes = state.config().max_body_bytes();
    let dummy_user_routes = if state.config().dummy_users_enabled() {
        dummy_user_routes::router().merge(dummy_recruitment_routes::router())
    } else {
        Router::new()
    };

    Router::new()
        .route("/healthz", get(routes::healthz))
        .route("/readyz", get(routes::readyz))
        .route("/ws", get(realtime_routes::websocket))
        .route("/oauth/discord/start", get(auth_routes::oauth_start))
        .route("/oauth/callback", get(auth_routes::oauth_callback))
        .route("/api/logout", post(auth_routes::logout))
        .route(
            "/internal/guilds/{guild_id}/setup",
            post(bot_routes::setup_guild),
        )
        .route(
            "/internal/guilds/{guild_id}/registrations",
            post(bot_routes::register_guild_member),
        )
        .route(
            "/internal/guilds/{guild_id}/discord-catalog",
            put(guild_admin_settings_routes::replace_discord_catalog),
        )
        .route("/api/me", get(context_routes::me))
        .route(
            "/api/users/{discord_id}/profile",
            get(context_routes::user_profile),
        )
        .route(
            "/api/me/champion-preferences",
            get(champion_preference_routes::get_champion_preferences),
        )
        .route(
            "/api/me/champion-preferences/{lane}",
            put(champion_preference_routes::update_champion_preferences),
        )
        .route("/api/me/guilds", get(context_routes::me_guilds))
        .route(
            "/api/me/default-guild",
            put(context_routes::set_default_guild),
        )
        .route("/api/guilds/{guild_id}", get(context_routes::guild_context))
        .route(
            "/api/guilds/{guild_id}/members",
            get(guild_admin_member_routes::members),
        )
        .route(
            "/api/guilds/{guild_id}/members/{member_id}/mmr",
            patch(guild_admin_member_routes::adjust_mmr),
        )
        .route(
            "/api/guilds/{guild_id}/members/{member_id}/status",
            patch(guild_admin_member_routes::set_status),
        )
        .route(
            "/api/guilds/{guild_id}/settings",
            get(guild_admin_settings_routes::settings)
                .put(guild_admin_settings_routes::update_settings),
        )
        .route(
            "/api/guilds/{guild_id}/data-export",
            post(guild_data_transfer_routes::export),
        )
        .route(
            "/api/guilds/{guild_id}/data-imports/preview",
            post(guild_data_transfer_routes::preview),
        )
        .route(
            "/api/guilds/{guild_id}/data-imports/{import_id}/apply",
            post(guild_data_transfer_routes::apply),
        )
        .route(
            "/api/guilds/{guild_id}/full-data-export",
            post(guild_full_data_transfer_routes::export),
        )
        .route(
            "/api/guilds/{guild_id}/full-data-imports/preview",
            post(guild_full_data_transfer_routes::preview),
        )
        .route(
            "/api/guilds/{guild_id}/full-data-imports/{import_id}/apply",
            post(guild_full_data_transfer_routes::apply),
        )
        .route(
            "/api/guilds/{guild_id}/seasons",
            get(guild_season_routes::seasons).post(guild_season_routes::rotate_season),
        )
        .route(
            "/api/guilds/{guild_id}/audit-logs",
            get(guild_audit_log_routes::audit_logs),
        )
        .route(
            "/api/guilds/{guild_id}/leaderboard",
            get(guild_leaderboard_routes::leaderboard),
        )
        .route(
            "/api/guilds/{guild_id}/statistics",
            get(guild_statistics_routes::statistics),
        )
        .route(
            "/api/guilds/{guild_id}/operations",
            get(workflow_routes::operations),
        )
        .route(
            "/api/guilds/{guild_id}/recruitments",
            get(workflow_routes::recruitments).post(bot_routes::create_recruitment),
        )
        .route(
            "/api/guilds/{guild_id}/recruitments/{recruitment_id}",
            get(workflow_routes::recruitment).delete(bot_routes::cancel_recruitment),
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
            "/api/guilds/{guild_id}/recruitments/{recruitment_id}/participant-confirmation-dms",
            post(bot_routes::dispatch_recruitment_participant_confirmation_dms),
        )
        .route(
            "/api/guilds/{guild_id}/recruitments/{recruitment_id}/entry-draft",
            get(workflow_routes::entry_draft).patch(bot_routes::update_entry_draft),
        )
        .route(
            "/api/guilds/{guild_id}/recruitments/{recruitment_id}/entry-draft/reset",
            post(bot_routes::reset_entry_draft),
        )
        .route(
            "/api/guilds/{guild_id}/recruitments/{recruitment_id}/reopen",
            post(bot_routes::reopen_recruitment),
        )
        .route(
            "/api/guilds/{guild_id}/recruitments/{recruitment_id}/restore",
            post(bot_routes::restore_recruitment),
        )
        .route(
            "/api/guilds/{guild_id}/recruitments/{recruitment_id}/auction-recruitment",
            post(bot_routes::convert_entry_draft_to_auction_recruitment),
        )
        .route(
            "/api/guilds/{guild_id}/series",
            get(workflow_routes::series).post(bot_routes::create_series),
        )
        .route(
            "/api/guilds/{guild_id}/series/{series_id}",
            get(workflow_routes::series_detail).delete(bot_routes::delete_series),
        )
        .route(
            "/api/guilds/{guild_id}/series/{series_id}/pickban",
            patch(bot_routes::update_series_pick_ban),
        )
        .route(
            "/api/guilds/{guild_id}/series/{series_id}/games",
            post(bot_routes::submit_series_game),
        )
        .route(
            "/api/guilds/{guild_id}/series/{series_id}/games/last",
            delete(bot_routes::revert_last_series_game),
        )
        .route(
            "/api/guilds/{guild_id}/series/{series_id}/complete",
            post(bot_routes::complete_series_early),
        )
        .route(
            "/api/guilds/{guild_id}/series/{series_id}/result",
            get(workflow_routes::series_result),
        )
        .route(
            "/api/guilds/{guild_id}/series/{series_id}/revert",
            post(bot_routes::revert_series),
        )
        .route(
            "/api/guilds/{guild_id}/auction-recruitments",
            get(workflow_routes::auction_recruitments).post(bot_routes::create_auction_recruitment),
        )
        .route(
            "/api/guilds/{guild_id}/auction-recruitments/{recruitment_id}",
            get(workflow_routes::auction_recruitment).delete(bot_routes::cancel_auction_recruitment),
        )
        .route(
            "/api/guilds/{guild_id}/auction-recruitments/{recruitment_id}/force-cancel",
            post(bot_routes::force_cancel_auction_recruitment),
        )
        .route(
            "/api/guilds/{guild_id}/auction-recruitments/{recruitment_id}/setup-draft",
            get(workflow_routes::auction_setup_draft).patch(bot_routes::update_auction_setup_draft),
        )
        .route(
            "/api/guilds/{guild_id}/auction-recruitments/{recruitment_id}/members",
            post(bot_routes::join_auction_recruitment),
        )
        .route(
            "/api/guilds/{guild_id}/auction-recruitments/{recruitment_id}/members/{user_id}",
            delete(bot_routes::leave_auction_recruitment),
        )
        .route(
            "/api/guilds/{guild_id}/auction-recruitments/{recruitment_id}/close",
            post(bot_routes::close_auction_recruitment),
        )
        .route(
            "/api/guilds/{guild_id}/auction-recruitments/{recruitment_id}/reopen",
            post(bot_routes::reopen_auction_recruitment),
        )
        .route(
            "/api/guilds/{guild_id}/auction-recruitments/{recruitment_id}/restore",
            post(bot_routes::restore_auction_recruitment),
        )
        .route(
            "/api/guilds/{guild_id}/auction-recruitments/{recruitment_id}/participant-confirmation-dms",
            post(bot_routes::dispatch_auction_recruitment_participant_confirmation_dms),
        )
        .route(
            "/api/guilds/{guild_id}/auction-tournaments",
            get(workflow_routes::auction_tournaments).post(bot_routes::create_auction_tournament),
        )

        .route(
            "/api/guilds/{guild_id}/auction-tournaments/{tournament_id}",
            get(workflow_routes::auction_tournament_draft),
        )
        .route(
            "/api/guilds/{guild_id}/auction-tournaments/{tournament_id}/commands",
            post(bot_routes::execute_auction_tournament_draft_command),
        )
        .route(
            "/api/guilds/{guild_id}/auction-tournaments/{tournament_id}/bracket",
            get(workflow_routes::auction_tournament_bracket),
        )
        .route(
            "/api/guilds/{guild_id}/auction-tournaments/{tournament_id}/result",
            get(workflow_routes::auction_tournament_result),
        )
        .route(
            "/api/guilds/{guild_id}/auction-tournaments/{tournament_id}/cancel-preview",
            get(bot_routes::auction_tournament_cancel_preview),
        )
        .route(
            "/api/guilds/{guild_id}/auction-tournaments/{tournament_id}/cancel",
            post(bot_routes::cancel_auction_tournament),
        )
        .route(
            "/api/guilds/{guild_id}/auction-tournaments/{tournament_id}/bracket/commands",
            post(bot_routes::execute_auction_tournament_bracket_command),
        )

        .route(
            "/api/guilds/{guild_id}/auction-matches/{match_id}",
            get(workflow_routes::auction_match),
        )
        .route(
            "/api/guilds/{guild_id}/auction-matches/{match_id}/lineup",
            patch(bot_routes::update_auction_match_lineup),
        )
        .route(
            "/api/guilds/{guild_id}/auction-matches/{match_id}/lineup/lock",
            post(bot_routes::lock_auction_match_lineup),
        )
        .route(
            "/api/guilds/{guild_id}/auction-matches/{match_id}/lineup/unlock",
            post(bot_routes::unlock_auction_match_lineup),
        )
        .route(
            "/api/guilds/{guild_id}/auction-matches/{match_id}/games",
            post(bot_routes::submit_auction_match_game),
        )
        .route(
            "/api/guilds/{guild_id}/auction-matches/{match_id}/games/last",
            delete(bot_routes::revert_last_auction_match_game),
        )
		.route("/api/riot/records", get(riot_routes::records))
		.route(
			"/api/riot/records/suggestions",
			get(search_suggestion_routes::public_riot_suggestions),
		)
		.route(
			"/api/riot/records/matches",
			get(riot_routes::recent_matches),
		)
		.route("/api/riot/records/refresh", post(riot_routes::refresh_records))
        .route(
            "/api/guilds/{guild_id}/records/suggestions",
            get(search_suggestion_routes::guild_member_suggestions),
        )
        .route(
            "/api/guilds/{guild_id}/records/profile",
            get(guild_record_profile_routes::profile),
        )
        .route(
            "/api/guilds/{guild_id}/records/search",
            get(guild_record_profile_routes::search),
        )
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
        .merge(notification_routes::router())
        .merge(service_admin_reset_routes::router())
        .merge(dummy_user_routes)
        .merge(extra_routes)
        .fallback(routes::not_found)
        .method_not_allowed_fallback(routes::method_not_allowed)
        .layer(DefaultBodyLimit::max(max_body_bytes))
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
    async fn json_body_above_axum_default_and_below_configured_limit_is_accepted() {
        const AXUM_DEFAULT_BODY_LIMIT: usize = 2 * 1024 * 1024;
        const CONFIGURED_BODY_LIMIT: usize = 4 * 1024 * 1024;

        let state = state(5_000, CONFIGURED_BODY_LIMIT);
        let app = build_router_with_routes(state, Router::new().route("/json", post(accept_json)));
        let payload = serde_json::json!({
            "archive": "x".repeat(AXUM_DEFAULT_BODY_LIMIT + 1),
        })
        .to_string();
        assert!(payload.len() < CONFIGURED_BODY_LIMIT);

        let response = app
            .oneshot(
                Request::post("/json")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(payload))
                    .expect("large JSON request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
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

    #[tokio::test]
    async fn dummy_user_routes_are_not_mounted_when_disabled() {
        let app = build_router(state(100, 1024));
        let response = app
            .oneshot(
                Request::get("/api/dev/dummy-users")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(body_json(response).await["error"]["code"], "not_found");
    }
}
