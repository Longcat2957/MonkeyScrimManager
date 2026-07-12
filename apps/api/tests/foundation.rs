use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

#[cfg(unix)]
use std::{
    net::TcpListener as StdTcpListener,
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use futures_util::future::BoxFuture;
use monkeybot_api::{
    AppState, DependencySnapshot, IdGenerator, StaticProbe, build_router, config::Config,
    state::DependencyProbe,
};
use serde_json::Value;
#[cfg(unix)]
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use tower::ServiceExt;

const FIXED_REQUEST_ID: &str = "01900000-0000-7000-8000-000000000001";

#[derive(Debug)]
struct FixedId;

impl IdGenerator for FixedId {
    fn next_id(&self) -> String {
        FIXED_REQUEST_ID.to_owned()
    }
}

struct CountingProbe {
    checks: Arc<AtomicUsize>,
    snapshot: DependencySnapshot,
}

struct SensitiveProbe {
    snapshot: DependencySnapshot,
    dsn: String,
    password: String,
    raw_error: String,
}

impl DependencyProbe for SensitiveProbe {
    fn check(&self) -> BoxFuture<'_, DependencySnapshot> {
        std::hint::black_box((&self.dsn, &self.password, &self.raw_error));
        Box::pin(async move { self.snapshot.clone() })
    }
}

impl DependencyProbe for CountingProbe {
    fn check(&self) -> BoxFuture<'_, DependencySnapshot> {
        self.checks.fetch_add(1, Ordering::AcqRel);
        Box::pin(async move { self.snapshot.clone() })
    }
}

fn config() -> Config {
    Config::from_map(&HashMap::from([
        ("MONKEYBOT_ENV".to_owned(), "test".to_owned()),
        (
            "MONKEYBOT_BIND_ADDR".to_owned(),
            "127.0.0.1:30020".to_owned(),
        ),
        (
            "MONKEYBOT_PUBLIC_ORIGIN".to_owned(),
            "http://localhost:5173".to_owned(),
        ),
    ]))
    .expect("test config should parse")
}

fn state(snapshot: DependencySnapshot) -> AppState {
    AppState::new(
        config(),
        Arc::new(StaticProbe::new(snapshot)),
        Arc::new(FixedId),
    )
}

async fn json(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    serde_json::from_slice(&bytes).expect("JSON response")
}

#[tokio::test]
async fn liveness_uses_the_same_request_id_in_header_and_envelope() {
    let response = build_router(state(DependencySnapshot::phase_one()))
        .oneshot(
            Request::get("/healthz")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["x-request-id"], FIXED_REQUEST_ID);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    let body = json(response).await;
    assert_eq!(body["data"]["status"], "ok");
    assert_eq!(body["meta"]["requestId"], FIXED_REQUEST_ID);
}

#[tokio::test]
async fn liveness_does_not_call_dependency_probe() {
    let checks = Arc::new(AtomicUsize::new(0));
    let probe = CountingProbe {
        checks: checks.clone(),
        snapshot: DependencySnapshot::ready(),
    };
    let app = build_router(AppState::new(config(), Arc::new(probe), Arc::new(FixedId)));

    let response = app
        .oneshot(
            Request::get("/healthz")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(checks.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn phase_one_readiness_is_honestly_unavailable() {
    let response = build_router(state(DependencySnapshot::phase_one()))
        .oneshot(
            Request::get("/readyz")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = json(response).await;
    assert_eq!(body["error"]["code"], "service_not_ready");
    assert_eq!(
        body["error"]["details"]["dependencies"]["postgres"],
        "not_configured"
    );
}

#[tokio::test]
async fn valkey_outage_is_degraded_but_ready() {
    let response = build_router(state(DependencySnapshot::ready_with_valkey_unavailable()))
        .oneshot(
            Request::get("/readyz")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = json(response).await;
    assert_eq!(body["data"]["status"], "degraded");
    assert_eq!(body["data"]["dependencies"]["valkey"], "unavailable");
}

#[tokio::test]
async fn health_responses_do_not_expose_dependency_secrets_or_raw_errors() {
    const DSN: &str = "postgresql://monkeybot:dsn-secret@db.internal/monkeybot";
    const PASSWORD: &str = "health-password-sentinel";
    const RAW_ERROR: &str = "connection refused: db.internal:5432; ssl key=/run/secrets/db.pem";

    let probe = SensitiveProbe {
        snapshot: DependencySnapshot::phase_one(),
        dsn: DSN.to_owned(),
        password: PASSWORD.to_owned(),
        raw_error: RAW_ERROR.to_owned(),
    };
    let app = build_router(AppState::new(config(), Arc::new(probe), Arc::new(FixedId)));

    for path in ["/healthz", "/readyz"] {
        let response = app
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).expect("request"))
            .await
            .expect("response");
        let body = json(response).await.to_string();

        assert!(!body.contains(DSN));
        assert!(!body.contains(PASSWORD));
        assert!(!body.contains(RAW_ERROR));
        assert!(!body.contains("dsn-secret"));
        assert!(!body.contains("/run/secrets/db.pem"));
    }
}

#[tokio::test]
async fn not_found_and_method_not_allowed_use_error_envelope() {
    let app = build_router(state(DependencySnapshot::ready()));
    let not_found = app
        .clone()
        .oneshot(
            Request::get("/missing")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let method_not_allowed = app
        .oneshot(
            Request::post("/healthz")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(not_found.status(), StatusCode::NOT_FOUND);
    assert_eq!(json(not_found).await["error"]["code"], "not_found");
    assert_eq!(method_not_allowed.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(
        json(method_not_allowed).await["error"]["code"],
        "method_not_allowed"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn child_binary_handles_sigterm_and_exits_cleanly() {
    let reserved_listener = StdTcpListener::bind("127.0.0.1:0").expect("reserve test port");
    let address = reserved_listener
        .local_addr()
        .expect("reserved listener address");
    drop(reserved_listener);

    let child = Command::new(env!("CARGO_BIN_EXE_monkeybot-api"))
        .env_clear()
        .env("MONKEYBOT_ENV", "test")
        .env("MONKEYBOT_BIND_ADDR", address.to_string())
        .env("MONKEYBOT_PUBLIC_ORIGIN", "http://localhost:5173")
        .env("MONKEYBOT_LOG_FILTER", "off")
        .env("MONKEYBOT_SHUTDOWN_TIMEOUT_MS", "1000")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn monkeybot-api child");
    let mut child = ChildGuard::new(child);

    wait_for_health(address, child.child_mut(), Duration::from_secs(5)).await;
    send_sigterm(child.id());
    let status = wait_for_exit(child.child_mut(), Duration::from_secs(5)).await;

    assert!(status.success(), "SIGTERM exit status was {status}");
}

#[cfg(unix)]
struct ChildGuard {
    child: Child,
}

#[cfg(unix)]
impl ChildGuard {
    fn new(child: Child) -> Self {
        Self { child }
    }

    fn id(&self) -> u32 {
        self.child.id()
    }

    fn child_mut(&mut self) -> &mut Child {
        &mut self.child
    }
}

#[cfg(unix)]
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None) | Err(_)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[cfg(unix)]
async fn wait_for_health(address: std::net::SocketAddr, child: &mut Child, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().expect("query child status") {
            panic!("monkeybot-api exited before health check: {status}");
        }

        let health_request = async {
            let mut stream = TcpStream::connect(address).await?;
            stream
                .write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .await?;
            let mut response = Vec::new();
            stream.read_to_end(&mut response).await?;
            Ok::<_, std::io::Error>(response)
        };
        if let Ok(Ok(response)) =
            tokio::time::timeout(Duration::from_millis(250), health_request).await
            && response.starts_with(b"HTTP/1.1 200 OK")
        {
            return;
        }

        assert!(Instant::now() < deadline, "child health check timed out");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[cfg(unix)]
async fn wait_for_exit(child: &mut Child, timeout: Duration) -> ExitStatus {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().expect("query child status") {
            return status;
        }
        assert!(Instant::now() < deadline, "child SIGTERM exit timed out");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[cfg(unix)]
fn send_sigterm(process_id: u32) {
    const SIGTERM: i32 = 15;

    unsafe extern "C" {
        fn kill(process_id: i32, signal: i32) -> i32;
    }

    // SAFETY: `process_id` belongs to the live child spawned by this test and SIGTERM is valid.
    let result = unsafe { kill(process_id as i32, SIGTERM) };
    assert_eq!(result, 0, "send SIGTERM to child process");
}
