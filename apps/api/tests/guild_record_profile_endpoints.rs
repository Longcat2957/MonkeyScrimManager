use std::{collections::HashMap, error::Error, sync::Arc, time::Duration};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, Response, StatusCode, header},
};
use futures_util::future::BoxFuture;
use monkeybot_api::{
    auth::{AuthService, CookiePolicy},
    build_router,
    config::Config,
    domain::DiscordUserId,
    infrastructure::postgres::{MIGRATOR, PostgresReadinessProbe},
    integrations::discord::{DiscordClient, DiscordClientError, DiscordUser},
    repository::auth::{AuthRepository, NewLogin},
    state::{AppState, AuthRuntime},
};
use reqwest::Url;
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
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
const POSTGRES_PASSWORD: &str = "monkeybot-guild-record-profile-password";
const DATABASE: &str = "monkeybot_guild_record_profile";

const VIEWER_ID: &str = "100000000000000001";
const OUTSIDER_ID: &str = "100000000000000002";
const VISIBLE_ID: &str = "100000000000000003";
const HIDDEN_ID: &str = "100000000000000004";
const MISSING_PRIVACY_ID: &str = "100000000000000005";
const INACTIVE_ID: &str = "100000000000000006";
const TWIN_A_ID: &str = "100000000000000007";
const TWIN_B_ID: &str = "100000000000000008";
const FILLER_1_ID: &str = "100000000000000009";
const FILLER_2_ID: &str = "100000000000000010";
const FILLER_3_ID: &str = "100000000000000011";
const FILLER_4_ID: &str = "100000000000000012";
const GUILD_A: &str = "200000000000000001";
const GUILD_B: &str = "200000000000000002";
const DISCORD_CLIENT_ID: &str = "900000000000000001";
const PUBLIC_ORIGIN: &str = "http://localhost:5173";
const OAUTH_KEY_BASE64URL: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const VIEWER_SESSION_TOKEN: &str = "guild-record-profile-viewer-session";
const OUTSIDER_SESSION_TOKEN: &str = "guild-record-profile-outsider-session";
const SEASON_ID: &str = "01900000-0000-7000-8000-000000000010";
const RECRUITMENT_1_ID: &str = "01900000-0000-7000-8000-000000000011";
const RECRUITMENT_2_ID: &str = "01900000-0000-7000-8000-000000000012";
const SERIES_1_ID: &str = "01900000-0000-7000-8000-000000000013";
const SERIES_2_ID: &str = "01900000-0000-7000-8000-000000000014";
const INPUT_1_ID: &str = "01900000-0000-7000-8000-000000000015";
const INPUT_2_ID: &str = "01900000-0000-7000-8000-000000000016";
const GAME_1_ID: &str = "01900000-0000-7000-8000-000000000017";
const GAME_2_ID: &str = "01900000-0000-7000-8000-000000000018";

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn guild_record_profile_is_tenant_scoped_ordered_and_privacy_filtered() -> TestResult {
    tokio::time::timeout(Duration::from_secs(120), run_profile_flow())
        .await
        .map_err(|_| "guild record profile endpoint integration test exceeded 120 seconds")??;
    Ok(())
}

async fn run_profile_flow() -> TestResult {
    let server = TestPostgres::start().await?;
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&server.database_url())
        .await?;
    MIGRATOR.run(&pool).await?;

    let config = test_config(server.database_url())?;
    let auth_config = config.auth().cloned().expect("test auth configuration");
    let auth_service = Arc::new(AuthService::new(
        auth_config.oauth_encryption_key(),
        CookiePolicy::new(auth_config.secure_cookies(), auth_config.session_ttl()),
    )?);
    let auth_repository = Arc::new(AuthRepository::new(pool.clone()));
    seed_login(&auth_repository, VIEWER_ID, "Viewer", VIEWER_SESSION_TOKEN).await?;
    seed_login(
        &auth_repository,
        OUTSIDER_ID,
        "Outsider",
        OUTSIDER_SESSION_TOKEN,
    )
    .await?;
    seed_profile_data(&pool).await?;
    seed_record_data(&pool).await?;
    seed_auction_record_data(&pool).await?;

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
        Arc::new(UnusedDiscordClient),
    ));
    let app = build_router(state);

    assert_access_and_resolution_errors(&app).await?;
    assert_visible_profile(&app).await?;
    assert_privacy_filtering(&app).await?;
    assert_records_search(&app).await?;
    assert_no_active_season(&app, &pool).await?;
    assert_preference_invariant(&app, &pool).await?;

    pool.close().await;
    drop(server);
    Ok(())
}

async fn assert_access_and_resolution_errors(app: &Router) -> TestResult {
    let path = profile_path("Longcat");
    let unauthenticated = get(app, &path, None).await?;
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);

    let outsider_cookie = session_cookie(OUTSIDER_SESSION_TOKEN);
    let outsider = get(app, &path, Some(&outsider_cookie)).await?;
    assert_eq!(outsider.status(), StatusCode::NOT_FOUND);
    assert_eq!(body_json(outsider).await?["error"]["code"], "not_found");

    let viewer_cookie = session_cookie(VIEWER_SESSION_TOKEN);
    for query in ["Unknown", "Inactive"] {
        let response = get(app, &profile_path(query), Some(&viewer_cookie)).await?;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    let duplicate = get(app, &profile_path("Twin"), Some(&viewer_cookie)).await?;
    assert_eq!(duplicate.status(), StatusCode::CONFLICT);
    assert_eq!(
        body_json(duplicate).await?["error"]["code"],
        "ambiguous_guild_member"
    );

    let empty = get(
        app,
        &format!("/api/guilds/{GUILD_A}/records/profile?q=%20%20"),
        Some(&viewer_cookie),
    )
    .await?;
    assert_eq!(empty.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        body_json(empty).await?["error"]["code"],
        "invalid_search_query"
    );
    Ok(())
}

async fn assert_visible_profile(app: &Router) -> TestResult {
    let viewer_cookie = session_cookie(VIEWER_SESSION_TOKEN);
    for query in ["Longcat", VISIBLE_ID, "Longcat%23KR1"] {
        let response = get(app, &profile_path(query), Some(&viewer_cookie)).await?;
        assert_eq!(response.status(), StatusCode::OK, "query {query}");
        let body = body_json(response).await?;
        assert_eq!(
            body["data"]["member"],
            json!({
                "displayName": "Longcat",
                "avatarUrl": null,
                "mainLane": "MID"
            })
        );
        assert_eq!(
            body["data"]["championPreferences"],
            json!({
                "TOP": ["Gnar"],
                "JUNGLE": [],
                "MID": ["Ahri", "Orianna"],
                "BOTTOM": [],
                "SUPPORT": []
            })
        );
        assert_eq!(
            body["data"]["riotAccounts"],
            json!([
                {
                    "platform": "KR",
                    "gameName": "Longcat",
                    "tagLine": "KR1",
                    "profileIconId": 7,
                    "isMain": true
                },
                {
                    "platform": "KR",
                    "gameName": "LongAlt",
                    "tagLine": "KR2",
                    "profileIconId": 8,
                    "isMain": false
                }
            ])
        );
        assert_private_fields_absent(&body);
    }
    Ok(())
}

async fn assert_privacy_filtering(app: &Router) -> TestResult {
    let viewer_cookie = session_cookie(VIEWER_SESSION_TOKEN);
    for query in ["Hidden", "MissingPrivacy"] {
        let response = get(app, &profile_path(query), Some(&viewer_cookie)).await?;
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_json(response).await?;
        assert_eq!(body["data"]["riotAccounts"], json!([]));
        assert_private_fields_absent(&body);
    }

    for hidden_riot_id in ["SecretLink%23KR3", "MissingLink%23KR4"] {
        let response = get(app, &profile_path(hidden_riot_id), Some(&viewer_cookie)).await?;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    Ok(())
}

async fn assert_records_search(app: &Router) -> TestResult {
    let viewer_cookie = session_cookie(VIEWER_SESSION_TOKEN);
    let first = get(
        app,
        &format!("/api/guilds/{GUILD_A}/records/search?q=Longcat&limit=1"),
        Some(&viewer_cookie),
    )
    .await?;
    assert_eq!(first.status(), StatusCode::OK);
    let first = body_json(first).await?;
    assert_eq!(first["data"]["member"]["displayName"], "Longcat");
    assert_eq!(first["data"]["selectedSeasonId"], SEASON_ID);
    assert_eq!(
        first["data"]["summary"],
        json!({ "games": 6, "wins": 5, "losses": 1 })
    );
    assert_eq!(first["data"]["laneMmr"].as_array().map(Vec::len), Some(5));
    assert_eq!(
        first["data"]["laneMmr"][2],
        json!({ "lane": "MID", "mmr": 1515.0, "games": 2, "wins": 1 })
    );
    assert_eq!(
        first["data"]["championRecords"],
        json!([
            { "championId": "Syndra", "lane": "MID", "games": 2, "wins": 2, "losses": 0 },
            { "championId": "Ahri", "lane": "MID", "games": 1, "wins": 1, "losses": 0 },
            { "championId": "LeBlanc", "lane": "MID", "games": 1, "wins": 1, "losses": 0 },
            { "championId": "Viktor", "lane": "MID", "games": 1, "wins": 1, "losses": 0 },
            { "championId": "Orianna", "lane": "MID", "games": 1, "wins": 0, "losses": 1 }
        ])
    );
    assert_eq!(first["data"]["records"].as_array().map(Vec::len), Some(1));
    let newest = &first["data"]["records"][0];
    assert_eq!(newest["kind"], "SERIES");
    assert_eq!(newest["source"], "REGULAR");
    assert_eq!(newest["result"], "WIN");
    assert_eq!(newest["wins"], 1);
    assert_eq!(newest["losses"], 0);
    assert_eq!(newest["mmrDelta"], 15.0);
    assert_eq!(
        newest["games"][0]["blueTeam"].as_array().map(Vec::len),
        Some(5)
    );
    assert_eq!(
        newest["games"][0]["redTeam"].as_array().map(Vec::len),
        Some(5)
    );
    assert_eq!(newest["games"][0]["target"]["side"], "BLUE");
    assert_eq!(newest["games"][0]["target"]["mmrBefore"], 1500.0);
    let cursor = first["data"]["nextCursor"]
        .as_str()
        .ok_or("first records page did not return a cursor")?;

    let second = get(
        app,
        &format!("/api/guilds/{GUILD_A}/records/search?q=Longcat&limit=1&cursor={cursor}"),
        Some(&viewer_cookie),
    )
    .await?;
    assert_eq!(second.status(), StatusCode::OK);
    let second = body_json(second).await?;
    let older = &second["data"]["records"][0];
    assert_eq!(older["result"], "LOSS");
    assert_eq!(older["mmrDelta"], Value::Null);
    assert_eq!(older["games"][0]["target"]["mmrBefore"], Value::Null);
    assert!(second["data"]["nextCursor"].is_string());

    let all = get(
        app,
        &format!("/api/guilds/{GUILD_A}/records/search?q=Longcat&limit=50"),
        Some(&viewer_cookie),
    )
    .await?;
    assert_eq!(all.status(), StatusCode::OK);
    let all = body_json(all).await?;
    let records = all["data"]["records"]
        .as_array()
        .ok_or("records response was not an array")?;
    assert_eq!(records.len(), 4);
    let auction_series = records
        .iter()
        .find(|record| record["kind"] == "SERIES" && record["source"] == "AUCTION")
        .ok_or("10-person auction series was missing")?;
    assert_eq!(auction_series["participantCount"], 10);
    assert_eq!(auction_series["stage"], "FINAL");
    assert_eq!(auction_series["teamName"], "10인 롱캣팀");
    assert_eq!(auction_series["opponentName"], "10인 상대팀");
    assert_eq!(auction_series["mmrDelta"], Value::Null);
    let tournament = records
        .iter()
        .find(|record| record["kind"] == "TOURNAMENT")
        .ok_or("20-person auction tournament was missing")?;
    assert_eq!(tournament["participantCount"], 20);
    assert_eq!(tournament["teamName"], "20인 롱캣팀");
    assert_eq!(tournament["spentPoints"], 300);
    assert_eq!(tournament["placement"], 1);
    assert_eq!(tournament["roster"].as_array().map(Vec::len), Some(5));
    assert_eq!(tournament["series"].as_array().map(Vec::len), Some(2));
    assert_eq!(tournament["series"][0]["stage"], "FINAL");
    assert_eq!(tournament["series"][0]["opponentName"], "20인 상대팀");
    assert_eq!(
        tournament["series"][0]["games"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        tournament["series"][0]["games"][0]["blueTeam"]
            .as_array()
            .map(Vec::len),
        Some(5)
    );
    let semifinal = tournament["series"]
        .as_array()
        .and_then(|series| {
            series
                .iter()
                .find(|series| series["stage"] == "SEMIFINAL_A")
        })
        .ok_or("20-person semifinal was missing")?;
    assert_eq!(semifinal["games"][0]["winningSide"], "RED");
    assert_eq!(semifinal["games"][0]["target"]["side"], "RED");
    assert_eq!(
        records
            .iter()
            .filter(|record| record["kind"] == "SERIES" && record["source"] == "AUCTION")
            .count(),
        1,
        "10-person semifinal must not become a top-level record"
    );

    let outsider = get(
        app,
        &format!("/api/guilds/{GUILD_A}/records/search?q=Longcat"),
        Some(&session_cookie(OUTSIDER_SESSION_TOKEN)),
    )
    .await?;
    assert_eq!(outsider.status(), StatusCode::NOT_FOUND);

    for path in [
        format!("/api/guilds/{GUILD_A}/records/search?q=%20%20"),
        format!("/api/guilds/{GUILD_A}/records/search?q=Longcat&limit=0"),
        format!("/api/guilds/{GUILD_A}/records/search?q=Longcat&cursor=invalid!"),
        format!("/api/guilds/{GUILD_A}/records/search?q=Longcat&seasonId=invalid"),
    ] {
        let response = get(app, &path, Some(&viewer_cookie)).await?;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "path {path}");
    }
    let missing_season = get(
        app,
        &format!(
            "/api/guilds/{GUILD_A}/records/search?q=Longcat&seasonId=01900000-0000-7000-8000-000000000099"
        ),
        Some(&viewer_cookie),
    )
    .await?;
    assert_eq!(missing_season.status(), StatusCode::NOT_FOUND);
    assert_private_fields_absent(&first);
    Ok(())
}

async fn assert_no_active_season(app: &Router, pool: &PgPool) -> TestResult {
    sqlx::query(
        "UPDATE seasons
         SET status = 'CLOSED', ended_at = clock_timestamp()
         WHERE guild_id = $1 AND id = $2::uuid",
    )
    .bind(GUILD_A)
    .bind(SEASON_ID)
    .execute(pool)
    .await?;
    let viewer_cookie = session_cookie(VIEWER_SESSION_TOKEN);
    let response = get(
        app,
        &format!("/api/guilds/{GUILD_A}/records/search?q=Longcat"),
        Some(&viewer_cookie),
    )
    .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await?;
    assert_eq!(body["data"]["selectedSeasonId"], Value::Null);
    assert_eq!(
        body["data"]["summary"],
        json!({ "games": 0, "wins": 0, "losses": 0 })
    );
    assert_eq!(body["data"]["laneMmr"].as_array().map(Vec::len), Some(5));
    assert!(body["data"]["laneMmr"].as_array().is_some_and(|lanes| {
        lanes
            .iter()
            .all(|lane| lane["mmr"] == 1500.0 && lane["games"] == 0 && lane["wins"] == 0)
    }));
    assert_eq!(body["data"]["championRecords"], json!([]));
    assert_eq!(body["data"]["records"], json!([]));

    let closed = get(
        app,
        &format!("/api/guilds/{GUILD_A}/records/search?q=Longcat&seasonId={SEASON_ID}&limit=50"),
        Some(&viewer_cookie),
    )
    .await?;
    assert_eq!(closed.status(), StatusCode::OK);
    let closed = body_json(closed).await?;
    assert_eq!(closed["data"]["selectedSeasonId"], SEASON_ID);
    assert_eq!(closed["data"]["records"].as_array().map(Vec::len), Some(4));
    Ok(())
}

async fn assert_preference_invariant(app: &Router, pool: &PgPool) -> TestResult {
    sqlx::query(
        "UPDATE user_champion_preferences
         SET position = 3
         WHERE user_id = $1 AND lane = 'MID' AND position = 2",
    )
    .bind(VISIBLE_ID)
    .execute(pool)
    .await?;
    let response = get(
        app,
        &format!("/api/guilds/{GUILD_A}/records/search?q=Longcat"),
        Some(&session_cookie(VIEWER_SESSION_TOKEN)),
    )
    .await?;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        body_json(response).await?["error"]["code"],
        "internal_error"
    );
    Ok(())
}

fn assert_private_fields_absent(body: &Value) {
    let serialized = body.to_string();
    for private in [
        "puuid-visible-main",
        "puuid-visible-secondary",
        "puuid-hidden",
        "puuid-missing-privacy",
        "routingRegion",
        "normalizedGameName",
        "normalizedTagLine",
        "userId",
        "discordId",
    ] {
        assert!(
            !serialized.contains(private),
            "leaked private value or field {private}"
        );
    }
}

async fn seed_login(
    repository: &AuthRepository,
    user_id: &str,
    display_name: &str,
    session_token: &str,
) -> TestResult {
    repository
        .complete_login(&NewLogin {
            discord_user_id: DiscordUserId::parse(user_id)?,
            display_name: display_name.to_owned(),
            avatar_hash: None,
            previous_session_token_hash: None,
            session_id: Uuid::now_v7(),
            token_hash: AuthService::hash_secret(session_token),
            csrf_secret_hash: AuthService::hash_secret("unused-csrf-token"),
            expires_at: OffsetDateTime::now_utc() + time::Duration::hours(1),
        })
        .await?;
    Ok(())
}

async fn seed_profile_data(pool: &PgPool) -> TestResult {
    for (user_id, display_name) in [
        (VISIBLE_ID, "Visible User"),
        (HIDDEN_ID, "Hidden User"),
        (MISSING_PRIVACY_ID, "Missing Privacy User"),
        (INACTIVE_ID, "Inactive User"),
        (TWIN_A_ID, "Twin A"),
        (TWIN_B_ID, "Twin B"),
        (FILLER_1_ID, "Filler 1"),
        (FILLER_2_ID, "Filler 2"),
        (FILLER_3_ID, "Filler 3"),
        (FILLER_4_ID, "Filler 4"),
    ] {
        sqlx::query(
            "INSERT INTO users (
                 user_id, display_name, avatar_hash, created_at, updated_at, deleted_at
             ) VALUES ($1, $2, NULL, clock_timestamp(), clock_timestamp(), NULL)",
        )
        .bind(user_id)
        .bind(display_name)
        .execute(pool)
        .await?;
    }

    for (guild_id, name, installed_by) in [
        (GUILD_A, "Guild A", VIEWER_ID),
        (GUILD_B, "Guild B", OUTSIDER_ID),
    ] {
        sqlx::query(
            "INSERT INTO guilds (
                 guild_id, discord_name, icon_hash, status, installed_by,
                 installed_at, activated_at, left_at, updated_at
             ) VALUES (
                 $1, $2, NULL, 'ACTIVE', $3,
                 clock_timestamp(), clock_timestamp(), NULL, clock_timestamp()
             )",
        )
        .bind(guild_id)
        .bind(name)
        .bind(installed_by)
        .execute(pool)
        .await?;
    }

    for (guild_id, user_id, display_name, status, main_lane) in [
        (GUILD_A, VIEWER_ID, "Viewer", "ACTIVE", None),
        (GUILD_A, VISIBLE_ID, "Longcat", "ACTIVE", Some("MID")),
        (GUILD_A, HIDDEN_ID, "Hidden", "ACTIVE", Some("TOP")),
        (
            GUILD_A,
            MISSING_PRIVACY_ID,
            "MissingPrivacy",
            "ACTIVE",
            None,
        ),
        (GUILD_A, INACTIVE_ID, "Inactive", "INACTIVE", None),
        (GUILD_A, TWIN_A_ID, "Twin", "ACTIVE", None),
        (GUILD_A, TWIN_B_ID, "Twin", "ACTIVE", None),
        (GUILD_A, FILLER_1_ID, "Filler 1", "ACTIVE", Some("TOP")),
        (GUILD_A, FILLER_2_ID, "Filler 2", "ACTIVE", Some("JUNGLE")),
        (GUILD_A, FILLER_3_ID, "Filler 3", "ACTIVE", Some("BOTTOM")),
        (GUILD_A, FILLER_4_ID, "Filler 4", "ACTIVE", Some("SUPPORT")),
        (GUILD_B, OUTSIDER_ID, "Outsider", "ACTIVE", None),
    ] {
        sqlx::query(
            "INSERT INTO guild_members (
                 guild_id, user_id, display_name, status, main_lane,
                 first_seen_at, registered_at, discord_joined_at,
                 last_active_at, last_synced_at, inactivated_at
             ) VALUES (
                 $1, $2, $3, $4, $5,
                 clock_timestamp(), clock_timestamp(), clock_timestamp(),
                 clock_timestamp(), clock_timestamp(),
                 CASE WHEN $4 = 'INACTIVE' THEN clock_timestamp() ELSE NULL END
             )",
        )
        .bind(guild_id)
        .bind(user_id)
        .bind(display_name)
        .bind(status)
        .bind(main_lane)
        .execute(pool)
        .await?;
    }

    for (user_id, show_riot_ids) in [(VISIBLE_ID, true), (HIDDEN_ID, false)] {
        sqlx::query(
            "INSERT INTO user_privacy_settings (
                 user_id, public_profile_enabled, show_riot_ids,
                 allow_external_record_search, updated_at
             ) VALUES ($1, true, $2, false, clock_timestamp())",
        )
        .bind(user_id)
        .bind(show_riot_ids)
        .execute(pool)
        .await?;
    }

    for (lane, position, champion_id) in [
        ("TOP", 1_i16, "Gnar"),
        ("MID", 1_i16, "Ahri"),
        ("MID", 2_i16, "Orianna"),
    ] {
        sqlx::query(
            "INSERT INTO user_champion_preferences (user_id, lane, position, champion_id)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(VISIBLE_ID)
        .bind(lane)
        .bind(position)
        .bind(champion_id)
        .execute(pool)
        .await?;
    }

    for (
        puuid,
        user_id,
        game_name,
        tag_line,
        normalized_game_name,
        normalized_tag_line,
        profile_icon_id,
        is_main,
        created_offset,
    ) in [
        (
            "puuid-visible-main",
            VISIBLE_ID,
            "Longcat",
            "KR1",
            "longcat",
            "kr1",
            7,
            true,
            1_i32,
        ),
        (
            "puuid-visible-secondary",
            VISIBLE_ID,
            "LongAlt",
            "KR2",
            "longalt",
            "kr2",
            8,
            false,
            0_i32,
        ),
        (
            "puuid-hidden",
            HIDDEN_ID,
            "SecretLink",
            "KR3",
            "secretlink",
            "kr3",
            9,
            true,
            0_i32,
        ),
        (
            "puuid-missing-privacy",
            MISSING_PRIVACY_ID,
            "MissingLink",
            "KR4",
            "missinglink",
            "kr4",
            10,
            true,
            0_i32,
        ),
    ] {
        sqlx::query(
            "INSERT INTO riot_accounts (
                 puuid, user_id, platform, routing_region, game_name, tag_line,
                 normalized_game_name, normalized_tag_line, profile_icon_id,
                 is_main, main_position, main_position_updated_at,
                 last_synced_at, created_at, updated_at
             ) VALUES (
                 $1, $2, 'KR', 'ASIA', $3, $4,
                 $5, $6, $7,
                 $8, NULL, NULL,
                 clock_timestamp(),
                 clock_timestamp() - make_interval(days => $9),
                 clock_timestamp()
             )",
        )
        .bind(puuid)
        .bind(user_id)
        .bind(game_name)
        .bind(tag_line)
        .bind(normalized_game_name)
        .bind(normalized_tag_line)
        .bind(profile_icon_id)
        .bind(is_main)
        .bind(created_offset)
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn seed_record_data(pool: &PgPool) -> TestResult {
    sqlx::query(
        "INSERT INTO seasons (
             guild_id, id, name, status, started_at, ended_at, created_by, created_at
         ) VALUES ($1, $2::uuid, '시즌 1', 'ACTIVE',
                   clock_timestamp() - interval '30 days', NULL, $3, clock_timestamp())",
    )
    .bind(GUILD_A)
    .bind(SEASON_ID)
    .bind(VIEWER_ID)
    .execute(pool)
    .await?;

    for (display_number, recruitment_id, title) in [
        (1_i64, RECRUITMENT_1_ID, "첫 번째 내전"),
        (2_i64, RECRUITMENT_2_ID, "두 번째 내전"),
    ] {
        sqlx::query(
            "INSERT INTO recruitments (
                 guild_id, id, season_id, display_number, title, target_count,
                 status, revision, channel_id, message_id, created_by, created_at,
                 updated_by, updated_at, closed_at, converted_at, cancelled_at
             ) VALUES (
                 $1, $2::uuid, $3::uuid, $4, $5, 10,
                 'CONVERTED', 1, NULL, NULL, $6, clock_timestamp() - interval '10 days',
                 $6, clock_timestamp(), clock_timestamp(), clock_timestamp(), NULL
             )",
        )
        .bind(GUILD_A)
        .bind(recruitment_id)
        .bind(SEASON_ID)
        .bind(display_number)
        .bind(title)
        .bind(VIEWER_ID)
        .execute(pool)
        .await?;
    }

    for (display_number, series_id, recruitment_id, title, winning_team, started_days_ago) in [
        (
            1_i64,
            SERIES_1_ID,
            RECRUITMENT_1_ID,
            "첫 번째 내전",
            "TEAM_2",
            3_i32,
        ),
        (
            2_i64,
            SERIES_2_ID,
            RECRUITMENT_2_ID,
            "두 번째 내전",
            "TEAM_1",
            1_i32,
        ),
    ] {
        sqlx::query(
            "INSERT INTO series (
                 guild_id, id, season_id, source_recruitment_id, display_number,
                 title, best_of, status, winning_team, terminal_reason, revision,
                 started_at, ended_at, created_by, end_card_channel_id,
                 end_card_message_id, updated_by, updated_at, deleted_at
             ) VALUES (
                 $1, $2::uuid, $3::uuid, $4::uuid, $5,
                 $6, 1, 'COMPLETED', $7, 'SCORE_REACHED', 1,
                 clock_timestamp() - make_interval(days => $8),
                 clock_timestamp() - make_interval(days => $8) + interval '1 hour',
                 $9, NULL, NULL, $9, clock_timestamp(), NULL
             )",
        )
        .bind(GUILD_A)
        .bind(series_id)
        .bind(SEASON_ID)
        .bind(recruitment_id)
        .bind(display_number)
        .bind(title)
        .bind(winning_team)
        .bind(started_days_ago)
        .bind(VIEWER_ID)
        .execute(pool)
        .await?;

        sqlx::query(
            "INSERT INTO series_participants (
                 guild_id, series_id, user_id, team, lane, mmr_at_lock, mmr_snapshot_origin
             ) VALUES ($1, $2::uuid, $3, 'TEAM_1', 'MID', 1500, 'CAPTURED')",
        )
        .bind(GUILD_A)
        .bind(series_id)
        .bind(VISIBLE_ID)
        .execute(pool)
        .await?;
    }

    for (input_id, series_id, winning_side, days_ago) in [
        (INPUT_1_ID, SERIES_1_ID, "RED", 3_i32),
        (INPUT_2_ID, SERIES_2_ID, "BLUE", 1_i32),
    ] {
        sqlx::query(
            "INSERT INTO game_inputs (
                 guild_id, id, series_id, auction_match_id, set_number,
                 side_preset, winning_side, patch_version, status, updated_by, updated_at
             ) VALUES ($1, $2::uuid, $3::uuid, NULL, 1,
                       'DEFAULT', $4, '26.13', 'SUBMITTED', $5,
                       clock_timestamp() - make_interval(days => $6))",
        )
        .bind(GUILD_A)
        .bind(input_id)
        .bind(series_id)
        .bind(winning_side)
        .bind(VIEWER_ID)
        .bind(days_ago)
        .execute(pool)
        .await?;
    }

    for (game_id, series_id, input_id, winning_side, mmr_status, days_ago) in [
        (
            GAME_1_ID,
            SERIES_1_ID,
            INPUT_1_ID,
            "RED",
            "LEGACY_NOT_APPLIED",
            3_i32,
        ),
        (GAME_2_ID, SERIES_2_ID, INPUT_2_ID, "BLUE", "APPLIED", 1_i32),
    ] {
        sqlx::query(
            "INSERT INTO games (
                 guild_id, id, season_id, series_id, auction_match_id, source_input_id,
                 game_number, side_preset, winning_side, duration_seconds, patch_version,
                 riot_match_id, status, mmr_status, played_at, confirmed_by,
                 confirmed_by_type, confirmed_at, voided_by, voided_by_type,
                 voided_at, void_reason
             ) VALUES (
                 $1, $2::uuid, $3::uuid, $4::uuid, NULL, $5::uuid,
                 1, 'DEFAULT', $6, 1800, '26.13',
                 NULL, 'CONFIRMED', $7,
                 clock_timestamp() - make_interval(days => $8),
                 $9, 'USER', clock_timestamp(), NULL, NULL, NULL, NULL
             )",
        )
        .bind(GUILD_A)
        .bind(game_id)
        .bind(SEASON_ID)
        .bind(series_id)
        .bind(input_id)
        .bind(winning_side)
        .bind(mmr_status)
        .bind(days_ago)
        .bind(VIEWER_ID)
        .execute(pool)
        .await?;
    }

    let participants = [
        (VISIBLE_ID, "TEAM_1", "MID", "Ahri"),
        (VIEWER_ID, "TEAM_1", "TOP", "Ornn"),
        (HIDDEN_ID, "TEAM_1", "JUNGLE", "LeeSin"),
        (MISSING_PRIVACY_ID, "TEAM_1", "BOTTOM", "Ezreal"),
        (TWIN_A_ID, "TEAM_1", "SUPPORT", "Nautilus"),
        (TWIN_B_ID, "TEAM_2", "MID", "Orianna"),
        (FILLER_1_ID, "TEAM_2", "TOP", "Gnar"),
        (FILLER_2_ID, "TEAM_2", "JUNGLE", "Viego"),
        (FILLER_3_ID, "TEAM_2", "BOTTOM", "Kaisa"),
        (FILLER_4_ID, "TEAM_2", "SUPPORT", "Rakan"),
    ];
    for (game_id, target_champion) in [(GAME_1_ID, "Orianna"), (GAME_2_ID, "Ahri")] {
        for (user_id, team, lane, default_champion) in participants {
            let champion = if user_id == VISIBLE_ID {
                target_champion
            } else {
                default_champion
            };
            sqlx::query(
                "INSERT INTO game_participants (
                     guild_id, game_id, user_id, team, lane, champion_id
                 ) VALUES ($1, $2::uuid, $3, $4, $5, $6)",
            )
            .bind(GUILD_A)
            .bind(game_id)
            .bind(user_id)
            .bind(team)
            .bind(lane)
            .bind(champion)
            .execute(pool)
            .await?;
        }
        for (side, slot, champion) in [
            ("BLUE", 1_i16, "Jax"),
            ("BLUE", 2_i16, "Vi"),
            ("RED", 1_i16, "Azir"),
            ("RED", 2_i16, "Jinx"),
        ] {
            sqlx::query(
                "INSERT INTO game_bans (guild_id, game_id, side, slot, champion_id)
                 VALUES ($1, $2::uuid, $3, $4, $5)",
            )
            .bind(GUILD_A)
            .bind(game_id)
            .bind(side)
            .bind(slot)
            .bind(champion)
            .execute(pool)
            .await?;
        }
    }

    sqlx::query(
        "INSERT INTO user_lane_mmr (
             guild_id, season_id, user_id, lane, mmr, games_played, wins, revision, updated_at
         ) VALUES ($1, $2::uuid, $3, 'MID', 1515, 2, 1, 1, clock_timestamp())",
    )
    .bind(GUILD_A)
    .bind(SEASON_ID)
    .bind(VISIBLE_ID)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO mmr_ledger_entries (
             guild_id, id, season_id, user_id, lane, lane_revision,
             source_type, source_id, game_id, opponent_user_id,
             mmr_before, mmr_after, delta, games_played_delta, wins_delta,
             reverses_entry_id, reversed_source_type, actor_type, actor_id,
             note, request_id, created_at
         ) VALUES (
             $1, '01900000-0000-7000-8000-000000000019', $2::uuid, $3, 'MID', 1,
             'GAME_RESULT', $4::uuid, $4::uuid, $5,
             1500, 1515, 15, 1, 1,
             NULL, NULL, 'USER', $6, NULL, 'record-test', clock_timestamp()
         )",
    )
    .bind(GUILD_A)
    .bind(SEASON_ID)
    .bind(VISIBLE_ID)
    .bind(GAME_2_ID)
    .bind(TWIN_B_ID)
    .bind(VIEWER_ID)
    .execute(pool)
    .await?;
    seed_cancelled_record(pool).await?;
    Ok(())
}

async fn seed_cancelled_record(pool: &PgPool) -> TestResult {
    let recruitment_id = Uuid::now_v7();
    let series_id = Uuid::now_v7();
    let input_id = Uuid::now_v7();
    let game_id = Uuid::now_v7();
    let season_id = Uuid::parse_str(SEASON_ID)?;
    sqlx::query(
        "INSERT INTO recruitments (
             guild_id, id, season_id, display_number, title, target_count,
             status, revision, created_by, created_at, updated_by, updated_at,
             closed_at, converted_at
         ) VALUES ($1, $2, $3, 99, '취소된 내전', 10,
                   'CONVERTED', 1, $4, clock_timestamp(), $4, clock_timestamp(),
                   clock_timestamp(), clock_timestamp())",
    )
    .bind(GUILD_A)
    .bind(recruitment_id)
    .bind(season_id)
    .bind(VIEWER_ID)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO series (
             guild_id, id, season_id, source_recruitment_id, display_number,
             title, best_of, status, winning_team, terminal_reason, revision,
             started_at, ended_at, created_by, updated_by, updated_at, deleted_at
         ) VALUES ($1, $2, $3, $4, 99,
                   '취소된 내전', 1, 'CANCELLED', NULL, 'CANCELLED', 1,
                   clock_timestamp(), clock_timestamp(), $5, $5, clock_timestamp(), NULL)",
    )
    .bind(GUILD_A)
    .bind(series_id)
    .bind(season_id)
    .bind(recruitment_id)
    .bind(VIEWER_ID)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO game_inputs (
             guild_id, id, series_id, set_number, side_preset, winning_side,
             status, updated_by, updated_at
         ) VALUES ($1, $2, $3, 1, 'DEFAULT', 'BLUE',
                   'SUBMITTED', $4, clock_timestamp())",
    )
    .bind(GUILD_A)
    .bind(input_id)
    .bind(series_id)
    .bind(VIEWER_ID)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO games (
             guild_id, id, season_id, series_id, source_input_id, game_number,
             side_preset, winning_side, status, mmr_status, played_at,
             confirmed_by, confirmed_by_type, confirmed_at
         ) VALUES ($1, $2, $3, $4, $5, 1,
                   'DEFAULT', 'BLUE', 'CONFIRMED', 'LEGACY_NOT_APPLIED',
                   clock_timestamp(), $6, 'USER', clock_timestamp())",
    )
    .bind(GUILD_A)
    .bind(game_id)
    .bind(season_id)
    .bind(series_id)
    .bind(input_id)
    .bind(VIEWER_ID)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO game_participants (
             guild_id, game_id, user_id, team, lane, champion_id
         ) VALUES ($1, $2, $3, 'TEAM_1', 'MID', 'Teemo')",
    )
    .bind(GUILD_A)
    .bind(game_id)
    .bind(VISIBLE_ID)
    .execute(pool)
    .await?;
    Ok(())
}

async fn seed_auction_record_data(pool: &PgPool) -> TestResult {
    let recruitment_10 = Uuid::now_v7();
    let tournament_10 = Uuid::now_v7();
    let target_team_10 = Uuid::now_v7();
    let opponent_team_10 = Uuid::now_v7();
    let match_10 = Uuid::now_v7();
    let input_10 = Uuid::now_v7();
    let game_10 = Uuid::now_v7();
    let semifinal_10 = Uuid::now_v7();
    let semifinal_input_10 = Uuid::now_v7();
    let semifinal_game_10 = Uuid::now_v7();
    let recruitment_20 = Uuid::now_v7();
    let tournament_20 = Uuid::now_v7();
    let target_team_20 = Uuid::now_v7();
    let opponent_team_20 = Uuid::now_v7();
    let match_20 = Uuid::now_v7();
    let input_20 = Uuid::now_v7();
    let game_20 = Uuid::now_v7();
    let semifinal_20 = Uuid::now_v7();
    let semifinal_input_20 = Uuid::now_v7();
    let semifinal_game_20 = Uuid::now_v7();

    for (recruitment_id, display_number, title, format, target_count, semifinal_best_of) in [
        (
            recruitment_10,
            10_i64,
            "10인 경매 모집",
            "TEN_PERSON",
            10_i16,
            None,
        ),
        (
            recruitment_20,
            20_i64,
            "20인 경매 모집",
            "TWENTY_PERSON",
            20_i16,
            Some(3_i16),
        ),
    ] {
        sqlx::query(
            "INSERT INTO auction_recruitments (
                 guild_id, id, season_id, display_number, title, format, target_count,
                 semifinal_best_of, status, revision, channel_id, message_id, created_by, created_at,
                 updated_by, updated_at, closed_at, converted_at, cancelled_at
             ) VALUES (
                 $1, $2, $3, $4, $5, $6, $7,
                 $8, 'CONVERTED', 1, NULL, NULL, $9, clock_timestamp() - interval '10 days',
                 $9, clock_timestamp(), clock_timestamp(), clock_timestamp(), NULL
             )",
        )
        .bind(GUILD_A)
        .bind(recruitment_id)
        .bind(Uuid::parse_str(SEASON_ID)?)
        .bind(display_number)
        .bind(title)
        .bind(format)
        .bind(target_count)
        .bind(semifinal_best_of)
        .bind(VIEWER_ID)
        .execute(pool)
        .await?;
    }

    for (
        tournament_id,
        recruitment_id,
        display_number,
        title,
        format,
        semifinal_best_of,
        started_days_ago,
    ) in [
        (
            tournament_10,
            recruitment_10,
            10_i64,
            "10인 완료 경매",
            "TEN_PERSON",
            None,
            5_i32,
        ),
        (
            tournament_20,
            recruitment_20,
            20_i64,
            "20인 완료 경매",
            "TWENTY_PERSON",
            Some(3_i16),
            6_i32,
        ),
    ] {
        sqlx::query(
            "INSERT INTO auction_tournaments (
                 guild_id, id, season_id, source_recruitment_id, display_number,
                 title, format, semifinal_best_of, status, revision, bracket_locked_at, bracket_locked_by,
                 champion_team_id, started_at, ended_at, created_by, updated_by,
                 updated_at, end_card_channel_id, end_card_message_id, deleted_at
             ) VALUES (
                 $1, $2, $3, $4, $5,
                 $6, $7, $8, 'IN_GAME', 1, clock_timestamp(), $9,
                 NULL, clock_timestamp() - make_interval(days => $10), NULL,
                 $9, $9, clock_timestamp(), NULL, NULL, NULL
             )",
        )
        .bind(GUILD_A)
        .bind(tournament_id)
        .bind(Uuid::parse_str(SEASON_ID)?)
        .bind(recruitment_id)
        .bind(display_number)
        .bind(title)
        .bind(format)
        .bind(semifinal_best_of)
        .bind(VIEWER_ID)
        .bind(started_days_ago)
        .execute(pool)
        .await?;
    }

    for (tournament_id, team_id, team_index, team_name, current_points) in [
        (tournament_10, target_team_10, 1_i16, "10인 롱캣팀", 800_i32),
        (
            tournament_10,
            opponent_team_10,
            2_i16,
            "10인 상대팀",
            850_i32,
        ),
        (tournament_20, target_team_20, 1_i16, "20인 롱캣팀", 700_i32),
        (
            tournament_20,
            opponent_team_20,
            2_i16,
            "20인 상대팀",
            750_i32,
        ),
    ] {
        sqlx::query(
            "INSERT INTO auction_teams (
                 guild_id, tournament_id, id, team_index, team_name,
                 initial_points, current_points, point_revision, created_at
             ) VALUES ($1, $2, $3, $4, $5, 1000, $6, 1, clock_timestamp())",
        )
        .bind(GUILD_A)
        .bind(tournament_id)
        .bind(team_id)
        .bind(team_index)
        .bind(team_name)
        .bind(current_points)
        .execute(pool)
        .await?;
    }

    seed_target_auction_roster(pool, tournament_20, target_team_20).await?;

    for (
        tournament_id,
        match_id,
        left_team_id,
        right_team_id,
        winner_team_id,
        match_key,
        days_ago,
    ) in [
        (
            tournament_10,
            match_10,
            target_team_10,
            opponent_team_10,
            target_team_10,
            "FINAL",
            5_i32,
        ),
        (
            tournament_10,
            semifinal_10,
            target_team_10,
            opponent_team_10,
            target_team_10,
            "SEMIFINAL_A",
            7_i32,
        ),
        (
            tournament_20,
            match_20,
            target_team_20,
            opponent_team_20,
            target_team_20,
            "FINAL",
            6_i32,
        ),
        (
            tournament_20,
            semifinal_20,
            target_team_20,
            opponent_team_20,
            target_team_20,
            "SEMIFINAL_A",
            8_i32,
        ),
    ] {
        sqlx::query(
            "INSERT INTO auction_matches (
                 guild_id, tournament_id, id, season_id, match_key, best_of,
                 left_team_id, right_team_id, status, winner_team_id, revision,
                 lineups_locked_at, lineups_locked_by, started_at, ended_at,
                 updated_by, updated_at, deleted_at
             ) VALUES (
                 $1, $2, $3, $4, $10, 1,
                 $5, $6, 'COMPLETED', $7, 1,
                 clock_timestamp(), $8,
                 clock_timestamp() - make_interval(days => $9),
                 clock_timestamp() - make_interval(days => $9) + interval '1 hour',
                 $8, clock_timestamp(), NULL
             )",
        )
        .bind(GUILD_A)
        .bind(tournament_id)
        .bind(match_id)
        .bind(Uuid::parse_str(SEASON_ID)?)
        .bind(left_team_id)
        .bind(right_team_id)
        .bind(winner_team_id)
        .bind(VIEWER_ID)
        .bind(days_ago)
        .bind(match_key)
        .execute(pool)
        .await?;

        sqlx::query(
            "UPDATE auction_tournaments
             SET status = 'COMPLETED', champion_team_id = $3,
                 ended_at = clock_timestamp() - make_interval(days => $4) + interval '1 hour',
                 updated_at = clock_timestamp()
             WHERE guild_id = $1 AND id = $2",
        )
        .bind(GUILD_A)
        .bind(tournament_id)
        .bind(winner_team_id)
        .bind(days_ago)
        .execute(pool)
        .await?;
    }

    for (input_id, match_id, side_preset, winning_side, days_ago) in [
        (input_10, match_10, "DEFAULT", "BLUE", 5_i32),
        (semifinal_input_10, semifinal_10, "DEFAULT", "BLUE", 7_i32),
        (input_20, match_20, "DEFAULT", "BLUE", 6_i32),
        (semifinal_input_20, semifinal_20, "SWAPPED", "RED", 8_i32),
    ] {
        sqlx::query(
            "INSERT INTO game_inputs (
                 guild_id, id, series_id, auction_match_id, set_number,
                 side_preset, winning_side, patch_version, status, updated_by, updated_at
             ) VALUES ($1, $2, NULL, $3, 1,
                       $4, $5, '26.13', 'SUBMITTED', $6,
                       clock_timestamp() - make_interval(days => $7))",
        )
        .bind(GUILD_A)
        .bind(input_id)
        .bind(match_id)
        .bind(side_preset)
        .bind(winning_side)
        .bind(VIEWER_ID)
        .bind(days_ago)
        .execute(pool)
        .await?;
    }
    for (game_id, match_id, input_id, side_preset, winning_side, days_ago, champion) in [
        (
            game_10, match_10, input_10, "DEFAULT", "BLUE", 5_i32, "Syndra",
        ),
        (
            semifinal_game_10,
            semifinal_10,
            semifinal_input_10,
            "DEFAULT",
            "BLUE",
            7_i32,
            "Viktor",
        ),
        (
            game_20, match_20, input_20, "DEFAULT", "BLUE", 6_i32, "Syndra",
        ),
        (
            semifinal_game_20,
            semifinal_20,
            semifinal_input_20,
            "SWAPPED",
            "RED",
            8_i32,
            "LeBlanc",
        ),
    ] {
        sqlx::query(
            "INSERT INTO games (
                 guild_id, id, season_id, series_id, auction_match_id, source_input_id,
                 game_number, side_preset, winning_side, duration_seconds, patch_version,
                 riot_match_id, status, mmr_status, played_at, confirmed_by,
                 confirmed_by_type, confirmed_at, voided_by, voided_by_type,
                 voided_at, void_reason
             ) VALUES (
                 $1, $2, $3, NULL, $4, $5,
                 1, $6, $7, 1800, '26.13', NULL,
                 'CONFIRMED', 'LEGACY_NOT_APPLIED',
                 clock_timestamp() - make_interval(days => $8),
                 $9, 'USER', clock_timestamp(), NULL, NULL, NULL, NULL
             )",
        )
        .bind(GUILD_A)
        .bind(game_id)
        .bind(Uuid::parse_str(SEASON_ID)?)
        .bind(match_id)
        .bind(input_id)
        .bind(side_preset)
        .bind(winning_side)
        .bind(days_ago)
        .bind(VIEWER_ID)
        .execute(pool)
        .await?;
        seed_game_projection(pool, game_id, champion).await?;
    }
    Ok(())
}

async fn seed_target_auction_roster(
    pool: &PgPool,
    tournament_id: Uuid,
    team_id: Uuid,
) -> TestResult {
    sqlx::query(
        "INSERT INTO auction_team_members (
             guild_id, tournament_id, team_id, id, user_id, roster_slot,
             acquired_via, declared_lane, mmr_at_acquisition, snapshot_origin,
             acquisition_action_id, acquisition_action_kind, joined_at, released_at
         ) VALUES ($1, $2, $3, $4, $5, 3,
                   'CAPTAIN', 'MID', 1500, 'CAPTURED', NULL, NULL,
                   clock_timestamp(), NULL)",
    )
    .bind(GUILD_A)
    .bind(tournament_id)
    .bind(team_id)
    .bind(Uuid::now_v7())
    .bind(VISIBLE_ID)
    .execute(pool)
    .await?;

    for (index, user_id, slot, lane, unavailable) in [
        (1_i64, VIEWER_ID, 1_i16, "TOP", false),
        (2_i64, HIDDEN_ID, 2_i16, "JUNGLE", false),
        (3_i64, MISSING_PRIVACY_ID, 4_i16, "BOTTOM", false),
        (4_i64, TWIN_A_ID, 5_i16, "SUPPORT", true),
    ] {
        let lot_id = Uuid::now_v7();
        let action_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO auction_bid_candidates (
                 guild_id, tournament_id, user_id, declared_lane, mmr_at_start,
                 snapshot_origin, queue_position, state, draw_count, updated_at
             ) VALUES ($1, $2, $3, $4, 1500, 'CAPTURED', $5, 'ASSIGNED', 1, clock_timestamp())",
        )
        .bind(GUILD_A)
        .bind(tournament_id)
        .bind(user_id)
        .bind(lane)
        .bind(index)
        .execute(pool)
        .await?;
        sqlx::query(
            "INSERT INTO auction_lots (
                 guild_id, tournament_id, id, draw_sequence, target_user_id,
                 status, opened_by_type, opened_by_id, opened_at,
                 closed_by_type, closed_by_id, closed_at
             ) VALUES ($1, $2, $3, $4, $5,
                       'ASSIGNED', 'USER', $6, clock_timestamp(),
                       'USER', $6, clock_timestamp())",
        )
        .bind(GUILD_A)
        .bind(tournament_id)
        .bind(lot_id)
        .bind(index)
        .bind(user_id)
        .bind(VIEWER_ID)
        .execute(pool)
        .await?;
        sqlx::query(
            "INSERT INTO auction_actions (
                 guild_id, tournament_id, id, sequence, kind, target_user_id,
                 team_id, lot_id, points, reverses_action_id, reversed_action_kind,
                 actor_type, actor_id, created_at
             ) VALUES ($1, $2, $3, $4, 'ASSIGN_CURRENT', $5,
                       $6, $7, NULL, NULL, NULL, 'USER', $8, clock_timestamp())",
        )
        .bind(GUILD_A)
        .bind(tournament_id)
        .bind(action_id)
        .bind(index)
        .bind(user_id)
        .bind(team_id)
        .bind(lot_id)
        .bind(VIEWER_ID)
        .execute(pool)
        .await?;
        sqlx::query(
            "INSERT INTO auction_team_members (
                 guild_id, tournament_id, team_id, id, user_id, roster_slot,
                 acquired_via, declared_lane, mmr_at_acquisition, snapshot_origin,
                 acquisition_action_id, acquisition_action_kind, joined_at, released_at
             ) VALUES (
                 $1, $2, $3, $4, $5, $6,
                 'MANUAL', CASE WHEN $9 THEN NULL ELSE $7 END,
                 CASE WHEN $9 THEN NULL ELSE 1500 END,
                 CASE WHEN $9 THEN 'UNAVAILABLE' ELSE 'CAPTURED' END,
                 $8, 'ASSIGN_CURRENT', clock_timestamp(), NULL
             )",
        )
        .bind(GUILD_A)
        .bind(tournament_id)
        .bind(team_id)
        .bind(Uuid::now_v7())
        .bind(user_id)
        .bind(slot)
        .bind(lane)
        .bind(action_id)
        .bind(unavailable)
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn seed_game_projection(pool: &PgPool, game_id: Uuid, target_champion: &str) -> TestResult {
    for (user_id, team, lane, champion) in [
        (VISIBLE_ID, "TEAM_1", "MID", target_champion),
        (VIEWER_ID, "TEAM_1", "TOP", "Ornn"),
        (HIDDEN_ID, "TEAM_1", "JUNGLE", "LeeSin"),
        (MISSING_PRIVACY_ID, "TEAM_1", "BOTTOM", "Ezreal"),
        (TWIN_A_ID, "TEAM_1", "SUPPORT", "Nautilus"),
        (TWIN_B_ID, "TEAM_2", "MID", "Orianna"),
        (FILLER_1_ID, "TEAM_2", "TOP", "Gnar"),
        (FILLER_2_ID, "TEAM_2", "JUNGLE", "Viego"),
        (FILLER_3_ID, "TEAM_2", "BOTTOM", "Kaisa"),
        (FILLER_4_ID, "TEAM_2", "SUPPORT", "Rakan"),
    ] {
        sqlx::query(
            "INSERT INTO game_participants (
                 guild_id, game_id, user_id, team, lane, champion_id
             ) VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(GUILD_A)
        .bind(game_id)
        .bind(user_id)
        .bind(team)
        .bind(lane)
        .bind(champion)
        .execute(pool)
        .await?;
    }
    for (side, slot, champion) in [
        ("BLUE", 1_i16, "Jax"),
        ("BLUE", 2_i16, "Vi"),
        ("RED", 1_i16, "Azir"),
        ("RED", 2_i16, "Jinx"),
    ] {
        sqlx::query(
            "INSERT INTO game_bans (guild_id, game_id, side, slot, champion_id)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(GUILD_A)
        .bind(game_id)
        .bind(side)
        .bind(slot)
        .bind(champion)
        .execute(pool)
        .await?;
    }
    Ok(())
}

fn profile_path(query: &str) -> String {
    format!("/api/guilds/{GUILD_A}/records/profile?q={query}")
}

fn session_cookie(token: &str) -> String {
    format!("monkeybot_session={token}")
}

async fn get(app: &Router, uri: &str, cookie: Option<&str>) -> TestResult<Response<Body>> {
    let mut request = Request::get(uri);
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    Ok(app.clone().oneshot(request.body(Body::empty())?).await?)
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
struct UnusedDiscordClient;

impl DiscordClient for UnusedDiscordClient {
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
        Box::pin(async { Err(DiscordClientError::InvalidAuthorizationCode) })
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
        let port = container.get_host_port_ipv4(5432).await?;
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
