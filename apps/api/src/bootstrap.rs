use std::{
    future::{Future, IntoFuture},
    sync::Arc,
    time::Duration,
};

use axum::Router;
use reqwest::Url;
use thiserror::Error;
use time::OffsetDateTime;
use tokio::{
    net::TcpListener,
    sync::watch,
    task::JoinHandle,
    time::{Instant, MissedTickBehavior, interval, sleep_until, timeout, timeout_at},
};

use crate::{
    auth::{AuthCryptoError, AuthService, CookiePolicy},
    config::Config,
    http::build_router,
    infrastructure::postgres::{self, PostgresError, PostgresReadinessProbe},
    integrations::{
        discord::{DiscordClientError, ReqwestDiscordClient},
        riot::{ReqwestRiotClient, RiotClientError},
    },
    realtime::{RealtimeRuntime, RealtimeRuntimeError},
    repository::{auth::AuthRepository, guild_data_transfer},
    state::{AppState, AuthRuntime},
};

const DATA_TRANSFER_RETENTION_INTERVAL: Duration = Duration::from_secs(60);

pub async fn run(config: Config) -> Result<(), BootstrapError> {
    let bind_addr = config.bind_addr();
    let environment = config.environment().as_str();
    let realtime = RealtimeRuntime::from_config(&config)?;
    let mut state = if let Some(database_config) = config.database() {
        let pool = postgres::connect(database_config).await?;
        let mut probe = PostgresReadinessProbe::new(pool.clone(), database_config.probe_timeout());
        if let Some(valkey) = realtime.as_ref().and_then(RealtimeRuntime::valkey_client) {
            probe = probe.with_valkey(valkey);
        }
        AppState::with_postgres(config, pool, Arc::new(probe))
    } else {
        AppState::phase_one(config)
    };
    if let Some(auth_config) = state.config().auth().cloned() {
        let postgres = state
            .postgres()
            .cloned()
            .ok_or(BootstrapError::AuthRequiresPostgres)?;
        let callback_url = Url::parse(&state.config().oauth_callback_url())
            .map_err(|_| BootstrapError::InvalidOAuthCallbackUrl)?;
        let cookie_policy =
            CookiePolicy::new(auth_config.secure_cookies(), auth_config.session_ttl());
        let auth_service = AuthService::new(auth_config.oauth_encryption_key(), cookie_policy)?;
        let auth_repository = AuthRepository::new(postgres);
        let discord = ReqwestDiscordClient::new(
            auth_config.discord_client_id().to_owned(),
            auth_config.discord_client_secret().clone(),
            callback_url,
        )?;
        state = state.with_auth(AuthRuntime::new(
            Arc::new(auth_service),
            Arc::new(auth_repository),
            Arc::new(discord),
        ));
    }

    if let Some(riot_config) = state.config().riot_service().cloned() {
        let riot =
            ReqwestRiotClient::new(riot_config.base_url().clone(), riot_config.token().clone())?;
        state = state.with_riot(Arc::new(riot));
    }

    match (state.postgres().cloned(), realtime) {
        (Some(pool), Some(realtime)) => {
            realtime.start(pool);
            state = state.with_realtime(realtime);
        }
        (Some(_), None) => {
            tracing::warn!(
                environment,
                "realtime WebSocket is disabled because MONKEYBOT_VALKEY_URL is not configured"
            );
        }
        (None, _) => {}
    }

    let app = build_router(state.clone());
    let listener = TcpListener::bind(bind_addr)
        .await
        .map_err(BootstrapError::Bind)?;
    let data_transfer_retention = state.postgres().cloned().map(start_data_transfer_retention);

    tracing::info!(%bind_addr, environment, "monkeybot-api listening");
    let result = serve_with_shutdown(listener, app, state, shutdown_signal()).await;
    if let Some(task) = data_transfer_retention {
        task.abort();
        let _ = task.await;
    }
    result
}

fn start_data_transfer_retention(pool: sqlx::PgPool) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = interval(DATA_TRANSFER_RETENTION_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            match guild_data_transfer::cleanup_expired_previews(&pool, OffsetDateTime::now_utc())
                .await
            {
                Ok(removed) if removed > 0 => {
                    tracing::info!(removed, "removed expired guild data transfer previews");
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(%error, "failed to remove expired guild data transfer previews");
                }
            }
        }
    })
}

pub async fn serve_with_shutdown<F>(
    listener: TcpListener,
    app: Router,
    state: AppState,
    shutdown: F,
) -> Result<(), BootstrapError>
where
    F: Future<Output = ()> + Send + 'static,
{
    let drain_timeout = state.config().shutdown_timeout();
    let readiness = state.readiness().clone();
    let postgres = state.postgres().cloned();
    let realtime = state.realtime().cloned();
    drop(state);
    let (shutdown_started_tx, shutdown_started_rx) = watch::channel(None::<Instant>);
    let graceful_signal = async move {
        shutdown.await;
        readiness.begin_shutdown();
        let _ = shutdown_started_tx.send(Some(Instant::now() + drain_timeout));
    };

    let server = axum::serve(listener, app)
        .with_graceful_shutdown(graceful_signal)
        .into_future();
    tokio::pin!(server);

    let mut drain_deadline_rx = shutdown_started_rx.clone();
    let drain_deadline = async move {
        loop {
            let deadline = *drain_deadline_rx.borrow();
            if let Some(deadline) = deadline {
                sleep_until(deadline).await;
                return;
            }
            if drain_deadline_rx.changed().await.is_err() {
                std::future::pending::<()>().await;
            }
        }
    };
    tokio::pin!(drain_deadline);

    tokio::select! {
        result = &mut server => {
            let server_result = result.map_err(BootstrapError::Serve);
            if let Some(realtime) = &realtime {
                realtime.shutdown();
            }
            if let Some(pool) = postgres {
                let shutdown_deadline = *shutdown_started_rx.borrow();
                close_pool(pool, shutdown_deadline, drain_timeout).await?;
            }
            server_result
        },
        () = &mut drain_deadline => {
            if let Some(realtime) = &realtime {
                realtime.shutdown();
            }
            Err(BootstrapError::DrainTimeout(drain_timeout))
        },
    }
}

async fn close_pool(
    pool: sqlx::PgPool,
    shutdown_deadline: Option<Instant>,
    drain_timeout: Duration,
) -> Result<(), BootstrapError> {
    let closed = if let Some(deadline) = shutdown_deadline {
        timeout_at(deadline, pool.close()).await
    } else {
        timeout(drain_timeout, pool.close()).await
    };
    closed.map_err(|_| BootstrapError::DrainTimeout(drain_timeout))
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        match signal(SignalKind::terminate()) {
            Ok(mut terminate) => {
                tokio::select! {
                    result = tokio::signal::ctrl_c() => log_signal_error(result),
                    _ = terminate.recv() => {},
                }
            }
            Err(error) => {
                tracing::warn!(%error, "failed to register SIGTERM handler");
                log_signal_error(tokio::signal::ctrl_c().await);
            }
        }
    }

    #[cfg(not(unix))]
    log_signal_error(tokio::signal::ctrl_c().await);
}

fn log_signal_error(result: Result<(), std::io::Error>) {
    if let Err(error) = result {
        tracing::error!(%error, "failed to listen for shutdown signal");
    }
}

#[derive(Debug, Error)]
pub enum BootstrapError {
    #[error("failed to bind API listener: {0}")]
    Bind(#[source] std::io::Error),
    #[error("PostgreSQL initialization failed")]
    Postgres(#[from] PostgresError),
    #[error("authentication requires PostgreSQL")]
    AuthRequiresPostgres,
    #[error("OAuth callback URL initialization failed")]
    InvalidOAuthCallbackUrl,
    #[error("authentication service initialization failed")]
    Auth(#[from] AuthCryptoError),
    #[error("Discord OAuth client initialization failed")]
    Discord(#[from] DiscordClientError),
    #[error("Riot integration client initialization failed")]
    Riot(#[from] RiotClientError),
    #[error("realtime runtime initialization failed")]
    Realtime(#[from] RealtimeRuntimeError),
    #[error("API server failed: {0}")]
    Serve(#[source] std::io::Error),
    #[error("graceful shutdown exceeded {0:?}")]
    DrainTimeout(Duration),
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
    };

    use axum::{Router, routing::get};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
        sync::{Notify, oneshot},
    };

    use super::*;
    use crate::{
        config::Config,
        state::{DependencySnapshot, IdGenerator, StaticProbe},
    };

    #[derive(Debug)]
    struct FixedId;

    impl IdGenerator for FixedId {
        fn next_id(&self) -> String {
            "01900000-0000-7000-8000-000000000001".to_owned()
        }
    }

    fn test_config(shutdown_timeout_ms: u64) -> Config {
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
                "MONKEYBOT_SHUTDOWN_TIMEOUT_MS".to_owned(),
                shutdown_timeout_ms.to_string(),
            ),
        ]);
        Config::from_map(&values).expect("test config should parse")
    }

    #[tokio::test]
    async fn graceful_shutdown_marks_readiness_false_and_drains_request() {
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let completed = Arc::new(AtomicBool::new(false));
        let route_started = started.clone();
        let route_release = release.clone();
        let route_completed = completed.clone();
        let config = test_config(1_000);
        let state = AppState::new(
            config,
            Arc::new(StaticProbe::new(DependencySnapshot::ready())),
            Arc::new(FixedId),
        );
        let app = Router::new()
            .route(
                "/slow",
                get(move || {
                    let started = route_started.clone();
                    let release = route_release.clone();
                    let completed = route_completed.clone();
                    async move {
                        started.notify_one();
                        release.notified().await;
                        completed.store(true, Ordering::Release);
                        "ok"
                    }
                }),
            )
            .merge(build_router(state.clone()));
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let address = listener.local_addr().expect("listener address");
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_state = state.clone();
        let server = tokio::spawn(async move {
            serve_with_shutdown(listener, app, server_state, async {
                let _ = shutdown_rx.await;
            })
            .await
        });

        let mut stream = TcpStream::connect(address).await.expect("connect");
        stream
            .write_all(b"GET /slow HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .expect("write request");
        started.notified().await;
        shutdown_tx.send(()).expect("send shutdown");
        tokio::time::timeout(Duration::from_millis(100), async {
            while state.readiness().is_accepting_requests() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("readiness should turn false");
        assert!(!state.readiness().is_accepting_requests());

        release.notify_one();
        let mut response = Vec::new();
        stream
            .read_to_end(&mut response)
            .await
            .expect("read response");
        assert!(String::from_utf8_lossy(&response).contains("200 OK"));
        assert!(completed.load(Ordering::Acquire));
        server.await.expect("join server").expect("server result");
    }

    #[tokio::test]
    async fn graceful_shutdown_has_a_bounded_drain_timeout() {
        let started = Arc::new(Notify::new());
        let route_started = started.clone();
        let config = test_config(10);
        let state = AppState::new(
            config,
            Arc::new(StaticProbe::new(DependencySnapshot::ready())),
            Arc::new(FixedId),
        );
        let app = Router::new()
            .route(
                "/never-finishes",
                get(move || {
                    let started = route_started.clone();
                    async move {
                        started.notify_one();
                        std::future::pending::<&'static str>().await
                    }
                }),
            )
            .merge(build_router(state.clone()));
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let address = listener.local_addr().expect("listener address");
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            serve_with_shutdown(listener, app, state, async {
                let _ = shutdown_rx.await;
            })
            .await
        });

        let mut stream = TcpStream::connect(address).await.expect("connect");
        stream
            .write_all(
                b"GET /never-finishes HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
            )
            .await
            .expect("write request");
        started.notified().await;
        shutdown_tx.send(()).expect("send shutdown");

        let result = server.await.expect("join server");

        assert!(matches!(result, Err(BootstrapError::DrainTimeout(_))));
    }
}
