use std::{
    collections::HashMap,
    error::Error,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, Response, StatusCode, header},
};
use futures_util::future::BoxFuture;
use monkeybot_api::{
    build_router,
    config::Config,
    infrastructure::postgres::{MIGRATOR, PostgresReadinessProbe},
    integrations::riot::{
        AccountLookup, RecordsRequest, ResolveAccountRequest, ResolvedRiotAccount, RiotAccount,
        RiotClient, RiotClientError, RiotRecentMatchesPage, RiotRecords,
    },
    state::AppState,
};
use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;
use testcontainers::{
    ContainerAsync, GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use tower::ServiceExt;
use uuid::Uuid;

const POSTGRES_IMAGE: &str = "postgres";
const POSTGRES_TAG: &str = "18.4-alpine3.23";
const POSTGRES_PASSWORD: &str = "monkeybot-bot-registration-password";
const DATABASE: &str = "monkeybot_bot_registration";
const BOT_TOKEN: &str = "bot-service-token-that-is-at-least-32-characters";
const PUBLIC_ORIGIN: &str = "http://localhost:5173";
const GUILD_A: &str = "200000000000000001";
const GUILD_B: &str = "200000000000000002";
const GUILD_UNCONFIGURED: &str = "200000000000000003";
const GUILD_UNKNOWN: &str = "200000000000000004";
const OPERATOR_ID: &str = "100000000000000001";
const USER_ID: &str = "100000000000000002";
const OTHER_USER_ID: &str = "100000000000000003";
const OPERATOR_ROLE_A: &str = "300000000000000001";
const OPERATOR_ROLE_B: &str = "300000000000000002";

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bot_registration_is_idempotent_tenant_scoped_and_preserves_inactive_members() -> TestResult
{
    tokio::time::timeout(Duration::from_secs(180), run_bot_registration())
        .await
        .map_err(|_| "Bot registration integration test exceeded 180 seconds")??;
    Ok(())
}

async fn run_bot_registration() -> TestResult {
    let server = TestPostgres::start().await?;
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&server.database_url())
        .await?;
    MIGRATOR.run(&pool).await?;

    let riot = FakeRiotClient::default();
    let state = AppState::with_postgres(
        test_config(server.database_url())?,
        pool.clone(),
        Arc::new(PostgresReadinessProbe::new(
            pool.clone(),
            Duration::from_secs(1),
        )),
    )
    .with_riot(Arc::new(riot.clone()));
    let app = build_router(state);

    setup_guild(&app, GUILD_A, OPERATOR_ROLE_A, "setup-a").await?;
    setup_guild(&app, GUILD_B, OPERATOR_ROLE_B, "setup-b").await?;

    let guild_a_registration =
        registration_body(USER_ID, "Stable Global", "Guild A Nick", "Shared");
    let created = bot_request(
        &app,
        Method::POST,
        &format!("/internal/guilds/{GUILD_A}/registrations"),
        "register-a",
        &guild_a_registration,
    )
    .await?;
    assert_eq!(created.status(), StatusCode::OK);
    let created = body_json(created).await?;
    assert_eq!(created["data"]["registration"], "CREATED");
    assert_eq!(created["data"]["accountLink"], "CREATED");
    assert_eq!(
        created["data"]["member"],
        json!({"id":USER_ID,"displayName":"Guild A Nick","status":"ACTIVE"})
    );
    assert_eq!(
        created["data"]["riotAccount"],
        json!({
            "puuid":"puuid-shared",
            "gameName":"Canonical Shared",
            "tagLine":"KR1",
            "platform":"KR",
            "isMain":true
        })
    );
    assert_eq!(riot.resolve_calls(), 1);

    riot.set_failure(true);
    let calls_before_replay = riot.resolve_calls();
    let replay = bot_request(
        &app,
        Method::POST,
        &format!("/internal/guilds/{GUILD_A}/registrations"),
        "register-a",
        &guild_a_registration,
    )
    .await?;
    assert_eq!(replay.status(), StatusCode::OK);
    assert_eq!(body_json(replay).await?["data"], created["data"]);
    assert_eq!(riot.resolve_calls(), calls_before_replay);
    riot.set_failure(false);

    let repeated = bot_request(
        &app,
        Method::POST,
        &format!("/internal/guilds/{GUILD_A}/registrations"),
        "register-a-again",
        &guild_a_registration,
    )
    .await?;
    assert_eq!(repeated.status(), StatusCode::OK);
    let repeated = body_json(repeated).await?;
    assert_eq!(repeated["data"]["registration"], "EXISTING");
    assert_eq!(repeated["data"]["accountLink"], "EXISTING");

    let guild_b_registration =
        registration_body(USER_ID, "Stable Global", "Guild B Nick", "Shared");
    let other_guild = bot_request(
        &app,
        Method::POST,
        &format!("/internal/guilds/{GUILD_B}/registrations"),
        "register-b",
        &guild_b_registration,
    )
    .await?;
    assert_eq!(other_guild.status(), StatusCode::OK);
    let other_guild = body_json(other_guild).await?;
    assert_eq!(other_guild["data"]["registration"], "CREATED");
    assert_eq!(other_guild["data"]["accountLink"], "EXISTING");

    let identity = sqlx::query_as::<_, (String, i64, i64)>(
        "SELECT users.display_name,
                (SELECT count(*) FROM guild_members WHERE user_id = $1),
                (SELECT count(*) FROM riot_accounts WHERE user_id = $1)
         FROM users WHERE user_id = $1",
    )
    .bind(USER_ID)
    .fetch_one(&pool)
    .await?;
    assert_eq!(identity, ("Stable Global".to_owned(), 2, 1));
    let guild_names = sqlx::query_as::<_, (String, String)>(
        "SELECT guild_id, display_name
         FROM guild_members WHERE user_id = $1 ORDER BY guild_id",
    )
    .bind(USER_ID)
    .fetch_all(&pool)
    .await?;
    assert_eq!(
        guild_names,
        vec![
            (GUILD_A.to_owned(), "Guild A Nick".to_owned()),
            (GUILD_B.to_owned(), "Guild B Nick".to_owned()),
        ]
    );

    sqlx::query(
        "UPDATE guild_members
         SET status = 'INACTIVE', inactivated_at = clock_timestamp()
         WHERE guild_id = $1 AND user_id = $2",
    )
    .bind(GUILD_A)
    .bind(USER_ID)
    .execute(&pool)
    .await?;
    let calls_before_inactive = riot.resolve_calls();
    assert_error(
        bot_request(
            &app,
            Method::POST,
            &format!("/internal/guilds/{GUILD_A}/registrations"),
            "inactive-register",
            &registration_body(USER_ID, "Changed Global", "Changed Nick", "Shared"),
        )
        .await?,
        StatusCode::FORBIDDEN,
        "member_inactive",
        "비활성화된 길드 멤버는 이 작업을 수행할 수 없습니다.",
    )
    .await?;
    assert_eq!(riot.resolve_calls(), calls_before_inactive);
    let inactive = sqlx::query_as::<_, (String, String, String)>(
        "SELECT users.display_name, members.display_name, members.status
         FROM guild_members AS members
         JOIN users ON users.user_id = members.user_id
         WHERE members.guild_id = $1 AND members.user_id = $2",
    )
    .bind(GUILD_A)
    .bind(USER_ID)
    .fetch_one(&pool)
    .await?;
    assert_eq!(
        inactive,
        (
            "Stable Global".to_owned(),
            "Guild A Nick".to_owned(),
            "INACTIVE".to_owned(),
        )
    );

    assert_error(
        bot_request(
            &app,
            Method::POST,
            &format!("/internal/guilds/{GUILD_A}/registrations"),
            "other-owner",
            &registration_body(OTHER_USER_ID, "Other Global", "Other Nick", "Shared"),
        )
        .await?,
        StatusCode::CONFLICT,
        "riot_account_already_linked",
        "이 Riot 계정은 이미 다른 사용자에게 연결되어 있습니다.",
    )
    .await?;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM users WHERE user_id = $1")
            .bind(OTHER_USER_ID)
            .fetch_one(&pool)
            .await?,
        0
    );

    let conflict_key = "conflicting-interaction";
    let conflict_first = bot_request(
        &app,
        Method::POST,
        &format!("/internal/guilds/{GUILD_B}/registrations"),
        conflict_key,
        &registration_body(OPERATOR_ID, "Operator Global", "Operator B", "Other"),
    )
    .await?;
    assert_eq!(conflict_first.status(), StatusCode::OK);
    let calls_before_conflict = riot.resolve_calls();
    assert_error(
        bot_request(
            &app,
            Method::POST,
            &format!("/internal/guilds/{GUILD_B}/registrations"),
            conflict_key,
            &registration_body(OPERATOR_ID, "Operator Global", "Operator B", "Shared"),
        )
        .await?,
        StatusCode::CONFLICT,
        "idempotency_conflict",
        "같은 interaction ID가 다른 요청에 사용되었습니다.",
    )
    .await?;
    assert_eq!(riot.resolve_calls(), calls_before_conflict);

    let calls_before_unknown_guild = riot.resolve_calls();
    assert_error(
        bot_request(
            &app,
            Method::POST,
            &format!("/internal/guilds/{GUILD_UNKNOWN}/registrations"),
            "unknown-guild",
            &registration_body(OTHER_USER_ID, "Other Global", "Other Nick", "Other"),
        )
        .await?,
        StatusCode::CONFLICT,
        "guild_setup_required",
        "길드 초기 설정을 먼저 완료해야 합니다.",
    )
    .await?;
    assert_eq!(riot.resolve_calls(), calls_before_unknown_guild);

    sqlx::query(
        "INSERT INTO guilds (
             guild_id, discord_name, icon_hash, status, installed_by,
             installed_at, activated_at, left_at, updated_at
         ) VALUES ($1, 'Unconfigured', NULL, 'ACTIVE', $2,
                   clock_timestamp(), clock_timestamp(), NULL, clock_timestamp())",
    )
    .bind(GUILD_UNCONFIGURED)
    .bind(OPERATOR_ID)
    .execute(&pool)
    .await?;
    let calls_before_unconfigured_guild = riot.resolve_calls();
    assert_error(
        bot_request(
            &app,
            Method::POST,
            &format!("/internal/guilds/{GUILD_UNCONFIGURED}/registrations"),
            "unconfigured-guild",
            &registration_body(OTHER_USER_ID, "Other Global", "Other Nick", "Other"),
        )
        .await?,
        StatusCode::CONFLICT,
        "guild_setup_required",
        "길드 초기 설정을 먼저 완료해야 합니다.",
    )
    .await?;
    assert_eq!(riot.resolve_calls(), calls_before_unconfigured_guild);

    sqlx::query("UPDATE guilds SET status = 'DISABLED' WHERE guild_id = $1")
        .bind(GUILD_B)
        .execute(&pool)
        .await?;
    let calls_before_disabled_guild = riot.resolve_calls();
    assert_error(
        bot_request(
            &app,
            Method::POST,
            &format!("/internal/guilds/{GUILD_B}/registrations"),
            "disabled-guild",
            &registration_body(OTHER_USER_ID, "Other Global", "Other Nick", "Other"),
        )
        .await?,
        StatusCode::CONFLICT,
        "guild_inactive",
        "현재 길드에서는 command를 실행할 수 없습니다.",
    )
    .await?;
    assert_eq!(riot.resolve_calls(), calls_before_disabled_guild);

    pool.close().await;
    drop(server);
    Ok(())
}

async fn setup_guild(
    app: &Router,
    guild_id: &str,
    operator_role_id: &str,
    idempotency_key: &str,
) -> TestResult {
    let response = bot_request(
        app,
        Method::POST,
        &format!("/internal/guilds/{guild_id}/setup"),
        idempotency_key,
        &json!({
            "actor": actor(OPERATOR_ID, "Operator Global", "Guild Operator", &[operator_role_id]),
            "guildName": format!("Guild {guild_id}"),
            "guildIconHash": null,
            "operatorRoleId": operator_role_id,
            "recruitmentChannelId": "400000000000000001",
            "entryChannelId": null,
            "resultChannelId": null,
            "logChannelId": null,
            "seasonName": "2026 Summer"
        }),
    )
    .await?;
    assert_eq!(response.status(), StatusCode::OK);
    Ok(())
}

fn registration_body(
    user_id: &str,
    global_display_name: &str,
    guild_display_name: &str,
    game_name: &str,
) -> Value {
    json!({
        "actor": actor(user_id, global_display_name, guild_display_name, &[]),
        "gameName": game_name,
        "tagLine": "KR1",
        "platform": "KR"
    })
}

fn actor(user_id: &str, global_display_name: &str, display_name: &str, role_ids: &[&str]) -> Value {
    json!({
        "id": user_id,
        "globalDisplayName": global_display_name,
        "displayName": display_name,
        "avatarHash": null,
        "roleIds": role_ids
    })
}

async fn bot_request(
    app: &Router,
    method: Method,
    uri: &str,
    idempotency_key: &str,
    body: &Value,
) -> TestResult<Response<Body>> {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {BOT_TOKEN}"))
        .header(header::CONTENT_TYPE, "application/json")
        .header("idempotency-key", interaction_id(idempotency_key))
        .body(Body::from(serde_json::to_vec(body)?))?;
    Ok(app.clone().oneshot(request).await?)
}

fn interaction_id(label: &str) -> String {
    let hash = label.bytes().fold(1_u64, |hash, byte| {
        hash.wrapping_mul(1_099_511_628_211)
            .wrapping_add(u64::from(byte))
    });
    hash.max(1).to_string()
}

async fn assert_error(
    response: Response<Body>,
    status: StatusCode,
    code: &str,
    message: &str,
) -> TestResult {
    assert_eq!(response.status(), status);
    let body = body_json(response).await?;
    assert_eq!(body["error"]["code"], code);
    assert_eq!(body["error"]["message"], message);
    let request_id = body["error"]["requestId"]
        .as_str()
        .ok_or("error response did not contain a request ID")?;
    Uuid::parse_str(request_id)?;
    Ok(())
}

async fn body_json(response: Response<Body>) -> TestResult<Value> {
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn test_config(database_url: String) -> Result<Config, Box<dyn Error + Send + Sync>> {
    let values = HashMap::from([
        ("MONKEYBOT_ENV".to_owned(), "test".to_owned()),
        (
            "MONKEYBOT_BIND_ADDR".to_owned(),
            "127.0.0.1:30020".to_owned(),
        ),
        (
            "MONKEYBOT_PUBLIC_ORIGIN".to_owned(),
            PUBLIC_ORIGIN.to_owned(),
        ),
        ("MONKEYBOT_DATABASE_URL".to_owned(), database_url),
        (
            "MONKEYBOT_DATABASE_MAX_CONNECTIONS".to_owned(),
            "8".to_owned(),
        ),
        (
            "MONKEYBOT_DATABASE_MIN_CONNECTIONS".to_owned(),
            "1".to_owned(),
        ),
        (
            "MONKEYBOT_BOT_SERVICE_TOKEN".to_owned(),
            BOT_TOKEN.to_owned(),
        ),
    ]);
    Ok(Config::from_map(&values)?)
}

#[derive(Clone, Debug, Default)]
struct FakeRiotClient {
    resolve_calls: Arc<AtomicUsize>,
    fail_resolve: Arc<AtomicBool>,
}

impl FakeRiotClient {
    fn resolve_calls(&self) -> usize {
        self.resolve_calls.load(Ordering::SeqCst)
    }

    fn set_failure(&self, fail: bool) {
        self.fail_resolve.store(fail, Ordering::SeqCst);
    }
}

impl RiotClient for FakeRiotClient {
    fn resolve_account<'a>(
        &'a self,
        request: &'a ResolveAccountRequest,
    ) -> BoxFuture<'a, Result<ResolvedRiotAccount, RiotClientError>> {
        Box::pin(async move {
            self.resolve_calls.fetch_add(1, Ordering::SeqCst);
            if self.fail_resolve.load(Ordering::SeqCst) {
                return Err(RiotClientError::RateLimited {
                    retry_after_seconds: Some(30),
                });
            }
            let AccountLookup::RiotId { game_name, .. } = &request.lookup else {
                return Err(RiotClientError::NotFound);
            };
            let (puuid, canonical_name) = match game_name.as_str() {
                "Shared" => ("puuid-shared", "Canonical Shared"),
                "Other" => ("puuid-other", "Canonical Other"),
                _ => return Err(RiotClientError::NotFound),
            };
            Ok(ResolvedRiotAccount {
                account: RiotAccount {
                    puuid: puuid.to_owned(),
                    game_name: canonical_name.to_owned(),
                    tag_line: "KR1".to_owned(),
                },
                platform: request.platform,
                routing_region: request.platform.routing_region(),
                profile_icon_id: Some(11),
                summoner_level: Some(300),
                synced_at: "2026-07-13T00:00:00Z".to_owned(),
            })
        })
    }

    fn records<'a>(
        &'a self,
        _request: &'a RecordsRequest,
    ) -> BoxFuture<'a, Result<RiotRecords, RiotClientError>> {
        Box::pin(async { Err(RiotClientError::NotFound) })
    }

    fn refresh_records<'a>(
        &'a self,
        _request: &'a RecordsRequest,
    ) -> BoxFuture<'a, Result<RiotRecords, RiotClientError>> {
        Box::pin(async { Err(RiotClientError::NotFound) })
    }

    fn recent_matches_page<'a>(
        &'a self,
        _request: &'a RecordsRequest,
        _offset: usize,
        _count: usize,
    ) -> BoxFuture<'a, Result<RiotRecentMatchesPage, RiotClientError>> {
        Box::pin(async { Err(RiotClientError::NotFound) })
    }
}

struct TestPostgres {
    _container: ContainerAsync<GenericImage>,
    host: String,
    port: u16,
}

impl TestPostgres {
    async fn start() -> TestResult<Self> {
        let container = GenericImage::new(POSTGRES_IMAGE, POSTGRES_TAG)
            .with_exposed_port(5432.tcp())
            .with_wait_for(WaitFor::message_on_stderr(
                "database system is ready to accept connections",
            ))
            .with_env_var("POSTGRES_USER", "postgres")
            .with_env_var("POSTGRES_PASSWORD", POSTGRES_PASSWORD)
            .with_env_var("POSTGRES_DB", DATABASE)
            .start()
            .await?;
        let host = container.get_host().await?.to_string();
        let port = container.get_host_port_ipv4(5432.tcp()).await?;
        Ok(Self {
            _container: container,
            host,
            port,
        })
    }

    fn database_url(&self) -> String {
        format!(
            "postgresql://postgres:{POSTGRES_PASSWORD}@{}:{}/{DATABASE}",
            self.host, self.port
        )
    }
}
