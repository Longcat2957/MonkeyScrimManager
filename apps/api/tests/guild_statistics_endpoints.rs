use std::{collections::HashMap, error::Error, sync::Arc, time::Duration};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, Response, StatusCode, header},
};
use futures_util::future::BoxFuture;
use monkeybot_api::{
    AppState,
    auth::{AuthService, CookiePolicy},
    build_router,
    config::Config,
    domain::DiscordUserId,
    infrastructure::postgres::{MIGRATOR, PostgresReadinessProbe},
    integrations::discord::{DiscordClient, DiscordClientError, DiscordUser},
    repository::auth::{AuthRepository, NewLogin},
    state::AuthRuntime,
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
const POSTGRES_PASSWORD: &str = "monkeybot-statistics-password";
const DATABASE: &str = "monkeybot_statistics";
const GUILD_ID: &str = "200000000000000001";
const VIEWER_ID: &str = "100000000000000001";
const OUTSIDER_ID: &str = "100000000000000002";
const CAPTAIN_B_ID: &str = "100000000000000003";
const BIDDER_ID: &str = "100000000000000004";
const VIEWER_SESSION: &str = "statistics-viewer-session";
const OUTSIDER_SESSION: &str = "statistics-outsider-session";
const ACTIVE_SEASON_ID: &str = "01900000-0000-7000-8000-000000000010";
const CLOSED_SEASON_ID: &str = "01900000-0000-7000-8000-000000000011";
const FOREIGN_SEASON_ID: &str = "01900000-0000-7000-8000-000000000012";
const REGULAR_SERIES_ID: &str = "01900000-0000-7000-8000-000000000102";
const CANCELLED_SERIES_ID: &str = "01900000-0000-7000-8000-000000000103";
const AUCTION_RECRUITMENT_ID: &str = "01900000-0000-7000-8000-000000000201";
const AUCTION_TOURNAMENT_ID: &str = "01900000-0000-7000-8000-000000000202";
const AUCTION_TEAM_A_ID: &str = "01900000-0000-7000-8000-000000000203";
const AUCTION_TEAM_B_ID: &str = "01900000-0000-7000-8000-000000000204";
const AUCTION_MATCH_ID: &str = "01900000-0000-7000-8000-000000000205";
const OAUTH_KEY_BASE64URL: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const DISCORD_CLIENT_ID: &str = "900000000000000001";
const PUBLIC_ORIGIN: &str = "http://localhost:5173";

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn guild_statistics_projects_confirmed_season_insights() -> TestResult {
    tokio::time::timeout(Duration::from_secs(120), run_flow())
        .await
        .map_err(|_| "guild statistics endpoint test exceeded 120 seconds")??;
    Ok(())
}

async fn run_flow() -> TestResult {
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
    seed_login(&auth_repository, VIEWER_ID, "Viewer", VIEWER_SESSION).await?;
    seed_login(&auth_repository, OUTSIDER_ID, "Outsider", OUTSIDER_SESSION).await?;
    seed_statistics(&pool).await?;

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
    let path = format!("/api/guilds/{GUILD_ID}/statistics");

    assert_eq!(
        get(&app, &path, None).await?.status(),
        StatusCode::UNAUTHORIZED
    );
    let outsider = get(&app, &path, Some(OUTSIDER_SESSION)).await?;
    assert_eq!(outsider.status(), StatusCode::NOT_FOUND);
    assert_eq!(body_json(outsider).await?["error"]["code"], "not_found");

    let response = get(&app, &path, Some(VIEWER_SESSION)).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await?;
    assert_eq!(body["data"]["selectedSeasonId"], ACTIVE_SEASON_ID);
    assert_eq!(body["data"]["scope"], "ALL");
    assert_eq!(
        body["data"]["overview"],
        json!({
            "games": 3,
            "regularGames": 2,
            "auctionGames": 1,
            "uniquePlayers": 3,
            "blueWins": 2,
            "redWins": 1,
            "averageDurationSeconds": 1500.0,
            "coverage": {
                "durationGames": 3,
                "participantStatGames": 1,
                "championPickGames": 3,
                "championBanGames": 3
            }
        })
    );
    assert_eq!(body["data"]["players"]["minimumWinRateGames"], 3);
    assert_eq!(
        body["data"]["players"]["mostActive"][0],
        json!({ "userId": VIEWER_ID, "name": "팀장 A", "games": 3, "wins": 3, "losses": 0 })
    );
    assert_eq!(
        body["data"]["players"]["bestWinRates"]
            .as_array()
            .map(Vec::len),
        Some(3)
    );
    assert_eq!(
        body["data"]["players"]["bestWinRates"][0]["userId"],
        VIEWER_ID
    );
    assert_eq!(
        body["data"]["relationships"]["bestDuos"][0],
        json!({
            "userAId": VIEWER_ID,
            "userAName": "팀장 A",
            "userBId": BIDDER_ID,
            "userBName": "고가 낙찰",
            "games": 3,
            "wins": 3,
            "losses": 0
        })
    );
    assert_eq!(
        body["data"]["champions"]["picks"][0],
        json!({ "championId": "Ahri", "lane": "TOP", "games": 3, "wins": 3, "losses": 0 })
    );
    assert_eq!(
        body["data"]["champions"]["bans"][0],
        json!({ "championId": "Jax", "bans": 3 })
    );
    assert_eq!(body["data"]["auction"]["completedTournaments"], 1);
    assert_eq!(
        body["data"]["auction"]["captains"][0],
        json!({
            "userId": VIEWER_ID,
            "name": "팀장 A",
            "teamsCaptained": 1,
            "titles": 1,
            "finalAppearances": 1,
            "matchWins": 1,
            "matchLosses": 0,
            "gameWins": 1,
            "gameLosses": 0,
            "totalSpentPoints": 700
        })
    );
    assert_eq!(
        body["data"]["auction"]["topBids"][0],
        json!({
            "tournamentId": AUCTION_TOURNAMENT_ID,
            "tournamentTitle": "완료 경매",
            "teamName": "파랑 바나나",
            "userId": BIDDER_ID,
            "name": "고가 낙찰",
            "points": 700,
            "games": 1,
            "wins": 1,
            "losses": 0,
            "wonTournament": true
        })
    );

    let regular = get(&app, &format!("{path}?scope=REGULAR"), Some(VIEWER_SESSION)).await?;
    assert_eq!(regular.status(), StatusCode::OK);
    let regular = body_json(regular).await?;
    assert_eq!(regular["data"]["scope"], "REGULAR");
    assert_eq!(regular["data"]["overview"]["games"], 2);
    assert_eq!(regular["data"]["overview"]["auctionGames"], 0);
    assert_eq!(
        regular["data"]["auction"],
        json!({
            "completedTournaments": 0,
            "captains": [],
            "topBids": []
        })
    );

    let auction = get(&app, &format!("{path}?scope=AUCTION"), Some(VIEWER_SESSION)).await?;
    assert_eq!(auction.status(), StatusCode::OK);
    let auction = body_json(auction).await?;
    assert_eq!(auction["data"]["overview"]["games"], 1);
    assert_eq!(auction["data"]["overview"]["regularGames"], 0);
    assert_eq!(
        auction["data"]["auction"]["captains"][0]["userId"],
        VIEWER_ID
    );

    let invalid_scope = get(
        &app,
        &format!("{path}?scope=UNSUPPORTED"),
        Some(VIEWER_SESSION),
    )
    .await?;
    assert_eq!(invalid_scope.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        body_json(invalid_scope).await?["error"]["code"],
        "invalid_scope"
    );

    let invalid_season = get(
        &app,
        &format!("{path}?seasonId=not-a-uuid"),
        Some(VIEWER_SESSION),
    )
    .await?;
    assert_eq!(invalid_season.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        body_json(invalid_season).await?["error"]["code"],
        "invalid_season_id"
    );

    let foreign_season = get(
        &app,
        &format!("{path}?seasonId={FOREIGN_SEASON_ID}"),
        Some(VIEWER_SESSION),
    )
    .await?;
    assert_eq!(foreign_season.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        body_json(foreign_season).await?["error"]["code"],
        "season_not_found"
    );

    let closed = get(
        &app,
        &format!("{path}?seasonId={CLOSED_SEASON_ID}"),
        Some(VIEWER_SESSION),
    )
    .await?;
    assert_eq!(closed.status(), StatusCode::OK);
    let closed = body_json(closed).await?;
    assert_eq!(closed["data"]["selectedSeasonId"], CLOSED_SEASON_ID);
    assert_eq!(closed["data"]["overview"]["games"], 0);
    assert_eq!(closed["data"]["auction"]["captains"], json!([]));

    sqlx::query(
        "UPDATE seasons
         SET status = 'CLOSED', ended_at = clock_timestamp()
         WHERE guild_id = $1 AND id = $2",
    )
    .bind(GUILD_ID)
    .bind(id(ACTIVE_SEASON_ID))
    .execute(&pool)
    .await?;
    let without_active_season = get(&app, &path, Some(VIEWER_SESSION)).await?;
    assert_eq!(without_active_season.status(), StatusCode::OK);
    let without_active_season = body_json(without_active_season).await?;
    assert_eq!(
        without_active_season["data"]["selectedSeasonId"],
        Value::Null
    );
    assert_eq!(without_active_season["data"]["overview"]["games"], 0);

    pool.close().await;
    drop(server);
    Ok(())
}

async fn seed_statistics(pool: &PgPool) -> TestResult {
    sqlx::raw_sql(
        "INSERT INTO guilds (
             guild_id, discord_name, icon_hash, status, installed_by,
             installed_at, activated_at, left_at, updated_at
         ) VALUES
           ('200000000000000001', '통계 길드', NULL, 'ACTIVE', '100000000000000001',
            '2026-08-01T00:00:00Z', '2026-08-01T00:00:00Z', NULL, '2026-08-01T00:00:00Z'),
           ('200000000000000002', '외부 길드', NULL, 'ACTIVE', '100000000000000002',
            '2026-08-01T00:00:00Z', '2026-08-01T00:00:00Z', NULL, '2026-08-01T00:00:00Z');

         INSERT INTO users (user_id, display_name, avatar_hash, created_at, updated_at, deleted_at)
         VALUES
           ('100000000000000003', '팀장 B 사용자', NULL, clock_timestamp(), clock_timestamp(), NULL),
           ('100000000000000004', '고가 낙찰 사용자', NULL, clock_timestamp(), clock_timestamp(), NULL);

         INSERT INTO guild_members (
             guild_id, user_id, display_name, status, main_lane,
             first_seen_at, registered_at, discord_joined_at, last_active_at, last_synced_at,
             inactivated_at
         ) VALUES
           ('200000000000000001', '100000000000000001', '팀장 A', 'ACTIVE', 'TOP',
            clock_timestamp(), clock_timestamp(), clock_timestamp(), clock_timestamp(), clock_timestamp(), NULL),
           ('200000000000000001', '100000000000000003', '팀장 B', 'ACTIVE', 'TOP',
            clock_timestamp(), clock_timestamp(), clock_timestamp(), clock_timestamp(), clock_timestamp(), NULL),
           ('200000000000000001', '100000000000000004', '고가 낙찰', 'ACTIVE', 'JUNGLE',
            clock_timestamp(), clock_timestamp(), clock_timestamp(), clock_timestamp(), clock_timestamp(), NULL),
           ('200000000000000002', '100000000000000002', '외부 사용자', 'ACTIVE', 'MID',
            clock_timestamp(), clock_timestamp(), clock_timestamp(), clock_timestamp(), clock_timestamp(), NULL);

         INSERT INTO seasons (guild_id, id, name, status, started_at, ended_at, created_by, created_at)
         VALUES
           ('200000000000000001', '01900000-0000-7000-8000-000000000010', '현재 시즌', 'ACTIVE',
            '2026-08-01T00:00:00Z', NULL, '100000000000000001', '2026-08-01T00:00:00Z'),
           ('200000000000000001', '01900000-0000-7000-8000-000000000011', '이전 시즌', 'CLOSED',
            '2026-07-01T00:00:00Z', '2026-07-31T23:59:59Z', '100000000000000001', '2026-07-01T00:00:00Z'),
           ('200000000000000002', '01900000-0000-7000-8000-000000000012', '외부 시즌', 'ACTIVE',
            '2026-08-01T00:00:00Z', NULL, '100000000000000002', '2026-08-01T00:00:00Z');

         INSERT INTO recruitments (
             guild_id, id, season_id, display_number, title, target_count, status, revision,
             created_by, created_at, updated_by, updated_at, closed_at, converted_at
         ) VALUES (
             '200000000000000001', '01900000-0000-7000-8000-000000000101',
             '01900000-0000-7000-8000-000000000010', 1, '일반 내전', 10, 'CONVERTED', 1,
             '100000000000000001', '2026-08-01T00:00:00Z', '100000000000000001', '2026-08-01T00:00:00Z',
             '2026-08-01T00:00:00Z', '2026-08-01T00:00:00Z'
         );

         INSERT INTO series (
             guild_id, id, season_id, source_recruitment_id, display_number, title, best_of,
             status, winning_team, terminal_reason, revision, started_at, ended_at, created_by,
             updated_by, updated_at, deleted_at
         ) VALUES
           ('200000000000000001', '01900000-0000-7000-8000-000000000102',
            '01900000-0000-7000-8000-000000000010', '01900000-0000-7000-8000-000000000101',
            1, '완료 일반 내전', 3, 'COMPLETED', 'TEAM_1', 'SCORE_REACHED', 1,
            '2026-08-02T00:00:00Z', '2026-08-02T02:00:00Z', '100000000000000001',
            '100000000000000001', '2026-08-02T02:00:00Z', NULL),
           ('200000000000000001', '01900000-0000-7000-8000-000000000103',
            '01900000-0000-7000-8000-000000000010', '01900000-0000-7000-8000-000000000101',
            2, '취소 일반 내전', 1, 'CANCELLED', NULL, 'CANCELLED', 1,
            '2026-08-03T00:00:00Z', '2026-08-03T01:00:00Z', '100000000000000001',
            '100000000000000001', '2026-08-03T01:00:00Z', NULL);

         INSERT INTO auction_recruitments (
             guild_id, id, season_id, display_number, title, format, target_count, status, revision,
             channel_id, message_id, created_by, created_at, updated_by, updated_at,
             closed_at, converted_at, cancelled_at
         ) VALUES (
             '200000000000000001', '01900000-0000-7000-8000-000000000201',
             '01900000-0000-7000-8000-000000000010', 1, '경매 모집', 'TEN_PERSON', 10, 'CONVERTED', 1,
             NULL, NULL, '100000000000000001', '2026-08-01T00:00:00Z', '100000000000000001',
             '2026-08-01T00:00:00Z', '2026-08-01T00:00:00Z', '2026-08-01T00:00:00Z', NULL
         );",
    )
    .execute(pool)
    .await?;

    seed_auction(pool).await?;
    seed_regular_game(
        pool,
        "01900000-0000-7000-8000-000000000111",
        "01900000-0000-7000-8000-000000000112",
        REGULAR_SERIES_ID,
        1,
        "DEFAULT",
        "BLUE",
        1800,
        false,
    )
    .await?;
    seed_regular_game(
        pool,
        "01900000-0000-7000-8000-000000000113",
        "01900000-0000-7000-8000-000000000114",
        REGULAR_SERIES_ID,
        2,
        "SWAPPED",
        "RED",
        1500,
        false,
    )
    .await?;
    seed_regular_game(
        pool,
        "01900000-0000-7000-8000-000000000115",
        "01900000-0000-7000-8000-000000000116",
        REGULAR_SERIES_ID,
        3,
        "DEFAULT",
        "BLUE",
        2100,
        true,
    )
    .await?;
    seed_regular_game(
        pool,
        "01900000-0000-7000-8000-000000000117",
        "01900000-0000-7000-8000-000000000118",
        CANCELLED_SERIES_ID,
        1,
        "DEFAULT",
        "BLUE",
        2400,
        false,
    )
    .await?;
    Ok(())
}

async fn seed_auction(pool: &PgPool) -> TestResult {
    let tournament_id = id(AUCTION_TOURNAMENT_ID);
    let team_a_id = id(AUCTION_TEAM_A_ID);
    let team_b_id = id(AUCTION_TEAM_B_ID);
    let action_id = id("01900000-0000-7000-8000-000000000206");
    let lot_id = id("01900000-0000-7000-8000-000000000207");
    sqlx::query(
        "INSERT INTO auction_tournaments (
             guild_id, id, season_id, source_recruitment_id, display_number, title, format,
             semifinal_best_of, final_best_of, hard_fearless, status, revision, started_at,
             ended_at, created_by, updated_by, updated_at, deleted_at
         ) VALUES (
             $1, $2, $3, $4, 1, '완료 경매', 'TEN_PERSON', NULL, 3, false,
             'BIDDING', 1, '2026-08-04T00:00:00Z', NULL, $5, $5, '2026-08-04T00:00:00Z', NULL
         )",
    )
    .bind(GUILD_ID)
    .bind(tournament_id)
    .bind(id(ACTIVE_SEASON_ID))
    .bind(id(AUCTION_RECRUITMENT_ID))
    .bind(VIEWER_ID)
    .execute(pool)
    .await?;
    for (team_id, index, name, current_points, revision) in [
        (team_a_id, 1_i16, "파랑 바나나", 300_i32, 1_i64),
        (team_b_id, 2_i16, "빨강 바나나", 1000_i32, 0_i64),
    ] {
        sqlx::query(
            "INSERT INTO auction_teams (
                 guild_id, tournament_id, id, team_index, team_name,
                 initial_points, current_points, point_revision, created_at
             ) VALUES ($1, $2, $3, $4, $5, 1000, $6, $7, '2026-08-04T00:00:00Z')",
        )
        .bind(GUILD_ID)
        .bind(tournament_id)
        .bind(team_id)
        .bind(index)
        .bind(name)
        .bind(current_points)
        .bind(revision)
        .execute(pool)
        .await?;
    }
    sqlx::query(
        "INSERT INTO auction_bid_candidates (
             guild_id, tournament_id, user_id, declared_lane, mmr_at_start,
             snapshot_origin, queue_position, state, draw_count, updated_at
         ) VALUES ($1, $2, $3, 'JUNGLE', 1500, 'CAPTURED', 1, 'ASSIGNED', 1, clock_timestamp())",
    )
    .bind(GUILD_ID)
    .bind(tournament_id)
    .bind(BIDDER_ID)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO auction_lots (
             guild_id, tournament_id, id, draw_sequence, target_user_id, status,
             opened_by_type, opened_by_id, opened_at, closed_by_type, closed_by_id, closed_at
         ) VALUES ($1, $2, $3, 1, $4, 'AWARDED', 'USER', $5, clock_timestamp(),
                   'USER', $5, clock_timestamp())",
    )
    .bind(GUILD_ID)
    .bind(tournament_id)
    .bind(lot_id)
    .bind(BIDDER_ID)
    .bind(VIEWER_ID)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO auction_actions (
             guild_id, tournament_id, id, sequence, kind, target_user_id, team_id, lot_id,
             points, reverses_action_id, reversed_action_kind, actor_type, actor_id, created_at
         ) VALUES ($1, $2, $3, 1, 'AWARD_BID', $4, $5, $6, 700, NULL, NULL,
                   'USER', $7, clock_timestamp())",
    )
    .bind(GUILD_ID)
    .bind(tournament_id)
    .bind(action_id)
    .bind(BIDDER_ID)
    .bind(team_a_id)
    .bind(lot_id)
    .bind(VIEWER_ID)
    .execute(pool)
    .await?;
    for (member_id, team_id, user_id, slot, via, action, action_kind, lane) in [
        (
            "01900000-0000-7000-8000-000000000208",
            team_a_id,
            VIEWER_ID,
            1_i16,
            "CAPTAIN",
            None,
            None,
            "TOP",
        ),
        (
            "01900000-0000-7000-8000-000000000209",
            team_b_id,
            CAPTAIN_B_ID,
            1_i16,
            "CAPTAIN",
            None,
            None,
            "TOP",
        ),
        (
            "01900000-0000-7000-8000-000000000210",
            team_a_id,
            BIDDER_ID,
            2_i16,
            "BID",
            Some(action_id),
            Some("AWARD_BID"),
            "JUNGLE",
        ),
    ] {
        sqlx::query(
            "INSERT INTO auction_team_members (
                 guild_id, tournament_id, team_id, id, user_id, roster_slot, acquired_via,
                 declared_lane, mmr_at_acquisition, snapshot_origin, acquisition_action_id,
                 acquisition_action_kind, joined_at, released_at
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 1500, 'CAPTURED', $9, $10,
                       clock_timestamp(), NULL)",
        )
        .bind(GUILD_ID)
        .bind(tournament_id)
        .bind(team_id)
        .bind(id(member_id))
        .bind(user_id)
        .bind(slot)
        .bind(via)
        .bind(lane)
        .bind(action)
        .bind(action_kind)
        .execute(pool)
        .await?;
    }
    seed_point_ledger(pool, tournament_id, team_a_id, team_b_id, action_id).await?;
    sqlx::query(
        "UPDATE auction_tournaments
         SET status = 'COMPLETED', champion_team_id = $3,
             ended_at = '2026-08-04T02:00:00Z', updated_at = '2026-08-04T02:00:00Z'
         WHERE guild_id = $1 AND id = $2",
    )
    .bind(GUILD_ID)
    .bind(tournament_id)
    .bind(team_a_id)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO auction_matches (
             guild_id, tournament_id, id, season_id, match_key, best_of, hard_fearless,
             left_team_id, right_team_id, status, winner_team_id, revision,
             lineups_locked_at, lineups_locked_by, lineup_lock_invariant_version,
             started_at, ended_at, updated_by, updated_at, deleted_at
         ) VALUES ($1, $2, $3, $4, 'FINAL', 3, false, $5, $6, 'COMPLETED', $5, 1,
                   '2026-08-04T00:30:00Z', $7, 1,
                   '2026-08-04T01:00:00Z', '2026-08-04T02:00:00Z', $7, '2026-08-04T02:00:00Z', NULL)",
    )
    .bind(GUILD_ID)
    .bind(tournament_id)
    .bind(id(AUCTION_MATCH_ID))
    .bind(id(ACTIVE_SEASON_ID))
    .bind(team_a_id)
    .bind(team_b_id)
    .bind(VIEWER_ID)
    .execute(pool)
    .await?;
    seed_auction_game(pool).await
}

async fn seed_point_ledger(
    pool: &PgPool,
    tournament_id: Uuid,
    team_a_id: Uuid,
    team_b_id: Uuid,
    action_id: Uuid,
) -> TestResult {
    for (entry_id, team_id) in [
        ("01900000-0000-7000-8000-000000000211", team_a_id),
        ("01900000-0000-7000-8000-000000000212", team_b_id),
    ] {
        sqlx::query(
            "INSERT INTO auction_point_ledger (
                 guild_id, tournament_id, id, team_id, point_revision, reason,
                 source_action_id, source_action_kind, points_before, points_after, delta,
                 reverses_entry_id, reversed_reason, actor_type, actor_id, created_at
             ) VALUES ($1, $2, $3, $4, 0, 'INITIAL', NULL, NULL, 0, 1000, 1000,
                       NULL, NULL, 'SYSTEM', $5, clock_timestamp())",
        )
        .bind(GUILD_ID)
        .bind(tournament_id)
        .bind(id(entry_id))
        .bind(team_id)
        .bind(VIEWER_ID)
        .execute(pool)
        .await?;
    }
    sqlx::query(
        "INSERT INTO auction_point_ledger (
             guild_id, tournament_id, id, team_id, point_revision, reason,
             source_action_id, source_action_kind, points_before, points_after, delta,
             reverses_entry_id, reversed_reason, actor_type, actor_id, created_at
         ) VALUES ($1, $2, $3, $4, 1, 'BID_DEBIT', $5, 'AWARD_BID', 1000, 300, -700,
                   NULL, NULL, 'USER', $6, clock_timestamp())",
    )
    .bind(GUILD_ID)
    .bind(tournament_id)
    .bind(id("01900000-0000-7000-8000-000000000213"))
    .bind(team_a_id)
    .bind(action_id)
    .bind(VIEWER_ID)
    .execute(pool)
    .await?;
    Ok(())
}

async fn seed_auction_game(pool: &PgPool) -> TestResult {
    let input_id = id("01900000-0000-7000-8000-000000000214");
    let game_id = id("01900000-0000-7000-8000-000000000215");
    sqlx::query(
        "INSERT INTO game_inputs (
             guild_id, id, series_id, auction_match_id, set_number, side_preset,
             winning_side, patch_version, status, updated_by, updated_at
         ) VALUES ($1, $2, NULL, $3, 1, 'DEFAULT', 'BLUE', '26.13', 'SUBMITTED', $4, clock_timestamp())",
    )
    .bind(GUILD_ID)
    .bind(input_id)
    .bind(id(AUCTION_MATCH_ID))
    .bind(VIEWER_ID)
    .execute(pool)
    .await?;
    insert_game(
        pool,
        game_id,
        input_id,
        None,
        Some(id(AUCTION_MATCH_ID)),
        1,
        "DEFAULT",
        "BLUE",
        1200,
        false,
    )
    .await?;
    seed_game_projection(pool, game_id, true).await?;
    sqlx::query(
        "INSERT INTO game_participant_stats (guild_id, game_id, user_id, kills, deaths, assists, cs)
         VALUES ($1, $2, $3, 4, 1, 9, 180)",
    )
    .bind(GUILD_ID)
    .bind(game_id)
    .bind(VIEWER_ID)
    .execute(pool)
    .await?;
    Ok(())
}

async fn seed_regular_game(
    pool: &PgPool,
    input_id: &str,
    game_id: &str,
    series_id: &str,
    game_number: i16,
    side_preset: &str,
    winning_side: &str,
    duration_seconds: i32,
    voided: bool,
) -> TestResult {
    let input_id = id(input_id);
    let game_id = id(game_id);
    let series_id = id(series_id);
    sqlx::query(
        "INSERT INTO game_inputs (
             guild_id, id, series_id, auction_match_id, set_number, side_preset,
             winning_side, patch_version, status, updated_by, updated_at
         ) VALUES ($1, $2, $3, NULL, $4, $5, $6, '26.13', $7, $8, clock_timestamp())",
    )
    .bind(GUILD_ID)
    .bind(input_id)
    .bind(series_id)
    .bind(game_number)
    .bind(side_preset)
    .bind(winning_side)
    .bind(if voided { "SUPERSEDED" } else { "SUBMITTED" })
    .bind(VIEWER_ID)
    .execute(pool)
    .await?;
    insert_game(
        pool,
        game_id,
        input_id,
        Some(series_id),
        None,
        game_number,
        side_preset,
        winning_side,
        duration_seconds,
        voided,
    )
    .await?;
    if !voided && series_id == id(REGULAR_SERIES_ID) {
        seed_game_projection(pool, game_id, true).await?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_game(
    pool: &PgPool,
    game_id: Uuid,
    input_id: Uuid,
    series_id: Option<Uuid>,
    auction_match_id: Option<Uuid>,
    game_number: i16,
    side_preset: &str,
    winning_side: &str,
    duration_seconds: i32,
    voided: bool,
) -> TestResult {
    let voided_at = voided.then(OffsetDateTime::now_utc);
    sqlx::query(
        "INSERT INTO games (
             guild_id, id, season_id, series_id, auction_match_id, source_input_id,
             game_number, side_preset, winning_side, duration_seconds, patch_version,
             riot_match_id, status, mmr_status, played_at, confirmed_by, confirmed_by_type,
             confirmed_at, voided_by, voided_by_type, voided_at, void_reason
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, '26.13', NULL,
                   $11, $12, '2026-08-04T01:30:00Z', $13, 'USER', '2026-08-04T01:31:00Z',
                   $14, $15, $16, $17)",
    )
    .bind(GUILD_ID)
    .bind(game_id)
    .bind(id(ACTIVE_SEASON_ID))
    .bind(series_id)
    .bind(auction_match_id)
    .bind(input_id)
    .bind(game_number)
    .bind(side_preset)
    .bind(winning_side)
    .bind(duration_seconds)
    .bind(if voided { "VOIDED" } else { "CONFIRMED" })
    .bind(if voided { "REVERSED" } else { "APPLIED" })
    .bind(VIEWER_ID)
    .bind(if voided { Some(VIEWER_ID) } else { None })
    .bind(if voided { Some("USER") } else { None })
    .bind(voided_at)
    .bind(if voided { Some("fixture void") } else { None })
    .execute(pool)
    .await?;
    Ok(())
}

async fn seed_game_projection(pool: &PgPool, game_id: Uuid, include_bans: bool) -> TestResult {
    for (user_id, team, lane, champion_id) in [
        (VIEWER_ID, "TEAM_1", "TOP", "Ahri"),
        (BIDDER_ID, "TEAM_1", "JUNGLE", "Viego"),
        (CAPTAIN_B_ID, "TEAM_2", "TOP", "Orianna"),
    ] {
        sqlx::query(
            "INSERT INTO game_participants (guild_id, game_id, user_id, team, lane, champion_id)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(GUILD_ID)
        .bind(game_id)
        .bind(user_id)
        .bind(team)
        .bind(lane)
        .bind(champion_id)
        .execute(pool)
        .await?;
    }
    if include_bans {
        sqlx::query(
            "INSERT INTO game_bans (guild_id, game_id, side, slot, champion_id)
             VALUES ($1, $2, 'BLUE', 1, 'Jax')",
        )
        .bind(GUILD_ID)
        .bind(game_id)
        .execute(pool)
        .await?;
    }
    Ok(())
}

fn id(value: &str) -> Uuid {
    Uuid::parse_str(value).expect("fixture UUID must be valid")
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

async fn get(app: &Router, uri: &str, session: Option<&str>) -> TestResult<Response<Body>> {
    let mut request = Request::get(uri);
    if let Some(session) = session {
        request = request.header(header::COOKIE, format!("monkeybot_session={session}"));
    }
    Ok(app.clone().oneshot(request.body(Body::empty())?).await?)
}

async fn body_json(response: Response<Body>) -> TestResult<Value> {
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn test_config(database_url: String) -> TestResult<Config> {
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
