use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use futures_util::future::BoxFuture;
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    auth::{AuthService, OAuthStartRateLimiter},
    config::Config,
    integrations::{discord::DiscordClient, riot::RiotClient},
    repository::auth::AuthRepository,
};

#[derive(Clone)]
pub struct AuthRuntime {
    service: Arc<AuthService>,
    repository: Arc<AuthRepository>,
    discord: Arc<dyn DiscordClient>,
    oauth_start_rate_limiter: OAuthStartRateLimiter,
}

impl AuthRuntime {
    pub fn new(
        service: Arc<AuthService>,
        repository: Arc<AuthRepository>,
        discord: Arc<dyn DiscordClient>,
    ) -> Self {
        Self {
            service,
            repository,
            discord,
            oauth_start_rate_limiter: OAuthStartRateLimiter::default(),
        }
    }

    pub fn service(&self) -> &Arc<AuthService> {
        &self.service
    }

    pub fn repository(&self) -> &Arc<AuthRepository> {
        &self.repository
    }

    pub fn discord(&self) -> &Arc<dyn DiscordClient> {
        &self.discord
    }

    pub const fn oauth_start_rate_limiter(&self) -> &OAuthStartRateLimiter {
        &self.oauth_start_rate_limiter
    }
}

#[derive(Clone)]
pub struct AppState {
    config: Arc<Config>,
    readiness: ReadinessService,
    id_generator: Arc<dyn IdGenerator>,
    postgres: Option<PgPool>,
    auth: Option<AuthRuntime>,
    riot: Option<Arc<dyn RiotClient>>,
}

impl AppState {
    pub fn new(
        config: Config,
        probe: Arc<dyn DependencyProbe>,
        id_generator: Arc<dyn IdGenerator>,
    ) -> Self {
        Self {
            config: Arc::new(config),
            readiness: ReadinessService::new(probe),
            id_generator,
            postgres: None,
            auth: None,
            riot: None,
        }
    }

    pub fn phase_one(config: Config) -> Self {
        Self::new(
            config,
            Arc::new(StaticProbe::phase_one()),
            Arc::new(UuidV7Generator),
        )
    }

    pub fn with_postgres(
        config: Config,
        postgres: PgPool,
        probe: Arc<dyn DependencyProbe>,
    ) -> Self {
        Self {
            config: Arc::new(config),
            readiness: ReadinessService::new(probe),
            id_generator: Arc::new(UuidV7Generator),
            postgres: Some(postgres),
            auth: None,
            riot: None,
        }
    }

    pub fn with_auth(mut self, auth: AuthRuntime) -> Self {
        self.auth = Some(auth);
        self
    }

    pub fn with_riot(mut self, riot: Arc<dyn RiotClient>) -> Self {
        self.riot = Some(riot);
        self
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub const fn readiness(&self) -> &ReadinessService {
        &self.readiness
    }

    pub fn next_request_id(&self) -> RequestId {
        RequestId(self.id_generator.next_id())
    }

    pub const fn postgres(&self) -> Option<&PgPool> {
        self.postgres.as_ref()
    }

    pub const fn auth(&self) -> Option<&AuthRuntime> {
        self.auth.as_ref()
    }

    pub const fn riot(&self) -> Option<&Arc<dyn RiotClient>> {
        self.riot.as_ref()
    }
}

pub trait IdGenerator: Send + Sync {
    fn next_id(&self) -> String;
}

#[derive(Debug)]
struct UuidV7Generator;

impl IdGenerator for UuidV7Generator {
    fn next_id(&self) -> String {
        Uuid::now_v7().to_string()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestId(pub String);

pub trait DependencyProbe: Send + Sync {
    fn check(&self) -> BoxFuture<'_, DependencySnapshot>;
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentStatus {
    Available,
    Unavailable,
    NotConfigured,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencySnapshot {
    pub postgres: ComponentStatus,
    pub migration: ComponentStatus,
    pub valkey: ComponentStatus,
}

impl DependencySnapshot {
    pub const fn phase_one() -> Self {
        Self {
            postgres: ComponentStatus::NotConfigured,
            migration: ComponentStatus::NotConfigured,
            valkey: ComponentStatus::NotConfigured,
        }
    }

    pub const fn ready() -> Self {
        Self {
            postgres: ComponentStatus::Available,
            migration: ComponentStatus::Available,
            valkey: ComponentStatus::Available,
        }
    }

    pub const fn ready_with_valkey_unavailable() -> Self {
        Self {
            postgres: ComponentStatus::Available,
            migration: ComponentStatus::Available,
            valkey: ComponentStatus::Unavailable,
        }
    }

    pub const fn shutting_down() -> Self {
        Self {
            postgres: ComponentStatus::Unavailable,
            migration: ComponentStatus::Unavailable,
            valkey: ComponentStatus::NotConfigured,
        }
    }

    fn core_ready(&self) -> bool {
        self.postgres == ComponentStatus::Available && self.migration == ComponentStatus::Available
    }
}

#[derive(Clone, Debug)]
pub struct StaticProbe {
    snapshot: DependencySnapshot,
}

impl StaticProbe {
    pub const fn new(snapshot: DependencySnapshot) -> Self {
        Self { snapshot }
    }

    pub const fn phase_one() -> Self {
        Self::new(DependencySnapshot::phase_one())
    }
}

impl DependencyProbe for StaticProbe {
    fn check(&self) -> BoxFuture<'_, DependencySnapshot> {
        Box::pin(async move { self.snapshot.clone() })
    }
}

#[derive(Clone)]
pub struct ReadinessService {
    accepting_requests: Arc<AtomicBool>,
    probe: Arc<dyn DependencyProbe>,
}

impl ReadinessService {
    pub fn new(probe: Arc<dyn DependencyProbe>) -> Self {
        Self {
            accepting_requests: Arc::new(AtomicBool::new(true)),
            probe,
        }
    }

    pub fn begin_shutdown(&self) {
        self.accepting_requests.store(false, Ordering::Release);
    }

    pub fn is_accepting_requests(&self) -> bool {
        self.accepting_requests.load(Ordering::Acquire)
    }

    pub async fn report(&self) -> ReadinessReport {
        let accepting_requests = self.is_accepting_requests();
        if !accepting_requests {
            return ReadinessReport {
                status: ReadinessStatus::NotReady,
                accepting_requests,
                dependencies: DependencySnapshot::shutting_down(),
            };
        }

        let dependencies = self.probe.check().await;
        let status = if !accepting_requests || !dependencies.core_ready() {
            ReadinessStatus::NotReady
        } else if dependencies.valkey == ComponentStatus::Available {
            ReadinessStatus::Ready
        } else {
            ReadinessStatus::Degraded
        };

        ReadinessReport {
            status,
            accepting_requests,
            dependencies,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessStatus {
    Ready,
    Degraded,
    NotReady,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadinessReport {
    pub status: ReadinessStatus,
    pub accepting_requests: bool,
    pub dependencies: DependencySnapshot,
}

impl ReadinessReport {
    pub fn is_ready(&self) -> bool {
        matches!(
            self.status,
            ReadinessStatus::Ready | ReadinessStatus::Degraded
        )
    }
}
