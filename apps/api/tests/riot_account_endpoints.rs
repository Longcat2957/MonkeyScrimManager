use std::{collections::HashMap, error::Error, sync::Arc, time::Duration};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, Response, StatusCode, header},
};
use futures_util::future::BoxFuture;
use monkeybot_api::{
    auth::{AuthService, CookiePolicy},
    build_router,
    config::Config,
    domain::DiscordUserId,
    infrastructure::postgres::{MIGRATOR, PostgresReadinessProbe},
    integrations::{
        discord::{DiscordClient, DiscordClientError, DiscordUser},
        riot::{
            AccountLookup, RecordsRequest, ResolveAccountRequest, ResolvedRiotAccount, RiotAccount,
            RiotCacheMetadata, RiotCacheState, RiotChampionAggregate, RiotChampionAggregateSample,
            RiotClient, RiotClientError, RiotMatchParticipant, RiotMatchTeam, RiotPlatform,
            RiotRank, RiotRecentMatch, RiotRecentMatchSample, RiotRecentMatchesPage,
            RiotRecentMatchesPageMetadata, RiotRecords, RiotRecordsSample, RiotRoutingRegion,
            RiotTeamObjectives,
        },
    },
    repository::auth::{AuthRepository, NewLogin},
    state::{AppState, AuthRuntime},
};
use reqwest::Url;
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;
use testcontainers::{
    ContainerAsync, GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use time::OffsetDateTime;
use tower::ServiceExt;
use uuid::Uuid;

const POSTGRES_IMAGE: &str = "postgres";
const POSTGRES_TAG: &str = "18.4-alpine3.23";
const POSTGRES_PASSWORD: &str = "monkeybot-riot-endpoint-password";
const DATABASE: &str = "monkeybot_riot_endpoints";
const USER_ID: &str = "100000000000000001";
const DISCORD_CLIENT_ID: &str = "900000000000000001";
const PUBLIC_ORIGIN: &str = "http://localhost:5173";
const OAUTH_KEY_BASE64URL: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const SESSION_TOKEN: &str = "riot-endpoint-session-token";
const CSRF_TOKEN: &str = "riot-endpoint-csrf-token";

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn riot_account_flow_uses_canonical_provider_identity_and_full_snapshots() -> TestResult {
    tokio::time::timeout(Duration::from_secs(120), run_riot_account_flow())
        .await
        .map_err(|_| "Riot account endpoint integration test exceeded 120 seconds")??;
    Ok(())
}

async fn run_riot_account_flow() -> TestResult {
    let server = TestPostgres::start().await?;
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&server.database_url())
        .await?;
    MIGRATOR.run(&pool).await?;

    let config = test_config(server.database_url())?;
    let auth_config = config.auth().cloned().expect("test auth config");
    let auth_service = Arc::new(AuthService::new(
        auth_config.oauth_encryption_key(),
        CookiePolicy::new(auth_config.secure_cookies(), auth_config.session_ttl()),
    )?);
    let auth_repository = Arc::new(AuthRepository::new(pool.clone()));
    auth_repository
        .complete_login(&NewLogin {
            user_id: DiscordUserId::parse(USER_ID)?,
            display_name: "Longcat".to_owned(),
            avatar_hash: None,
            previous_session_token_hash: None,
            session_id: Uuid::now_v7(),
            token_hash: AuthService::hash_secret(SESSION_TOKEN),
            csrf_secret_hash: AuthService::hash_secret(CSRF_TOKEN),
            expires_at: OffsetDateTime::now_utc() + time::Duration::hours(1),
        })
        .await?;

    let state = AppState::with_postgres(
        config,
        pool.clone(),
        Arc::new(PostgresReadinessProbe::new(
            pool.clone(),
            Duration::from_secs(1),
        )),
    )
    .with_auth(AuthRuntime::new(
        auth_service,
        auth_repository,
        Arc::new(FakeDiscordClient),
    ))
    .with_riot(Arc::new(FakeRiotClient));
    let app = build_router(state);

    let rejected = mutation(
        &app,
        Method::POST,
        "/api/me/riot-accounts",
        &json!({
            "gameName": "Alias One",
            "tagLine": "KR1",
            "platform": "kr",
            "puuid": "client-controlled"
        }),
    )
    .await?;
    assert_eq!(rejected.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let created = mutation(
        &app,
        Method::POST,
        "/api/me/riot-accounts",
        &json!({"gameName":"Alias One","tagLine":"kr1","platform":"kr"}),
    )
    .await?;
    assert_eq!(created.status(), StatusCode::CREATED);
    let created = body_json(created).await?;
    assert_eq!(created["data"]["riotAccounts"].as_array().unwrap().len(), 1);
    assert_eq!(created["data"]["riotAccounts"][0]["puuid"], "puuid-one");
    assert_eq!(
        created["data"]["riotAccounts"][0]["gameName"],
        "Canonical One"
    );
    assert_eq!(created["data"]["riotAccounts"][0]["platform"], "KR");
    assert_eq!(created["data"]["riotAccounts"][0]["isMain"], true);

    let normalized = sqlx::query_as::<_, (String, String)>(
        "SELECT normalized_game_name, normalized_tag_line
         FROM riot_accounts WHERE puuid = 'puuid-one'",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(normalized, ("canonical one".to_owned(), "kr1".to_owned()));

    let second = mutation(
        &app,
        Method::POST,
        "/api/me/riot-accounts",
        &json!({"gameName":"Second","tagLine":"KR2","platform":"KR"}),
    )
    .await?;
    let second = body_json(second).await?;
    assert_eq!(second["data"]["riotAccounts"].as_array().unwrap().len(), 2);
    assert_eq!(second["data"]["riotAccounts"][0]["puuid"], "puuid-one");

    let promoted = mutation(
        &app,
        Method::PUT,
        "/api/me/riot-accounts/puuid-two/main",
        &json!({}),
    )
    .await?;
    let promoted = body_json(promoted).await?;
    assert_eq!(promoted["data"]["riotAccounts"][0]["puuid"], "puuid-two");
    assert_eq!(promoted["data"]["riotAccounts"][0]["isMain"], true);

    let deleted = mutation(
        &app,
        Method::DELETE,
        "/api/me/riot-accounts/puuid-two",
        &json!({}),
    )
    .await?;
    let deleted = body_json(deleted).await?;
    assert_eq!(deleted["data"]["riotAccounts"].as_array().unwrap().len(), 1);
    assert_eq!(deleted["data"]["riotAccounts"][0]["puuid"], "puuid-one");
    assert_eq!(deleted["data"]["riotAccounts"][0]["isMain"], true);

    let refreshed = mutation(
        &app,
        Method::POST,
        "/api/me/riot-accounts/puuid-one/refresh",
        &json!({}),
    )
    .await?;
    let refreshed = body_json(refreshed).await?;
    assert_eq!(
        refreshed["data"]["riotAccounts"][0]["gameName"],
        "Canonical Renamed"
    );
    assert_eq!(refreshed["data"]["riotAccounts"][0]["profileIconId"], 22);

    let records = app
        .clone()
        .oneshot(
            Request::get("/api/riot/records?gameName=Alias%20One&tagLine=KR1&platform=kr")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(records.status(), StatusCode::OK);
    let records = body_json(records).await?;
    assert_eq!(records["data"]["account"]["puuid"], "puuid-one");
    assert_eq!(records["data"]["cache"]["state"], "fresh");
    assert_eq!(
        records["data"]["recentMatches"].as_array().unwrap().len(),
        10
    );
    assert_eq!(
        records["data"]["championAggregates"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    assert_eq!(records["data"]["championAggregates"][0]["games"], 10);
    assert_eq!(
        records["data"]["sample"]["championAggregates"]["analyzedMatches"],
        35
    );
    assert_eq!(records["data"]["sample"]["recentMatches"]["requested"], 10);
    assert_eq!(records["data"]["ranks"][0]["queueType"], "RANKED_SOLO_5x5");
    assert_eq!(records["data"]["ranks"][0]["tier"], "PLATINUM");
    assert_eq!(
        records["data"]["recentMatches"][0]["teams"][0]["objectives"]["towerKills"],
        11
    );
    assert_eq!(records["data"]["championAggregates"][0]["pentaKills"], 10);

    let more_records = app
		.clone()
		.oneshot(
			Request::get(
				"/api/riot/records/matches?gameName=Alias%20One&tagLine=KR1&platform=kr&offset=10&count=10",
			)
			.body(Body::empty())?,
		)
		.await?;
    assert_eq!(more_records.status(), StatusCode::OK);
    let more_records = body_json(more_records).await?;
    assert_eq!(
        more_records["data"]["matches"].as_array().unwrap().len(),
        10
    );
    assert_eq!(more_records["data"]["page"]["nextOffset"], 20);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM riot_records_recent_pages")
            .fetch_one(&pool)
            .await?,
        1
    );

    let unauthenticated_refresh = app
        .clone()
        .oneshot(
            Request::post("/api/riot/records/refresh")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&json!({
                    "gameName": "Alias One",
                    "tagLine": "KR1",
                    "platform": "kr"
                }))?))?,
        )
        .await?;
    assert_eq!(unauthenticated_refresh.status(), StatusCode::UNAUTHORIZED);

    let refreshed_records = mutation(
        &app,
        Method::POST,
        "/api/riot/records/refresh",
        &json!({"gameName":"Alias One","tagLine":"KR1","platform":"kr"}),
    )
    .await?;
    assert_eq!(refreshed_records.status(), StatusCode::TOO_MANY_REQUESTS);
    let cooldown = body_json(refreshed_records).await?;
    assert_eq!(cooldown["error"]["code"], "riot_records_refresh_cooldown");

    sqlx::query(
        "UPDATE riot_records_snapshots
		 SET refreshed_at = clock_timestamp() - INTERVAL '31 seconds'",
    )
    .execute(&pool)
    .await?;
    let refreshed_records = mutation(
        &app,
        Method::POST,
        "/api/riot/records/refresh",
        &json!({"gameName":"Alias One","tagLine":"KR1","platform":"kr"}),
    )
    .await?;
    assert_eq!(refreshed_records.status(), StatusCode::OK);
    let refreshed_records = body_json(refreshed_records).await?;
    assert_eq!(refreshed_records["data"]["cache"]["state"], "refreshed");
    assert_eq!(
        refreshed_records["data"]["recentMatches"]
            .as_array()
            .unwrap()
            .len(),
        20
    );
    assert_eq!(
        refreshed_records["data"]["sample"]["recentMatches"]["requested"],
        20
    );
    assert_eq!(
        refreshed_records["data"]["championAggregates"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM riot_records_recent_pages")
            .fetch_one(&pool)
            .await?,
        0
    );

    let main_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM riot_accounts WHERE user_id = $1 AND is_main",
    )
    .bind(USER_ID)
    .fetch_one(&pool)
    .await?;
    assert_eq!(main_count, 1);

    pool.close().await;
    drop(server);
    Ok(())
}

async fn mutation(
    app: &Router,
    method: Method,
    uri: &str,
    body: &Value,
) -> TestResult<Response<Body>> {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, PUBLIC_ORIGIN)
        .header("x-csrf-token", CSRF_TOKEN)
        .header(
            header::COOKIE,
            format!("monkeybot_session={SESSION_TOKEN}; monkeybot_csrf={CSRF_TOKEN}"),
        )
        .body(Body::from(serde_json::to_vec(body)?))?;
    Ok(app.clone().oneshot(request).await?)
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
            "MONKEYBOT_DISCORD_CLIENT_ID".to_owned(),
            DISCORD_CLIENT_ID.to_owned(),
        ),
        (
            "MONKEYBOT_DISCORD_CLIENT_SECRET".to_owned(),
            "fake-discord-secret".to_owned(),
        ),
        (
            "MONKEYBOT_OAUTH_ENCRYPTION_KEY".to_owned(),
            OAUTH_KEY_BASE64URL.to_owned(),
        ),
        (
            "MONKEYBOT_SESSION_TTL_SECONDS".to_owned(),
            "3600".to_owned(),
        ),
    ]);
    Ok(Config::from_map(&values)?)
}

#[derive(Debug)]
struct FakeDiscordClient;

impl DiscordClient for FakeDiscordClient {
    fn authorization_url(
        &self,
        _state: &str,
        _pkce_challenge: &str,
    ) -> Result<Url, DiscordClientError> {
        Err(DiscordClientError::InvalidConfiguration)
    }

    fn authenticate<'a>(
        &'a self,
        _code: &'a str,
        _pkce_verifier: &'a SecretString,
    ) -> BoxFuture<'a, Result<DiscordUser, DiscordClientError>> {
        Box::pin(async { Err(DiscordClientError::InvalidConfiguration) })
    }
}

#[derive(Debug)]
struct FakeRiotClient;

impl RiotClient for FakeRiotClient {
    fn resolve_account<'a>(
        &'a self,
        request: &'a ResolveAccountRequest,
    ) -> BoxFuture<'a, Result<ResolvedRiotAccount, RiotClientError>> {
        Box::pin(async move {
            let (puuid, game_name, tag_line, icon) = match &request.lookup {
                AccountLookup::RiotId { game_name, .. } if game_name == "Second" => {
                    ("puuid-two", "Canonical Two", "KR2", 12)
                }
                AccountLookup::RiotId { .. } => ("puuid-one", "Canonical One", "KR1", 11),
                AccountLookup::Puuid { puuid } if puuid == "puuid-one" => {
                    ("puuid-one", "Canonical Renamed", "KR1", 22)
                }
                AccountLookup::Puuid { .. } => return Err(RiotClientError::NotFound),
            };
            Ok(resolved_account(
                puuid,
                game_name,
                tag_line,
                icon,
                request.platform,
            ))
        })
    }

    fn records<'a>(
        &'a self,
        request: &'a RecordsRequest,
    ) -> BoxFuture<'a, Result<RiotRecords, RiotClientError>> {
        Box::pin(async move { Ok(records_snapshot(request.platform, RiotCacheState::Fresh)) })
    }

    fn refresh_records<'a>(
        &'a self,
        request: &'a RecordsRequest,
    ) -> BoxFuture<'a, Result<RiotRecords, RiotClientError>> {
        Box::pin(async move {
            Ok(records_snapshot(
                request.platform,
                RiotCacheState::Refreshed,
            ))
        })
    }

    fn recent_matches_page<'a>(
        &'a self,
        request: &'a RecordsRequest,
        offset: usize,
        count: usize,
    ) -> BoxFuture<'a, Result<RiotRecentMatchesPage, RiotClientError>> {
        Box::pin(async move {
            Ok(RiotRecentMatchesPage {
                account: RiotAccount {
                    puuid: "puuid-one".to_owned(),
                    game_name: "Canonical One".to_owned(),
                    tag_line: "KR1".to_owned(),
                },
                platform: request.platform,
                routing_region: request.platform.routing_region(),
                target_puuid: "puuid-one".to_owned(),
                matches: (offset..offset + count).map(recent_match).collect(),
                page: RiotRecentMatchesPageMetadata {
                    offset,
                    requested: count,
                    returned: count,
                    has_more: true,
                    next_offset: Some(offset + count),
                },
            })
        })
    }
}

fn resolved_account(
    puuid: &str,
    game_name: &str,
    tag_line: &str,
    profile_icon_id: i32,
    platform: RiotPlatform,
) -> ResolvedRiotAccount {
    ResolvedRiotAccount {
        account: RiotAccount {
            puuid: puuid.to_owned(),
            game_name: game_name.to_owned(),
            tag_line: tag_line.to_owned(),
        },
        platform,
        routing_region: platform.routing_region(),
        profile_icon_id: Some(profile_icon_id),
        summoner_level: Some(300),
        synced_at: "2026-07-13T00:00:00Z".to_owned(),
    }
}

fn records_snapshot(platform: RiotPlatform, state: RiotCacheState) -> RiotRecords {
    let sample_size = if state == RiotCacheState::Refreshed {
        20
    } else {
        10
    };
    RiotRecords {
        account: RiotAccount {
            puuid: "puuid-one".to_owned(),
            game_name: "Canonical One".to_owned(),
            tag_line: "KR1".to_owned(),
        },
        platform,
        routing_region: RiotRoutingRegion::Asia,
        profile_icon_id: Some(11),
        summoner_level: Some(300),
        ranks: vec![
            RiotRank {
                queue_type: "RANKED_SOLO_5x5".to_owned(),
                tier: Some("PLATINUM".to_owned()),
                rank: Some("III".to_owned()),
                league_points: 23,
                wins: 22,
                losses: 29,
            },
            RiotRank {
                queue_type: "RANKED_FLEX_SR".to_owned(),
                tier: None,
                rank: None,
                league_points: 0,
                wins: 0,
                losses: 0,
            },
        ],
        champion_aggregates: season_aggregates(),
        recent_matches: (0..sample_size).map(recent_match).collect(),
        sample: RiotRecordsSample {
            recent_matches: RiotRecentMatchSample {
                requested: sample_size,
                returned: sample_size,
            },
            champion_aggregates: RiotChampionAggregateSample {
                season_start_unix_seconds: 1_735_689_600,
                discovered_matches: 35,
                analyzed_matches: 35,
                truncated: false,
            },
        },
        refreshed_at: "2026-07-13T00:00:00Z".to_owned(),
        cache: RiotCacheMetadata { state },
    }
}

fn season_aggregates() -> Vec<RiotChampionAggregate> {
    [
        ("Ahri", 10, 7),
        ("LeeSin", 8, 5),
        ("Gnar", 7, 4),
        ("Lux", 6, 3),
        ("Vi", 4, 2),
    ]
    .into_iter()
    .map(|(champion_name, games, wins)| RiotChampionAggregate {
        champion_name: champion_name.to_owned(),
        games,
        wins,
        kills: i64::from(games * 10),
        deaths: i64::from(games * 2),
        assists: i64::from(games * 8),
        creep_score: i64::from(games * 220),
        gold_earned: i64::from(games * 14_000),
        triple_kills: i64::from(games),
        quadra_kills: i64::from(games),
        penta_kills: i64::from(games),
    })
    .collect()
}

fn recent_match(index: usize) -> RiotRecentMatch {
    let target_won = index.is_multiple_of(2);
    RiotRecentMatch {
        match_id: format!("KR_{index}"),
        queue_id: 420,
        game_version: "16.13.1".to_owned(),
        duration_seconds: 1_800,
        started_at: "2026-07-13T00:00:00Z".to_owned(),
        target_puuid: "puuid-one".to_owned(),
        participants: vec![RiotMatchParticipant {
            puuid: "puuid-one".to_owned(),
            game_name: Some("Canonical One".to_owned()),
            tag_line: Some("KR1".to_owned()),
            champion_name: format!("Champion{index:02}"),
            champion_level: 18,
            team_id: 100,
            win: target_won,
            kills: 10,
            deaths: 2,
            assists: 8,
            creep_score: 220,
            gold_earned: 14_000,
            damage_dealt_to_champions: 30_000,
            damage_taken: 20_000,
            vision_score: 40,
            summoner_spell_ids: [4, 11],
            item_ids: [1001, 1002, 1003, 1004, 1005, 1006, 3364],
            triple_kills: 1,
            quadra_kills: 1,
            penta_kills: 1,
            team_position: "JUNGLE".to_owned(),
        }],
        teams: vec![
            match_team(100, target_won, 11, 3),
            match_team(200, !target_won, 3, 2),
        ],
    }
}

fn match_team(team_id: i32, win: bool, tower_kills: i32, dragon_kills: i32) -> RiotMatchTeam {
    RiotMatchTeam {
        team_id,
        win,
        objectives: RiotTeamObjectives {
            baron_kills: 1,
            champion_kills: if team_id == 100 { 40 } else { 25 },
            dragon_kills,
            horde_kills: 3,
            inhibitor_kills: 2,
            rift_herald_kills: 1,
            tower_kills,
        },
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
