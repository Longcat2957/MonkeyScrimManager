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
    infrastructure::postgres::{MIGRATOR, PostgresReadinessProbe},
    integrations::discord::{DiscordClient, DiscordClientError, DiscordUser},
    repository::auth::AuthRepository,
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
use tower::ServiceExt;
use uuid::Uuid;

const POSTGRES_IMAGE: &str = "postgres";
const POSTGRES_TAG: &str = "18.4-alpine3.23";
const POSTGRES_PASSWORD: &str = "monkeybot-workflow-read-password";
const DATABASE: &str = "monkeybot_workflow_reads";

const USER_ID: &str = "100000000000000001";
const MEMBER_2_ID: &str = "100000000000000002";
const MEMBER_3_ID: &str = "100000000000000003";
const MEMBER_4_ID: &str = "100000000000000004";
const OUTSIDER_ID: &str = "100000000000000099";
const GUILD_A: &str = "200000000000000001";
const GUILD_B: &str = "200000000000000002";
const OPERATOR_ROLE_ID: &str = "300000000000000001";
const RECRUITMENT_CHANNEL_ID: &str = "400000000000000001";
const DISCORD_CLIENT_ID: &str = "900000000000000001";
const PUBLIC_ORIGIN: &str = "http://localhost:5173";
const OAUTH_KEY_BASE64URL: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const BOT_TOKEN: &str = "bot-service-token-that-is-at-least-32-characters";
const SESSION_TOKEN: &str = "workflow-member-session";
const OUTSIDER_SESSION_TOKEN: &str = "workflow-outsider-session";

const ACTIONABLE_RECRUITMENT: &str = "01900000-0000-7000-8000-000000000101";
const ACTIVE_SOURCE_RECRUITMENT: &str = "01900000-0000-7000-8000-000000000102";
const COMPLETED_SOURCE_RECRUITMENT: &str = "01900000-0000-7000-8000-000000000103";
const ACTIVE_SERIES: &str = "01900000-0000-7000-8000-000000000201";
const COMPLETED_SERIES: &str = "01900000-0000-7000-8000-000000000202";

const AUCTION_ACTIONABLE_10: &str = "01900000-0000-7000-8000-000000000501";
const AUCTION_ACTIONABLE_20: &str = "01900000-0000-7000-8000-000000000502";
const AUCTION_ACTIVE_SOURCE: &str = "01900000-0000-7000-8000-000000000503";
const AUCTION_COMPLETED_SOURCE: &str = "01900000-0000-7000-8000-000000000504";
const AUCTION_CANCELLED_SOURCE: &str = "01900000-0000-7000-8000-000000000505";
const ACTIVE_TOURNAMENT: &str = "01900000-0000-7000-8000-000000000601";
const COMPLETED_TOURNAMENT: &str = "01900000-0000-7000-8000-000000000602";
const CANCELLED_TOURNAMENT: &str = "01900000-0000-7000-8000-000000000603";
const ACTIVE_MATCH: &str = "01900000-0000-7000-8000-000000000801";
const ACTIVE_SEMIFINAL_B_MATCH: &str = "01900000-0000-7000-8000-000000000802";
const ACTIVE_FINAL_MATCH: &str = "01900000-0000-7000-8000-000000000803";
const CANCELLED_SEMIFINAL_A_HISTORY_MATCH: &str = "01900000-0000-7000-8000-000000000804";

const ACTIVE_TEAM_1: &str = "01900000-0000-7000-8000-000000000701";
const ACTIVE_TEAM_2: &str = "01900000-0000-7000-8000-000000000702";
const ACTIVE_TEAM_3: &str = "01900000-0000-7000-8000-000000000703";
const ACTIVE_TEAM_4: &str = "01900000-0000-7000-8000-000000000704";
const COMPLETED_TEAM_1: &str = "01900000-0000-7000-8000-000000000705";
const COMPLETED_TEAM_2: &str = "01900000-0000-7000-8000-000000000706";
const CANCELLED_TEAM_1: &str = "01900000-0000-7000-8000-000000000707";
const CANCELLED_TEAM_2: &str = "01900000-0000-7000-8000-000000000708";

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn workflow_read_endpoints_project_postgres_state_and_isolate_tenants() -> TestResult {
    tokio::time::timeout(Duration::from_secs(180), run_workflow_read_flow())
        .await
        .map_err(|_| "workflow read integration test exceeded 180 seconds")??;
    Ok(())
}

async fn run_workflow_read_flow() -> TestResult {
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
        Arc::new(AuthRepository::new(pool.clone())),
        Arc::new(UnusedDiscordClient),
    ));
    let app = build_router(state);

    seed_identity_and_guilds(&pool).await?;
    let (season_a, season_b) = seed_seasons(&pool).await?;
    seed_sessions(&pool).await?;
    seed_ordinary_workflow(&pool, season_a, season_b).await?;
    seed_auction_workflow(&pool, season_a).await?;

    let member_cookie = format!("monkeybot_session={SESSION_TOKEN}");
    let outsider_cookie = format!("monkeybot_session={OUTSIDER_SESSION_TOKEN}");

    assert_unauthorized_and_hidden(&app, &member_cookie, &outsider_cookie).await?;
    assert_operations(&app, &member_cookie).await?;
    assert_actionable_recruitment(&app, &member_cookie).await?;
    assert_series(&app, &member_cookie).await?;
    assert_auction_recruitment_cursor(&app, &member_cookie).await?;
    seed_operator_permissions(&pool).await?;
    assert_auction_tournaments(&app, &member_cookie).await?;
    assert_cancelled_auction_tournament_archive(&app, &member_cookie).await?;
    assert_bidding_auction_tournament_bracket_is_unavailable(&app, &pool, &member_cookie).await?;
    assert_auction_tournament_cancel_preview(&app, &member_cookie, &outsider_cookie).await?;
    assert_twenty_person_semifinal_game_requires_both_lineups(&app, &pool).await?;
    assert_legacy_auction_match_lock_exemption(&pool).await?;
    assert_active_auction_tournament_result_ignores_cancelled_match_history(
        &app,
        &pool,
        season_a,
        &member_cookie,
    )
    .await?;
    assert_browser_auction_tournament_cancel(&app).await?;

    pool.close().await;
    drop(server);
    Ok(())
}

async fn assert_unauthorized_and_hidden(
    app: &Router,
    member_cookie: &str,
    outsider_cookie: &str,
) -> TestResult {
    let guild_a_paths = workflow_paths(GUILD_A);
    for path in &guild_a_paths {
        let response = get(app, path, None).await?;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
        assert_eq!(body_json(response).await?["error"]["code"], "unauthorized");
    }

    for path in workflow_paths(GUILD_B) {
        let response = get(app, &path, Some(member_cookie)).await?;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        assert_eq!(body_json(response).await?["error"]["code"], "not_found");
    }

    for path in guild_a_paths {
        let response = get(app, &path, Some(outsider_cookie)).await?;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        assert_eq!(body_json(response).await?["error"]["code"], "not_found");
    }
    Ok(())
}

async fn assert_operations(app: &Router, cookie: &str) -> TestResult {
    let path = format!("/api/guilds/{GUILD_A}/operations");
    let response = get(app, &path, Some(cookie)).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await?;
    let operations = body["data"]["operations"]
        .as_array()
        .ok_or("operations response was not an array")?;
    assert_eq!(operations.len(), 5, "{body}");

    assert_eq!(operations[0]["kind"], "PICK_BAN");
    assert_eq!(operations[0]["priority"], "urgent");
    assert_eq!(operations[0]["status"], "픽밴/결과 입력");
    assert_eq!(operations[0]["detail"], "1세트 완료 · 2세트 입력 대기");
    assert_eq!(
        operations[0]["href"],
        format!("/g/{GUILD_A}/series/{ACTIVE_SERIES}/pick-ban")
    );

    assert_eq!(operations[1]["kind"], "ENTRY");
    assert_eq!(operations[1]["detail"], "2/10명 · 1명 미배정");
    assert_eq!(operations[1]["updatedAt"], "2026-07-10T12:00:00Z");

    assert_eq!(operations[2]["kind"], "AUCTION");
    assert_eq!(operations[2]["status"], "결과 입력");
    assert_eq!(
        operations[2]["href"],
        format!("/g/{GUILD_A}/auction/matches/{ACTIVE_MATCH}")
    );
    assert_eq!(operations[3]["title"], "20인 경매 모집");
    assert_eq!(operations[4]["title"], "10인 경매 모집");
    assert!(!body.to_string().contains("다른 길드 비공개 모집"));
    Ok(())
}

async fn assert_actionable_recruitment(app: &Router, cookie: &str) -> TestResult {
    let path = format!("/api/guilds/{GUILD_A}/recruitments?state=actionable");
    let response = get(app, &path, Some(cookie)).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await?;
    let rows = body["data"]["recruitments"]
        .as_array()
        .ok_or("recruitments response was not an array")?;
    assert_eq!(rows.len(), 1, "{body}");
    let row = &rows[0];
    assert_eq!(row["id"], ACTIONABLE_RECRUITMENT);
    assert_eq!(row["displayNumber"], 214);
    assert_eq!(row["status"], "CLOSED");
    assert_eq!(row["targetCount"], 10);
    assert_eq!(row["joinedCount"], 2);
    assert_eq!(row["assignedCount"], 1);
    assert_eq!(row["revision"], 4);
    assert_eq!(row["updatedAt"], "2026-07-10T12:00:00Z");
    assert_eq!(
        row["href"],
        format!("/g/{GUILD_A}/recruitments/{ACTIONABLE_RECRUITMENT}/entry")
    );
    assert!(body["data"]["nextCursor"].is_null());
    Ok(())
}

async fn assert_series(app: &Router, cookie: &str) -> TestResult {
    let active_path = format!("/api/guilds/{GUILD_A}/series?state=active");
    let active = get(app, &active_path, Some(cookie)).await?;
    assert_eq!(active.status(), StatusCode::OK);
    let active = body_json(active).await?;
    let row = &active["data"]["series"][0];
    assert_eq!(row["id"], ACTIVE_SERIES);
    assert_eq!(row["bestOf"], 3);
    assert_eq!(row["status"], "IN_PROGRESS");
    assert_eq!(row["team1Score"], 0);
    assert_eq!(row["team2Score"], 1);
    assert_eq!(row["currentSetNumber"], 2);
    assert!(row["outcome"].is_null());
    assert!(row["winningTeam"].is_null());
    assert!(row["endedAt"].is_null());
    assert_eq!(
        row["href"],
        format!("/g/{GUILD_A}/series/{ACTIVE_SERIES}/pick-ban")
    );

    let completed_path = format!("/api/guilds/{GUILD_A}/series?state=completed");
    let completed = get(app, &completed_path, Some(cookie)).await?;
    assert_eq!(completed.status(), StatusCode::OK);
    let completed = body_json(completed).await?;
    let rows = completed["data"]["series"]
        .as_array()
        .ok_or("completed series response was not an array")?;
    assert_eq!(rows.len(), 1, "{completed}");
    let row = &rows[0];
    assert_eq!(row["id"], COMPLETED_SERIES);
    assert_eq!(row["team1Score"], 2);
    assert_eq!(row["team2Score"], 1, "voided game must not affect score");
    assert_eq!(row["winningTeam"], "TEAM_1");
    assert_eq!(row["terminalReason"], "SCORE_REACHED");
    assert_eq!(row["outcome"], "TEAM_1");
    assert!(row["currentSetNumber"].is_null());
    assert_eq!(row["endedAt"], "2026-07-10T11:00:00Z");
    assert_eq!(
        row["href"],
        format!("/g/{GUILD_A}/series/{COMPLETED_SERIES}/result")
    );
    Ok(())
}

async fn assert_auction_recruitment_cursor(app: &Router, cookie: &str) -> TestResult {
    let first_path = format!("/api/guilds/{GUILD_A}/auction-recruitments?state=actionable&limit=1");
    let first = get(app, &first_path, Some(cookie)).await?;
    assert_eq!(first.status(), StatusCode::OK);
    let first = body_json(first).await?;
    let first_row = &first["data"]["recruitments"][0];
    assert_eq!(first_row["id"], AUCTION_ACTIONABLE_20);
    assert_eq!(first_row["format"], "TWENTY_PERSON");
    assert_eq!(first_row["participantCount"], 20);
    assert_eq!(first_row["joinedParticipantCount"], 3);
    assert_eq!(first_row["teamCount"], 4);
    let cursor = first["data"]["nextCursor"]
        .as_str()
        .ok_or("first auction recruitment page had no cursor")?;

    let second_path = format!(
        "/api/guilds/{GUILD_A}/auction-recruitments?state=actionable&limit=1&cursor={cursor}"
    );
    let second = get(app, &second_path, Some(cookie)).await?;
    assert_eq!(second.status(), StatusCode::OK);
    let second = body_json(second).await?;
    let second_row = &second["data"]["recruitments"][0];
    assert_eq!(second_row["id"], AUCTION_ACTIONABLE_10);
    assert_eq!(second_row["format"], "TEN_PERSON");
    assert_eq!(second_row["participantCount"], 10);
    assert_eq!(second_row["joinedParticipantCount"], 2);
    assert_eq!(second_row["teamCount"], 2);
    assert!(second["data"]["nextCursor"].is_null());
    Ok(())
}

async fn assert_auction_tournaments(app: &Router, cookie: &str) -> TestResult {
    let active_path = format!("/api/guilds/{GUILD_A}/auction-tournaments?state=active");
    let active = get(app, &active_path, Some(cookie)).await?;
    assert_eq!(active.status(), StatusCode::OK);
    let active = body_json(active).await?;
    assert_eq!(
        active["data"]["tournaments"].as_array().map(Vec::len),
        Some(1)
    );
    let row = &active["data"]["tournaments"][0];
    assert_eq!(row["id"], ACTIVE_TOURNAMENT);
    assert_eq!(row["format"], "TWENTY_PERSON");
    assert_eq!(row["participantCount"], 20);
    assert_eq!(row["teamCount"], 4);
    assert_eq!(row["status"], "IN_GAME");
    assert_eq!(row["activeMemberCount"], 4);
    assert_eq!(row["bidMemberCount"], 0);
    assert_eq!(row["incompleteMatchCount"], 1);
    assert_eq!(row["nextMatch"]["id"], ACTIVE_MATCH);
    assert_eq!(row["nextMatch"]["label"], "준결승 A");
    assert_eq!(row["nextMatch"]["matchKey"], "SEMIFINAL_A");
    assert_eq!(row["nextMatch"]["status"], "READY");
    assert_eq!(row["nextMatch"]["confirmedGameCount"], 0);
    assert!(row["championTeam"].is_null());
    assert_eq!(
        row["href"],
        format!("/g/{GUILD_A}/auction/matches/{ACTIVE_MATCH}")
    );

    let draft_path = format!("/api/guilds/{GUILD_A}/auction-tournaments/{ACTIVE_TOURNAMENT}");
    let draft = get(app, &draft_path, Some(cookie)).await?;
    assert_eq!(draft.status(), StatusCode::OK);
    let draft = body_json(draft).await?;
    assert!(draft["data"]["permissions"]["canManageDraft"].is_boolean());
    assert!(draft["data"]["permissions"]["canRevertLast"].is_boolean());
    assert_eq!(draft["data"]["permissions"]["canCancel"], true);

    let bracket_path =
        format!("/api/guilds/{GUILD_A}/auction-tournaments/{ACTIVE_TOURNAMENT}/bracket");
    let bracket = get(app, &bracket_path, Some(cookie)).await?;
    assert_eq!(bracket.status(), StatusCode::OK);
    let bracket = body_json(bracket).await?;
    assert!(bracket["data"]["permissions"]["canRevertDraft"].is_boolean());
    assert_eq!(bracket["data"]["permissions"]["canCancel"], true);

    let bot_active = bot_get(app, &active_path).await?;
    assert_eq!(bot_active.status(), StatusCode::OK);
    let bot_active = body_json(bot_active).await?;
    assert_eq!(
        bot_active["data"]["tournaments"][0]["id"],
        ACTIVE_TOURNAMENT
    );

    let completed_path = format!("/api/guilds/{GUILD_A}/auction-tournaments?state=completed");
    let completed = get(app, &completed_path, Some(cookie)).await?;
    assert_eq!(completed.status(), StatusCode::OK);
    let completed = body_json(completed).await?;
    assert_eq!(
        completed["data"]["tournaments"].as_array().map(Vec::len),
        Some(2)
    );
    let row = &completed["data"]["tournaments"][0];
    assert_eq!(row["id"], COMPLETED_TOURNAMENT);
    assert_eq!(row["format"], "TEN_PERSON");
    assert_eq!(row["participantCount"], 10);
    assert_eq!(row["teamCount"], 2);
    assert_eq!(row["status"], "COMPLETED");
    assert_eq!(row["activeMemberCount"], 2);
    assert_eq!(row["incompleteMatchCount"], 0);
    assert_eq!(row["championTeam"]["id"], COMPLETED_TEAM_1);
    assert_eq!(row["championTeam"]["name"], "완료 1팀");
    assert!(row["nextMatch"].is_null());
    assert_eq!(row["endedAt"], "2026-07-09T18:00:00Z");
    assert_eq!(
        row["href"],
        format!("/g/{GUILD_A}/auction/tournaments/{COMPLETED_TOURNAMENT}/result")
    );
    let cancelled = &completed["data"]["tournaments"][1];
    assert_eq!(cancelled["id"], CANCELLED_TOURNAMENT);
    assert_eq!(cancelled["status"], "CANCELLED");
    assert_eq!(cancelled["incompleteMatchCount"], 0);
    assert!(cancelled["nextMatch"].is_null());
    assert!(cancelled["championTeam"].is_null());
    assert_eq!(
        cancelled["href"],
        format!("/g/{GUILD_A}/auction/tournaments/{CANCELLED_TOURNAMENT}/result")
    );
    Ok(())
}

async fn assert_cancelled_auction_tournament_archive(app: &Router, cookie: &str) -> TestResult {
    let path = format!("/api/guilds/{GUILD_A}/auction-tournaments/{CANCELLED_TOURNAMENT}/result");
    let response = get(app, &path, Some(cookie)).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await?;
    assert_eq!(body["data"]["tournament"]["status"], "CANCELLED");
    assert_eq!(body["data"]["matches"].as_array().map(Vec::len), Some(0));
    assert_eq!(body["data"]["teams"].as_array().map(Vec::len), Some(2));
    assert_eq!(body["data"]["players"].as_array().map(Vec::len), Some(0));
    assert_eq!(body["data"]["permissions"]["canRevertResults"], false);
    assert_eq!(body["data"]["permissions"]["canRecoverBracket"], false);
    assert_eq!(body["data"]["permissions"]["canCancel"], false);
    Ok(())
}

async fn assert_active_auction_tournament_result_ignores_cancelled_match_history(
    app: &Router,
    pool: &PgPool,
    season_id: Uuid,
    cookie: &str,
) -> TestResult {
    sqlx::query(
        "UPDATE auction_matches
         SET status = 'LINEUP_PENDING',
             lineups_locked_at = NULL,
             lineups_locked_by = NULL
         WHERE guild_id = $1 AND id = $2",
    )
    .bind(GUILD_A)
    .bind(id(ACTIVE_MATCH))
    .execute(pool)
    .await?;

    sqlx::query(
        "INSERT INTO auction_matches (
            guild_id, tournament_id, id, season_id, match_key, best_of,
            left_team_id, right_team_id, status, revision,
            ended_at, updated_by, updated_at
         ) VALUES
           ($1, $2, $3, $4, 'SEMIFINAL_B', 3,
            $5, $6, 'LINEUP_PENDING', 2,
            NULL, $7, '2026-07-10T12:30:00Z'),
           ($1, $2, $8, $4, 'FINAL', 3,
            NULL, NULL, 'LINEUP_PENDING', 0,
            NULL, $7, '2026-07-10T12:30:00Z'),
           ($1, $2, $9, $4, 'SEMIFINAL_A', 3,
            $10, $11, 'CANCELLED', 1,
            '2026-07-10T12:10:00Z', $7, '2026-07-10T12:10:00Z')",
    )
    .bind(GUILD_A)
    .bind(id(ACTIVE_TOURNAMENT))
    .bind(id(ACTIVE_SEMIFINAL_B_MATCH))
    .bind(season_id)
    .bind(id(ACTIVE_TEAM_3))
    .bind(id(ACTIVE_TEAM_4))
    .bind(USER_ID)
    .bind(id(ACTIVE_FINAL_MATCH))
    .bind(id(CANCELLED_SEMIFINAL_A_HISTORY_MATCH))
    .bind(id(ACTIVE_TEAM_1))
    .bind(id(ACTIVE_TEAM_2))
    .execute(pool)
    .await?;

    let path = format!("/api/guilds/{GUILD_A}/auction-tournaments/{ACTIVE_TOURNAMENT}/result");
    let response = get(app, &path, Some(cookie)).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await?;

    assert_eq!(body["data"]["tournament"]["id"], ACTIVE_TOURNAMENT);
    assert_eq!(body["data"]["tournament"]["status"], "IN_GAME");
    let matches = body["data"]["matches"]
        .as_array()
        .ok_or("auction tournament result matches was not an array")?;
    assert_eq!(matches.len(), 3, "{body}");
    assert_eq!(matches[0]["id"], ACTIVE_MATCH);
    assert_eq!(matches[0]["matchKey"], "SEMIFINAL_A");
    assert_eq!(matches[1]["id"], ACTIVE_SEMIFINAL_B_MATCH);
    assert_eq!(matches[1]["matchKey"], "SEMIFINAL_B");
    assert_eq!(matches[2]["id"], ACTIVE_FINAL_MATCH);
    assert_eq!(matches[2]["matchKey"], "FINAL");
    assert!(matches.iter().all(|row| {
        row["id"] != CANCELLED_SEMIFINAL_A_HISTORY_MATCH && row["status"] != "CANCELLED"
    }));
    Ok(())
}

async fn assert_bidding_auction_tournament_bracket_is_unavailable(
    app: &Router,
    pool: &PgPool,
    cookie: &str,
) -> TestResult {
    sqlx::query(
        "UPDATE auction_tournaments
         SET status = 'BIDDING', champion_team_id = NULL, ended_at = NULL
         WHERE guild_id = $1 AND id = $2",
    )
    .bind(GUILD_A)
    .bind(id(COMPLETED_TOURNAMENT))
    .execute(pool)
    .await?;

    let path = format!("/api/guilds/{GUILD_A}/auction-tournaments/{COMPLETED_TOURNAMENT}/bracket");
    let response = get(app, &path, Some(cookie)).await?;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = body_json(response).await?;
    assert_eq!(
        body["error"]["code"],
        "auction_tournament_bracket_unavailable"
    );
    assert_eq!(
        body["error"]["message"],
        "경매 드래프트가 완료된 후 브래킷을 확인할 수 있습니다."
    );
    assert_eq!(body["error"]["details"]["currentStatus"], "BIDDING");
    assert!(body["error"]["requestId"].is_string());
    Ok(())
}

async fn assert_auction_tournament_cancel_preview(
    app: &Router,
    member_cookie: &str,
    outsider_cookie: &str,
) -> TestResult {
    let path =
        format!("/api/guilds/{GUILD_A}/auction-tournaments/{CANCELLED_TOURNAMENT}/cancel-preview");
    let response = bot_get(app, &path).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await?;
    assert_eq!(body["data"]["tournament"]["id"], CANCELLED_TOURNAMENT);
    assert_eq!(body["data"]["tournament"]["displayNumber"], 116);
    assert_eq!(body["data"]["tournament"]["status"], "CANCELLED");
    assert_eq!(body["data"]["tournament"]["revision"], 6);

    let active_path =
        format!("/api/guilds/{GUILD_A}/auction-tournaments/{ACTIVE_TOURNAMENT}/cancel-preview");
    let browser_preview = get(app, &active_path, Some(member_cookie)).await?;
    assert_eq!(browser_preview.status(), StatusCode::OK);
    let browser_preview = body_json(browser_preview).await?;
    assert_eq!(
        browser_preview["data"]["tournament"]["id"],
        ACTIVE_TOURNAMENT
    );
    assert_eq!(browser_preview["data"]["tournament"]["status"], "IN_GAME");
    assert_eq!(browser_preview["data"]["tournament"]["revision"], 7);

    assert_error(
        get(app, &active_path, Some(outsider_cookie)).await?,
        StatusCode::FORBIDDEN,
        "operator_required",
    )
    .await?;
    Ok(())
}

async fn assert_browser_auction_tournament_cancel(app: &Router) -> TestResult {
    let path = format!("/api/guilds/{GUILD_A}/auction-tournaments/{ACTIVE_TOURNAMENT}/cancel");
    let body = json!({
        "expectedRevision": 7,
        "reason": "운영자 요청으로 경매 내전을 조기 종료합니다."
    });
    let response = browser_post(
        app,
        &path,
        "01900000-0000-7000-8000-000000000911",
        body.clone(),
    )
    .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let response = body_json(response).await?;
    assert_eq!(response["data"]["tournamentId"], ACTIVE_TOURNAMENT);
    assert_eq!(response["data"]["tournamentStatus"], "CANCELLED");
    assert_eq!(
        response["data"]["sourceRecruitmentId"],
        AUCTION_ACTIVE_SOURCE
    );
    assert_eq!(response["data"]["reversedGameCount"], 0);

    let replay = browser_post(app, &path, "01900000-0000-7000-8000-000000000911", body).await?;
    assert_eq!(replay.status(), StatusCode::OK);
    assert_eq!(body_json(replay).await?["data"], response["data"]);
    Ok(())
}

async fn seed_operator_permissions(pool: &PgPool) -> TestResult {
    sqlx::query(
        "INSERT INTO guild_config (
             guild_id, display_name, operator_role_id, recruitment_channel_id,
             log_channel_id, is_externally_public, revision, updated_by, updated_at
         ) VALUES ($1, 'Workflow Guild', $2, $3, NULL, false, 0, $4, CURRENT_TIMESTAMP)",
    )
    .bind(GUILD_A)
    .bind(OPERATOR_ROLE_ID)
    .bind(RECRUITMENT_CHANNEL_ID)
    .bind(USER_ID)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO guild_member_discord_roles (guild_id, user_id, role_id, last_synced_at)
         VALUES ($1, $2, $3, CURRENT_TIMESTAMP)",
    )
    .bind(GUILD_A)
    .bind(USER_ID)
    .bind(OPERATOR_ROLE_ID)
    .execute(pool)
    .await?;
    Ok(())
}

async fn assert_twenty_person_semifinal_game_requires_both_lineups(
    app: &Router,
    pool: &PgPool,
) -> TestResult {
    let path = format!("/api/guilds/{GUILD_A}/auction-matches/{ACTIVE_MATCH}/games");
    let response = browser_post(
        app,
        &path,
        "01900000-0000-7000-8000-000000000901",
        json!({
            "expectedRevision": 2,
            "setNumber": 1,
            "sidePreset": "DEFAULT",
            "winner": "BLUE",
            "picks": {},
            "bans": {
                "BLUE": [null, null, null, null, null],
                "RED": [null, null, null, null, null]
            },
            "patchVersion": null
        }),
    )
    .await?;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = body_json(response).await?;
    assert_eq!(
        body["error"]["code"],
        "auction_match_semifinal_lineups_incomplete"
    );
    assert_eq!(
        body["error"]["message"],
        "20인 경매 내전은 두 준결승의 라인업을 모두 확정한 뒤 세트 결과를 입력할 수 있습니다."
    );
    assert!(body["error"]["requestId"].is_string());
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*)
             FROM game_inputs
             WHERE guild_id = $1 AND auction_match_id = $2",
        )
        .bind(GUILD_A)
        .bind(id(ACTIVE_MATCH))
        .fetch_one(pool)
        .await?,
        0
    );
    Ok(())
}

async fn assert_legacy_auction_match_lock_exemption(pool: &PgPool) -> TestResult {
    let invariant_version = sqlx::query_scalar::<_, Option<i16>>(
        "SELECT lineup_lock_invariant_version
         FROM auction_matches
         WHERE guild_id = $1 AND id = $2",
    )
    .bind(GUILD_A)
    .bind(id(ACTIVE_MATCH))
    .fetch_one(pool)
    .await?;
    assert_eq!(invariant_version, Some(1));

    let current_row_violation = sqlx::query(
        "UPDATE auction_matches
         SET lineups_locked_at = NULL,
             lineups_locked_by = NULL
         WHERE guild_id = $1 AND id = $2",
    )
    .bind(GUILD_A)
    .bind(id(ACTIVE_MATCH))
    .execute(pool)
    .await;
    assert!(current_row_violation.is_err());

    // This mirrors an invalid row that predates 0018. It remains writable for
    // later remediation without creating a fictional lock actor.
    sqlx::query(
        "UPDATE auction_matches
         SET lineup_lock_invariant_version = NULL,
             lineups_locked_at = NULL,
             lineups_locked_by = NULL
         WHERE guild_id = $1 AND id = $2",
    )
    .bind(GUILD_A)
    .bind(id(ACTIVE_MATCH))
    .execute(pool)
    .await?;
    let updated = sqlx::query(
        "UPDATE auction_matches
         SET revision = revision + 1
         WHERE guild_id = $1 AND id = $2",
    )
    .bind(GUILD_A)
    .bind(id(ACTIVE_MATCH))
    .execute(pool)
    .await?;
    assert_eq!(updated.rows_affected(), 1);
    Ok(())
}

fn workflow_paths(guild_id: &str) -> Vec<String> {
    vec![
        format!("/api/guilds/{guild_id}/operations"),
        format!("/api/guilds/{guild_id}/recruitments?state=actionable"),
        format!("/api/guilds/{guild_id}/series?state=active"),
        format!("/api/guilds/{guild_id}/auction-recruitments?state=actionable"),
        format!("/api/guilds/{guild_id}/auction-tournaments?state=active"),
    ]
}

async fn seed_identity_and_guilds(pool: &PgPool) -> TestResult {
    for (id, display_name) in [
        (USER_ID, "Longcat"),
        (MEMBER_2_ID, "Member Two"),
        (MEMBER_3_ID, "Member Three"),
        (MEMBER_4_ID, "Member Four"),
        (OUTSIDER_ID, "Outsider"),
    ] {
        sqlx::query(
            "INSERT INTO users (user_id, display_name, created_at, updated_at)
             VALUES ($1, $2, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .bind(id)
        .bind(display_name)
        .execute(pool)
        .await?;

        sqlx::query(
            "INSERT INTO user_discord_identities (
                user_id, discord_id, created_at, updated_at
             ) VALUES ($1, $1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .bind(id)
        .execute(pool)
        .await?;
    }

    sqlx::query(
        "INSERT INTO guilds (
            guild_id, discord_name, status, installed_by,
            installed_at, activated_at, updated_at
         ) VALUES
           ($1, 'Workflow Guild', 'ACTIVE', $2,
            CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
           ($3, 'Hidden Guild', 'ACTIVE', $4,
            CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
    )
    .bind(GUILD_A)
    .bind(USER_ID)
    .bind(GUILD_B)
    .bind(OUTSIDER_ID)
    .execute(pool)
    .await?;

    for (guild_id, user_id, display_name) in [
        (GUILD_A, USER_ID, "Longcat"),
        (GUILD_A, MEMBER_2_ID, "Member Two"),
        (GUILD_A, MEMBER_3_ID, "Member Three"),
        (GUILD_A, MEMBER_4_ID, "Member Four"),
        (GUILD_B, OUTSIDER_ID, "Outsider"),
    ] {
        sqlx::query(
            "INSERT INTO guild_members (
                guild_id, user_id, display_name, status, first_seen_at, registered_at
             ) VALUES ($1, $2, $3, 'ACTIVE', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .bind(guild_id)
        .bind(user_id)
        .bind(display_name)
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn seed_seasons(pool: &PgPool) -> TestResult<(Uuid, Uuid)> {
    let season_a = id("01900000-0000-7000-8000-000000000001");
    let season_b = id("01900000-0000-7000-8000-000000000002");
    sqlx::query(
        "INSERT INTO seasons (
            guild_id, id, name, status, started_at, created_by, created_at
         ) VALUES
           ($1, $2, '2026 Summer', 'ACTIVE',
            '2026-07-01T00:00:00Z'::timestamptz, $3, CURRENT_TIMESTAMP),
           ($4, $5, 'Hidden Season', 'ACTIVE',
            '2026-07-01T00:00:00Z'::timestamptz, $6, CURRENT_TIMESTAMP)",
    )
    .bind(GUILD_A)
    .bind(season_a)
    .bind(USER_ID)
    .bind(GUILD_B)
    .bind(season_b)
    .bind(OUTSIDER_ID)
    .execute(pool)
    .await?;
    Ok((season_a, season_b))
}

async fn seed_sessions(pool: &PgPool) -> TestResult {
    for (user_id, token) in [
        (USER_ID, SESSION_TOKEN),
        (OUTSIDER_ID, OUTSIDER_SESSION_TOKEN),
    ] {
        sqlx::query(
            "INSERT INTO user_sessions (
                id, user_id, token_hash, csrf_secret_hash,
                created_at, last_seen_at, expires_at, revoked_at
             ) VALUES (
                $1, $2, $3, $4,
                CURRENT_TIMESTAMP, CURRENT_TIMESTAMP,
                CURRENT_TIMESTAMP + INTERVAL '1 hour', NULL
             )",
        )
        .bind(Uuid::now_v7())
        .bind(user_id)
        .bind(AuthService::hash_secret(token))
        .bind(AuthService::hash_secret(&format!("{token}-csrf")))
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn seed_ordinary_workflow(pool: &PgPool, season_a: Uuid, season_b: Uuid) -> TestResult {
    sqlx::query(
        "INSERT INTO recruitments (
            guild_id, id, season_id, display_number, title, target_count,
            status, revision, created_by, created_at, updated_by, updated_at,
            closed_at, converted_at
         ) VALUES
           ($1, $2, $3, 214, '금요 일반 모집', 10,
            'CLOSED', 4, $4, '2026-07-10T10:00:00Z', $4, '2026-07-10T12:00:00Z',
            '2026-07-10T12:00:00Z', NULL),
           ($1, $5, $3, 215, '진행 시리즈 원본', 10,
            'CONVERTED', 2, $4, '2026-07-10T08:00:00Z', $4, '2026-07-10T10:00:00Z',
            '2026-07-10T09:00:00Z', '2026-07-10T10:00:00Z'),
           ($1, $6, $3, 216, '완료 시리즈 원본', 10,
            'CONVERTED', 2, $4, '2026-07-09T08:00:00Z', $4, '2026-07-09T10:00:00Z',
            '2026-07-09T09:00:00Z', '2026-07-09T10:00:00Z'),
           ($7, $8, $9, 1, '다른 길드 비공개 모집', 10,
            'CLOSED', 0, $10, '2026-07-10T13:00:00Z', $10, '2026-07-10T13:00:00Z',
            '2026-07-10T13:00:00Z', NULL)",
    )
    .bind(GUILD_A)
    .bind(id(ACTIONABLE_RECRUITMENT))
    .bind(season_a)
    .bind(USER_ID)
    .bind(id(ACTIVE_SOURCE_RECRUITMENT))
    .bind(id(COMPLETED_SOURCE_RECRUITMENT))
    .bind(GUILD_B)
    .bind(id("01900000-0000-7000-8000-000000000104"))
    .bind(season_b)
    .bind(OUTSIDER_ID)
    .execute(pool)
    .await?;

    for (user_id, status, withdrawn_at) in [
        (USER_ID, "JOINED", None),
        (MEMBER_2_ID, "JOINED", None),
        (MEMBER_3_ID, "WITHDRAWN", Some("2026-07-10T11:30:00Z")),
    ] {
        sqlx::query(
            "INSERT INTO recruitment_participants (
                guild_id, recruitment_id, user_id, status, joined_at, withdrawn_at
             ) VALUES ($1, $2, $3, $4, '2026-07-10T10:30:00Z', $5::timestamptz)",
        )
        .bind(GUILD_A)
        .bind(id(ACTIONABLE_RECRUITMENT))
        .bind(user_id)
        .bind(status)
        .bind(withdrawn_at)
        .execute(pool)
        .await?;
    }

    sqlx::query(
        "INSERT INTO recruitment_entry_drafts (
            guild_id, recruitment_id, revision, updated_by, updated_at
         ) VALUES ($1, $2, 3, $3, '2026-07-10T11:50:00Z')",
    )
    .bind(GUILD_A)
    .bind(id(ACTIONABLE_RECRUITMENT))
    .bind(USER_ID)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO recruitment_entry_assignments (
            guild_id, recruitment_id, user_id, team, lane
         ) VALUES ($1, $2, $3, 'TEAM_1', 'TOP')",
    )
    .bind(GUILD_A)
    .bind(id(ACTIONABLE_RECRUITMENT))
    .bind(USER_ID)
    .execute(pool)
    .await?;

    sqlx::query(
        "INSERT INTO series (
            guild_id, id, season_id, source_recruitment_id, display_number,
            title, best_of, status, winning_team, terminal_reason, revision,
            started_at, ended_at, created_by, updated_by, updated_at
         ) VALUES
           ($1, $2, $3, $4, 106,
            '진행 중 BO3', 3, 'IN_PROGRESS', NULL, NULL, 5,
            '2026-07-10T10:00:00Z', NULL, $5, $5, '2026-07-10T12:10:00Z'),
           ($1, $6, $3, $7, 105,
            '완료 BO3', 3, 'COMPLETED', 'TEAM_1', 'SCORE_REACHED', 8,
            '2026-07-10T09:00:00Z', '2026-07-10T11:00:00Z', $5, $5,
            '2026-07-10T11:00:00Z')",
    )
    .bind(GUILD_A)
    .bind(id(ACTIVE_SERIES))
    .bind(season_a)
    .bind(id(ACTIVE_SOURCE_RECRUITMENT))
    .bind(USER_ID)
    .bind(id(COMPLETED_SERIES))
    .bind(id(COMPLETED_SOURCE_RECRUITMENT))
    .execute(pool)
    .await?;

    seed_series_game(
        pool,
        season_a,
        ACTIVE_SERIES,
        "01900000-0000-7000-8000-000000000301",
        "01900000-0000-7000-8000-000000000401",
        1,
        "DEFAULT",
        "RED",
        "CONFIRMED",
    )
    .await?;
    for (input_id, game_id, number, preset, winner, status) in [
        (
            "01900000-0000-7000-8000-000000000302",
            "01900000-0000-7000-8000-000000000402",
            1,
            "DEFAULT",
            "BLUE",
            "CONFIRMED",
        ),
        (
            "01900000-0000-7000-8000-000000000303",
            "01900000-0000-7000-8000-000000000403",
            2,
            "SWAPPED",
            "RED",
            "CONFIRMED",
        ),
        (
            "01900000-0000-7000-8000-000000000304",
            "01900000-0000-7000-8000-000000000404",
            3,
            "SWAPPED",
            "BLUE",
            "CONFIRMED",
        ),
        (
            "01900000-0000-7000-8000-000000000305",
            "01900000-0000-7000-8000-000000000405",
            3,
            "DEFAULT",
            "RED",
            "VOIDED",
        ),
    ] {
        seed_series_game(
            pool,
            season_a,
            COMPLETED_SERIES,
            input_id,
            game_id,
            number,
            preset,
            winner,
            status,
        )
        .await?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn seed_series_game(
    pool: &PgPool,
    season_id: Uuid,
    series_id: &str,
    input_id: &str,
    game_id: &str,
    game_number: i16,
    side_preset: &str,
    winning_side: &str,
    game_status: &str,
) -> TestResult {
    let input_status = if game_status == "VOIDED" {
        "SUPERSEDED"
    } else {
        "SUBMITTED"
    };
    sqlx::query(
        "INSERT INTO game_inputs (
            guild_id, id, series_id, set_number, side_preset, winning_side,
            status, updated_by, updated_at
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, '2026-07-10T10:30:00Z')",
    )
    .bind(GUILD_A)
    .bind(id(input_id))
    .bind(id(series_id))
    .bind(game_number)
    .bind(side_preset)
    .bind(winning_side)
    .bind(input_status)
    .bind(USER_ID)
    .execute(pool)
    .await?;

    sqlx::query(
        "INSERT INTO games (
            guild_id, id, season_id, series_id, source_input_id, game_number,
            side_preset, winning_side, status, mmr_status, played_at,
            confirmed_by, confirmed_by_type, confirmed_at,
            voided_by, voided_by_type, voided_at, void_reason
         ) VALUES (
            $1, $2, $3, $4, $5, $6,
            $7, $8, $9,
            CASE WHEN $9 = 'VOIDED' THEN 'LEGACY_NOT_APPLIED' ELSE 'APPLIED' END,
            '2026-07-10T10:30:00Z', $10, 'USER', '2026-07-10T10:31:00Z',
            CASE WHEN $9 = 'VOIDED' THEN $10 ELSE NULL END,
            CASE WHEN $9 = 'VOIDED' THEN 'USER' ELSE NULL END,
            CASE WHEN $9 = 'VOIDED' THEN '2026-07-10T10:40:00Z'::timestamptz ELSE NULL END,
            CASE WHEN $9 = 'VOIDED' THEN 'integration fixture void' ELSE NULL END
         )",
    )
    .bind(GUILD_A)
    .bind(id(game_id))
    .bind(season_id)
    .bind(id(series_id))
    .bind(id(input_id))
    .bind(game_number)
    .bind(side_preset)
    .bind(winning_side)
    .bind(game_status)
    .bind(USER_ID)
    .execute(pool)
    .await?;
    Ok(())
}

async fn seed_auction_workflow(pool: &PgPool, season_id: Uuid) -> TestResult {
    sqlx::query(
        "INSERT INTO auction_recruitments (
            guild_id, id, season_id, display_number, title, format, target_count,
            semifinal_best_of, status, revision, created_by, created_at, updated_by, updated_at,
            closed_at, converted_at
         ) VALUES
           ($1, $2, $3, 110, '10인 경매 모집', 'TEN_PERSON', 10,
            NULL, 'CLOSED', 2, $4, '2026-07-10T08:00:00Z', $4, '2026-07-10T12:20:00Z',
            '2026-07-10T12:20:00Z', NULL),
           ($1, $5, $3, 120, '20인 경매 모집', 'TWENTY_PERSON', 20,
            3, 'CLOSED', 3, $4, '2026-07-10T08:00:00Z', $4, '2026-07-10T12:25:00Z',
            '2026-07-10T12:25:00Z', NULL),
           ($1, $6, $3, 121, '진행 경매 원본', 'TWENTY_PERSON', 20,
            3, 'CONVERTED', 4, $4, '2026-07-10T07:00:00Z', $4, '2026-07-10T10:00:00Z',
            '2026-07-10T09:00:00Z', '2026-07-10T10:00:00Z'),
           ($1, $7, $3, 111, '완료 경매 원본', 'TEN_PERSON', 10,
            NULL, 'CONVERTED', 4, $4, '2026-07-09T07:00:00Z', $4, '2026-07-09T10:00:00Z',
            '2026-07-09T09:00:00Z', '2026-07-09T10:00:00Z')",
    )
    .bind(GUILD_A)
    .bind(id(AUCTION_ACTIONABLE_10))
    .bind(season_id)
    .bind(USER_ID)
    .bind(id(AUCTION_ACTIONABLE_20))
    .bind(id(AUCTION_ACTIVE_SOURCE))
    .bind(id(AUCTION_COMPLETED_SOURCE))
    .execute(pool)
    .await?;

    sqlx::query(
        "INSERT INTO auction_recruitments (
            guild_id, id, season_id, display_number, title, format, target_count,
            status, revision, created_by, created_at, updated_by, updated_at, cancelled_at
         ) VALUES (
            $1, $2, $3, 116, '취소 경매 원본', 'TEN_PERSON', 10,
            'CANCELLED', 5, $4, '2026-07-08T07:00:00Z', $4, '2026-07-08T10:00:00Z',
            '2026-07-08T10:00:00Z'
         )",
    )
    .bind(GUILD_A)
    .bind(id(AUCTION_CANCELLED_SOURCE))
    .bind(season_id)
    .bind(USER_ID)
    .execute(pool)
    .await?;

    for (recruitment_id, user_id, status) in [
        (AUCTION_ACTIONABLE_10, USER_ID, "JOINED"),
        (AUCTION_ACTIONABLE_10, MEMBER_2_ID, "JOINED"),
        (AUCTION_ACTIONABLE_10, MEMBER_3_ID, "WITHDRAWN"),
        (AUCTION_ACTIONABLE_20, USER_ID, "JOINED"),
        (AUCTION_ACTIONABLE_20, MEMBER_2_ID, "JOINED"),
        (AUCTION_ACTIONABLE_20, MEMBER_3_ID, "JOINED"),
    ] {
        sqlx::query(
            "INSERT INTO auction_recruitment_participants (
                guild_id, recruitment_id, user_id, status, snapshot_origin,
                joined_at, withdrawn_at
             ) VALUES (
                $1, $2, $3, $4, 'UNAVAILABLE', '2026-07-10T09:00:00Z',
                CASE WHEN $4 = 'WITHDRAWN' THEN '2026-07-10T10:00:00Z'::timestamptz ELSE NULL END
             )",
        )
        .bind(GUILD_A)
        .bind(id(recruitment_id))
        .bind(user_id)
        .bind(status)
        .execute(pool)
        .await?;
    }

    sqlx::query(
        "INSERT INTO auction_tournaments (
            guild_id, id, season_id, source_recruitment_id, display_number,
            title, format, semifinal_best_of, status, revision, started_at, created_by, updated_by, updated_at
         ) VALUES
           ($1, $2, $3, $4, 118,
            '20인 진행 경매', 'TWENTY_PERSON', 3, 'IN_GAME', 7,
            '2026-07-10T10:00:00Z', $5, $5, '2026-07-10T12:30:00Z'),
           ($1, $6, $3, $7, 117,
            '10인 완료 경매', 'TEN_PERSON', NULL, 'BIDDING', 8,
            '2026-07-09T10:00:00Z', $5, $5, '2026-07-09T18:00:00Z')",
    )
    .bind(GUILD_A)
    .bind(id(ACTIVE_TOURNAMENT))
    .bind(season_id)
    .bind(id(AUCTION_ACTIVE_SOURCE))
    .bind(USER_ID)
    .bind(id(COMPLETED_TOURNAMENT))
    .bind(id(AUCTION_COMPLETED_SOURCE))
    .execute(pool)
    .await?;

    sqlx::query(
        "INSERT INTO auction_tournaments (
            guild_id, id, season_id, source_recruitment_id, display_number,
            title, format, status, revision, started_at, ended_at, created_by, updated_by, updated_at
         ) VALUES (
            $1, $2, $3, $4, 116,
            '매치 없이 취소된 10인 경매', 'TEN_PERSON', 'CANCELLED', 6,
            '2026-07-08T09:00:00Z', '2026-07-08T10:00:00Z', $5, $5, '2026-07-08T10:00:00Z'
         )",
    )
    .bind(GUILD_A)
    .bind(id(CANCELLED_TOURNAMENT))
    .bind(season_id)
    .bind(id(AUCTION_CANCELLED_SOURCE))
    .bind(USER_ID)
    .execute(pool)
    .await?;

    for (tournament_id, team_id, team_index, name) in [
        (ACTIVE_TOURNAMENT, ACTIVE_TEAM_1, 1_i16, "진행 1팀"),
        (ACTIVE_TOURNAMENT, ACTIVE_TEAM_2, 2_i16, "진행 2팀"),
        (ACTIVE_TOURNAMENT, ACTIVE_TEAM_3, 3_i16, "진행 3팀"),
        (ACTIVE_TOURNAMENT, ACTIVE_TEAM_4, 4_i16, "진행 4팀"),
        (COMPLETED_TOURNAMENT, COMPLETED_TEAM_1, 1_i16, "완료 1팀"),
        (COMPLETED_TOURNAMENT, COMPLETED_TEAM_2, 2_i16, "완료 2팀"),
        (CANCELLED_TOURNAMENT, CANCELLED_TEAM_1, 1_i16, "취소 1팀"),
        (CANCELLED_TOURNAMENT, CANCELLED_TEAM_2, 2_i16, "취소 2팀"),
    ] {
        sqlx::query(
            "INSERT INTO auction_teams (
                guild_id, tournament_id, id, team_index, team_name,
                initial_points, current_points, point_revision, created_at
             ) VALUES ($1, $2, $3, $4, $5, 1000, 1000, 0, CURRENT_TIMESTAMP)",
        )
        .bind(GUILD_A)
        .bind(id(tournament_id))
        .bind(id(team_id))
        .bind(team_index)
        .bind(name)
        .execute(pool)
        .await?;
    }

    sqlx::query(
        "UPDATE auction_tournaments
         SET status = 'COMPLETED', champion_team_id = $1,
             ended_at = '2026-07-09T18:00:00Z', updated_at = '2026-07-09T18:00:00Z'
         WHERE guild_id = $2 AND id = $3",
    )
    .bind(id(COMPLETED_TEAM_1))
    .bind(GUILD_A)
    .bind(id(COMPLETED_TOURNAMENT))
    .execute(pool)
    .await?;

    for (tournament_id, team_id, member_id, user_id) in [
        (
            ACTIVE_TOURNAMENT,
            ACTIVE_TEAM_1,
            "01900000-0000-7000-8000-000000000711",
            USER_ID,
        ),
        (
            ACTIVE_TOURNAMENT,
            ACTIVE_TEAM_2,
            "01900000-0000-7000-8000-000000000712",
            MEMBER_2_ID,
        ),
        (
            ACTIVE_TOURNAMENT,
            ACTIVE_TEAM_3,
            "01900000-0000-7000-8000-000000000713",
            MEMBER_3_ID,
        ),
        (
            ACTIVE_TOURNAMENT,
            ACTIVE_TEAM_4,
            "01900000-0000-7000-8000-000000000714",
            MEMBER_4_ID,
        ),
        (
            COMPLETED_TOURNAMENT,
            COMPLETED_TEAM_1,
            "01900000-0000-7000-8000-000000000715",
            USER_ID,
        ),
        (
            COMPLETED_TOURNAMENT,
            COMPLETED_TEAM_2,
            "01900000-0000-7000-8000-000000000716",
            MEMBER_2_ID,
        ),
    ] {
        sqlx::query(
            "INSERT INTO auction_team_members (
                guild_id, tournament_id, team_id, id, user_id, roster_slot,
                acquired_via, snapshot_origin, joined_at
             ) VALUES ($1, $2, $3, $4, $5, 1, 'CAPTAIN', 'UNAVAILABLE', CURRENT_TIMESTAMP)",
        )
        .bind(GUILD_A)
        .bind(id(tournament_id))
        .bind(id(team_id))
        .bind(id(member_id))
        .bind(user_id)
        .execute(pool)
        .await?;
    }

    sqlx::query(
        "INSERT INTO auction_matches (
            guild_id, tournament_id, id, season_id, match_key, best_of,
            left_team_id, right_team_id, status, revision,
            lineups_locked_at, lineups_locked_by, updated_by, updated_at
         ) VALUES (
            $1, $2, $3, $4, 'SEMIFINAL_A', 3,
            $5, $6, 'READY', 2,
            '2026-07-10T12:20:00Z', $7, $7, '2026-07-10T12:30:00Z'
         )",
    )
    .bind(GUILD_A)
    .bind(id(ACTIVE_TOURNAMENT))
    .bind(id(ACTIVE_MATCH))
    .bind(season_id)
    .bind(id(ACTIVE_TEAM_1))
    .bind(id(ACTIVE_TEAM_2))
    .bind(USER_ID)
    .execute(pool)
    .await?;
    Ok(())
}

fn id(value: &str) -> Uuid {
    Uuid::parse_str(value).expect("fixture UUID must be valid")
}

async fn get(app: &Router, uri: &str, cookie: Option<&str>) -> TestResult<Response<Body>> {
    let mut request = Request::get(uri);
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    Ok(app.clone().oneshot(request.body(Body::empty())?).await?)
}

async fn browser_post(
    app: &Router,
    path: &str,
    idempotency_key: &str,
    body: Value,
) -> TestResult<Response<Body>> {
    let csrf = format!("{SESSION_TOKEN}-csrf");
    let request = Request::post(path)
        .header(
            header::COOKIE,
            format!("monkeybot_session={SESSION_TOKEN}; monkeybot_csrf={csrf}"),
        )
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, PUBLIC_ORIGIN)
        .header("x-csrf-token", &csrf)
        .header("idempotency-key", idempotency_key)
        .body(Body::from(serde_json::to_vec(&body)?))?;
    Ok(app.clone().oneshot(request).await?)
}

async fn bot_get(app: &Router, uri: &str) -> TestResult<Response<Body>> {
    let request = Request::get(uri)
        .header(header::AUTHORIZATION, format!("Bearer {BOT_TOKEN}"))
        .body(Body::empty())?;
    Ok(app.clone().oneshot(request).await?)
}

async fn assert_error(response: Response<Body>, status: StatusCode, code: &str) -> TestResult {
    assert_eq!(response.status(), status);
    assert_eq!(body_json(response).await?["error"]["code"], code);
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
