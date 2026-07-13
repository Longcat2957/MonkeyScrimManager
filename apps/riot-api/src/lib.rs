pub mod client;
pub mod config;
pub mod domain;
pub mod http;
pub mod service;

use std::sync::Arc;

use axum::Router;

use client::{HttpRiotClient, RiotClient};
use config::Config;
use service::RiotService;

pub fn build_app(config: Config) -> Router {
    let client: Arc<dyn RiotClient> = Arc::new(HttpRiotClient::new(
        config.api_key().clone(),
        config.request_timeout(),
    ));
    let service = Arc::new(RiotService::new(
        client,
        config.match_count(),
        config.season_start_unix_seconds(),
    ));
    http::router(http::AppState::new(config, service))
}
