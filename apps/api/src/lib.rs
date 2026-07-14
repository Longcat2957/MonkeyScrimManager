pub mod auth;
pub mod bootstrap;
pub mod config;
pub mod domain;
pub mod error;
pub mod http;
pub mod infrastructure;
pub mod integrations;
pub mod realtime;
pub mod repository;
pub mod state;
pub mod telemetry;

pub use http::build_router;
pub use state::{AppState, DependencySnapshot, IdGenerator, ReadinessService, StaticProbe};
