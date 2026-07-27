use std::{collections::HashMap, error::Error, sync::Arc, time::Duration};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, Response, StatusCode, header},
};
use monkeybot_api::{
    build_router,
    config::Config,
    infrastructure::postgres::{MIGRATOR, PostgresReadinessProbe},
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
const POSTGRES_PASSWORD: &str = "monkeybot-bot-workflow-password";
const DATABASE: &str = "monkeybot_bot_workflow";

const BOT_TOKEN: &str = "bot-service-token-that-is-at-least-32-characters";
const WRONG_BOT_TOKEN: &str = "wrong-service-token-that-is-at-least-32-characters";
const PUBLIC_ORIGIN: &str = "http://localhost:5173";
const GUILD_A: &str = "200000000000000001";
const GUILD_B: &str = "200000000000000002";
const UNKNOWN_GUILD: &str = "200000000000000003";
const OPERATOR_ROLE_A: &str = "300000000000000001";
const OPERATOR_ROLE_B: &str = "300000000000000002";
const CHANNEL_ID: &str = "400000000000000001";
const ENTRY_CHANNEL_ID: &str = "400000000000000002";
const RESULT_CHANNEL_ID: &str = "400000000000000003";
const LOG_CHANNEL_ID: &str = "400000000000000004";
const MESSAGE_ID: &str = "500000000000000001";
const OPERATOR_ID: &str = "100000000000000001";
const OTHER_OPERATOR_ID: &str = "100000000000000099";
const MEMBER_IDS: [&str; 11] = [
    "100000000000000001",
    "100000000000000002",
    "100000000000000003",
    "100000000000000004",
    "100000000000000005",
    "100000000000000006",
    "100000000000000007",
    "100000000000000008",
    "100000000000000009",
    "100000000000000010",
    "100000000000000011",
];
const LANES: [&str; 5] = ["TOP", "JUNGLE", "MID", "BOTTOM", "SUPPORT"];

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bot_workflow_endpoints_are_idempotent_authorized_and_tenant_scoped() -> TestResult {
    tokio::time::timeout(Duration::from_secs(180), run_bot_workflow())
        .await
        .map_err(|_| "Bot workflow integration test exceeded 180 seconds")??;
    Ok(())
}

async fn run_bot_workflow() -> TestResult {
    let server = TestPostgres::start().await?;
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&server.database_url())
        .await?;
    MIGRATOR.run(&pool).await?;

    let config = test_config(server.database_url())?;
    let state = AppState::with_postgres(
        config,
        pool.clone(),
        Arc::new(PostgresReadinessProbe::new(
            pool.clone(),
            Duration::from_secs(1),
        )),
    );
    let app = build_router(state);

    let operator = actor(OPERATOR_ID, "Guild A Operator", &[OPERATOR_ROLE_A]);
    let setup_a = json!({
        "actor": operator,
        "guildName": "MonkeyBot Test Guild",
        "guildIconHash": "guild-a-icon",
        "operatorRoleId": OPERATOR_ROLE_A,
        "recruitmentChannelId": CHANNEL_ID,
        "entryChannelId": ENTRY_CHANNEL_ID,
        "resultChannelId": RESULT_CHANNEL_ID,
        "logChannelId": LOG_CHANNEL_ID,
        "seasonName": "2026 Summer"
    });

    assert_error(
        bot_request(
            &app,
            Method::POST,
            &format!("/internal/guilds/{GUILD_A}/setup"),
            WRONG_BOT_TOKEN,
            "invalid-bearer",
            &setup_a,
        )
        .await?,
        StatusCode::UNAUTHORIZED,
        "unauthorized",
    )
    .await?;

    let setup_response = bot_request(
        &app,
        Method::POST,
        &format!("/internal/guilds/{GUILD_A}/setup"),
        BOT_TOKEN,
        "setup-guild-a",
        &setup_a,
    )
    .await?;
    assert_eq!(setup_response.status(), StatusCode::OK);
    let setup_body = body_json(setup_response).await?;
    assert_eq!(setup_body["data"]["guild"]["id"], GUILD_A);
    assert_eq!(
        setup_body["data"]["guild"]["operatorRoleId"],
        OPERATOR_ROLE_A
    );
    assert_eq!(
        setup_body["data"]["guild"]["currentSeason"]["name"],
        "2026 Summer"
    );
    assert_eq!(
        setup_body["data"]["guild"]["dashboardHref"],
        format!("/g/{GUILD_A}")
    );

    let setup_replay = bot_request(
        &app,
        Method::POST,
        &format!("/internal/guilds/{GUILD_A}/setup"),
        BOT_TOKEN,
        "setup-guild-a",
        &setup_a,
    )
    .await?;
    assert_eq!(setup_replay.status(), StatusCode::OK);
    assert_eq!(body_json(setup_replay).await?["data"], setup_body["data"]);

    let mut repeated_setup = setup_a.clone();
    repeated_setup["guildName"] = json!("Must Not Replace Guild");
    repeated_setup["guildIconHash"] = json!("must-not-replace-icon");
    repeated_setup["operatorRoleId"] = json!(OPERATOR_ROLE_B);
    repeated_setup["seasonName"] = json!("Must Not Replace Season");
    repeated_setup["actor"]["globalDisplayName"] = json!("Must Not Replace User");
    repeated_setup["actor"]["displayName"] = json!("Must Not Replace Member");
    let setup_refresh = bot_request(
        &app,
        Method::POST,
        &format!("/internal/guilds/{GUILD_A}/setup"),
        BOT_TOKEN,
        "setup-guild-a-refresh",
        &repeated_setup,
    )
    .await?;
    assert_eq!(setup_refresh.status(), StatusCode::CONFLICT);
    let setup_refresh_error = body_json(setup_refresh).await?;
    assert_eq!(
        setup_refresh_error["error"]["code"],
        "guild_already_configured"
    );
    assert_eq!(
        setup_refresh_error["error"]["message"],
        "이미 초기 설정이 완료된 길드입니다. 길드 관리 화면에서 설정을 변경해 주세요."
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT revision FROM guild_config WHERE guild_id = $1")
            .bind(GUILD_A)
            .fetch_one(&pool)
            .await?,
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM admin_audit_log WHERE guild_id = $1 AND action = 'GUILD_SETUP'"
        )
        .bind(GUILD_A)
        .fetch_one(&pool)
        .await?,
        1
    );
    assert_eq!(
        sqlx::query_as::<_, (String, Option<String>)>(
            "SELECT discord_name, icon_hash FROM guilds WHERE guild_id = $1"
        )
        .bind(GUILD_A)
        .fetch_one(&pool)
        .await?,
        (
            "MonkeyBot Test Guild".to_owned(),
            Some("guild-a-icon".to_owned())
        )
    );
    assert_eq!(
        sqlx::query_as::<
            _,
            (
                String,
                String,
                String,
                Option<String>,
                Option<String>,
                Option<String>,
            ),
        >(
            "SELECT display_name, operator_role_id, recruitment_channel_id, entry_channel_id,
                    result_channel_id, log_channel_id
             FROM guild_config WHERE guild_id = $1"
        )
        .bind(GUILD_A)
        .fetch_one(&pool)
        .await?,
        (
            "MonkeyBot Test Guild".to_owned(),
            OPERATOR_ROLE_A.to_owned(),
            CHANNEL_ID.to_owned(),
            Some(ENTRY_CHANNEL_ID.to_owned()),
            Some(RESULT_CHANNEL_ID.to_owned()),
            Some(LOG_CHANNEL_ID.to_owned()),
        )
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT name FROM seasons WHERE guild_id = $1")
            .bind(GUILD_A)
            .fetch_one(&pool)
            .await?,
        "2026 Summer"
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT display_name FROM users WHERE user_id = $1")
            .bind(OPERATOR_ID)
            .fetch_one(&pool)
            .await?,
        "Guild A Operator"
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT display_name FROM guild_members WHERE guild_id = $1 AND user_id = $2"
        )
        .bind(GUILD_A)
        .bind(OPERATOR_ID)
        .fetch_one(&pool)
        .await?,
        "Guild A Operator"
    );

    let mut conflicting_setup = setup_a.clone();
    conflicting_setup["guildName"] = json!("Conflicting Guild Name");
    assert_error(
        bot_request(
            &app,
            Method::POST,
            &format!("/internal/guilds/{GUILD_A}/setup"),
            BOT_TOKEN,
            "setup-guild-a",
            &conflicting_setup,
        )
        .await?,
        StatusCode::CONFLICT,
        "idempotency_conflict",
    )
    .await?;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM guilds WHERE guild_id = $1")
            .bind(GUILD_A)
            .fetch_one(&pool)
            .await?,
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM seasons WHERE guild_id = $1")
            .bind(GUILD_A)
            .fetch_one(&pool)
            .await?,
        1
    );

    let create_request = json!({
        "actor": actor(OPERATOR_ID, "Guild A Operator", &[OPERATOR_ROLE_A]),
        "bestOf": 5,
        "hardFearless": true,
        "expectedStartAt": "2026-07-26T11:00:00Z"
    });
    let invalid_best_of = bot_request(
        &app,
        Method::POST,
        &format!("/api/guilds/{GUILD_A}/recruitments"),
        BOT_TOKEN,
        "create-recruitment-invalid-best-of",
        &json!({
            "actor": actor(OPERATOR_ID, "Guild A Operator", &[OPERATOR_ROLE_A]),
            "bestOf": 2,
            "hardFearless": true,
            "expectedStartAt": "2026-07-26T11:00:00Z"
        }),
    )
    .await?;
    assert_eq!(invalid_best_of.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let invalid_best_of = body_json(invalid_best_of).await?;
    assert_eq!(
        invalid_best_of["error"]["code"],
        "invalid_recruitment_best_of"
    );
    assert_eq!(
        invalid_best_of["error"]["message"],
        "시리즈 형식은 BO1, BO3 또는 BO5만 선택할 수 있습니다."
    );
    assert_eq!(
        invalid_best_of["error"]["details"],
        json!({
            "field": "bestOf",
            "allowedValues": [1, 3, 5],
            "receivedValue": 2
        })
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM recruitments WHERE guild_id = $1")
            .bind(GUILD_A)
            .fetch_one(&pool)
            .await?,
        0
    );
    let create_response = bot_request(
        &app,
        Method::POST,
        &format!("/api/guilds/{GUILD_A}/recruitments"),
        BOT_TOKEN,
        "create-recruitment-a",
        &create_request,
    )
    .await?;
    assert_eq!(create_response.status(), StatusCode::CREATED);
    let created = body_json(create_response).await?;
    let recruitment_id = created["data"]["recruitment"]["id"]
        .as_str()
        .ok_or("create response did not contain a recruitment ID")?
        .to_owned();
    assert_eq!(created["data"]["recruitment"]["status"], "OPEN");
    assert_eq!(created["data"]["recruitment"]["targetCount"], 10);
    assert_eq!(created["data"]["recruitment"]["bestOf"], 5);
    assert_eq!(created["data"]["recruitment"]["hardFearless"], true);
    assert_eq!(
        created["data"]["recruitment"]["title"],
        "일반 내전 모집 #1 · 예상 시작 2026-07-26 20:00 (KST)"
    );
    assert!(created["data"]["recruitment"]["updatedAt"].is_string());
    assert_eq!(created["data"]["participants"], json!([]));

    let create_replay = bot_request(
        &app,
        Method::POST,
        &format!("/api/guilds/{GUILD_A}/recruitments"),
        BOT_TOKEN,
        "create-recruitment-a",
        &create_request,
    )
    .await?;
    assert_eq!(create_replay.status(), StatusCode::CREATED);
    assert_eq!(
        body_json(create_replay).await?["data"]["recruitment"]["id"],
        recruitment_id
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM recruitments WHERE guild_id = $1")
            .bind(GUILD_A)
            .fetch_one(&pool)
            .await?,
        1
    );

    let non_operator = actor(MEMBER_IDS[1], "Non Operator", &[]);
    assert_error(
        bot_request(
            &app,
            Method::POST,
            &format!("/api/guilds/{UNKNOWN_GUILD}/recruitments"),
            BOT_TOKEN,
            "unknown-guild-create",
            &create_request,
        )
        .await?,
        StatusCode::NOT_FOUND,
        "not_found",
    )
    .await?;

    let non_operator_created = bot_request(
        &app,
        Method::POST,
        &format!("/api/guilds/{GUILD_A}/recruitments"),
        BOT_TOKEN,
        "non-operator-create",
        &json!({
            "actor": non_operator,
            "expectedStartAt": "2026-07-26T11:00:00Z"
        }),
    )
    .await?;
    assert_eq!(non_operator_created.status(), StatusCode::CREATED);
    let non_operator_created = body_json(non_operator_created).await?;
    assert_eq!(non_operator_created["data"]["recruitment"]["bestOf"], 3);
    assert_eq!(
        non_operator_created["data"]["recruitment"]["hardFearless"],
        true
    );
    let non_operator_recruitment_id = non_operator_created["data"]["recruitment"]["id"]
        .as_str()
        .ok_or("non-operator create response did not contain a recruitment ID")?;

    let non_operator_bound = bot_request(
        &app,
        Method::PUT,
        &format!("/api/guilds/{GUILD_A}/recruitments/{non_operator_recruitment_id}/message"),
        BOT_TOKEN,
        "non-operator-bind-own-message",
        &json!({
            "actor": actor(MEMBER_IDS[1], "Non Operator", &[]),
            "channelId": "400000000000000002",
            "messageId": "500000000000000002"
        }),
    )
    .await?;
    assert_eq!(non_operator_bound.status(), StatusCode::OK);
    assert_eq!(
        body_json(non_operator_bound).await?["data"]["recruitment"]["messageId"],
        "500000000000000002"
    );

    let bind_response = bot_request(
        &app,
        Method::PUT,
        &format!("/api/guilds/{GUILD_A}/recruitments/{recruitment_id}/message"),
        BOT_TOKEN,
        "bind-recruitment-message",
        &json!({
            "actor": actor(OPERATOR_ID, "Guild A Operator", &[OPERATOR_ROLE_A]),
            "channelId": CHANNEL_ID,
            "messageId": MESSAGE_ID
        }),
    )
    .await?;
    assert_eq!(bind_response.status(), StatusCode::OK);
    let bound = body_json(bind_response).await?;
    assert_eq!(bound["data"]["recruitment"]["channelId"], CHANNEL_ID);
    assert_eq!(bound["data"]["recruitment"]["messageId"], MESSAGE_ID);

    for (index, user_id) in MEMBER_IDS.iter().take(10).enumerate() {
        let role_ids = if *user_id == OPERATOR_ID {
            &[OPERATOR_ROLE_A][..]
        } else {
            &[][..]
        };
        let member = actor(user_id, &format!("Member {}", index + 1), role_ids);
        let join_response = bot_request(
            &app,
            Method::POST,
            &format!("/api/guilds/{GUILD_A}/recruitments/{recruitment_id}/members"),
            BOT_TOKEN,
            &format!("join-member-{index}"),
            &json!({ "actor": member, "member": member }),
        )
        .await?;
        assert_eq!(join_response.status(), StatusCode::OK);
        assert_eq!(
            body_json(join_response).await?["data"]["participants"]
                .as_array()
                .map(Vec::len),
            Some(index + 1)
        );

        let lane = LANES[index % LANES.len()];
        let lanes_response = bot_request(
            &app,
            Method::PUT,
            &format!(
                "/api/guilds/{GUILD_A}/recruitments/{recruitment_id}/members/{user_id}/preferred-lanes"
            ),
            BOT_TOKEN,
            &format!("lanes-member-{index}"),
            &json!({ "actor": member, "preferredLanes": [lane] }),
        )
        .await?;
        assert_eq!(lanes_response.status(), StatusCode::OK);
        let lanes_body = body_json(lanes_response).await?;
        let participant = find_participant(&lanes_body, user_id)?;
        assert_eq!(participant["preferredLanes"], json!([lane]));
    }

    let overflow_member = actor(MEMBER_IDS[10], "Capacity Overflow", &[]);
    assert_error(
        bot_request(
            &app,
            Method::POST,
            &format!("/api/guilds/{GUILD_A}/recruitments/{recruitment_id}/members"),
            BOT_TOKEN,
            "join-over-capacity",
            &json!({ "actor": overflow_member, "member": overflow_member }),
        )
        .await?,
        StatusCode::UNPROCESSABLE_ENTITY,
        "recruitment_full",
    )
    .await?;

    let leaving_user = MEMBER_IDS[9];
    let leave_response = bot_request(
        &app,
        Method::DELETE,
        &format!("/api/guilds/{GUILD_A}/recruitments/{recruitment_id}/members/{leaving_user}"),
        BOT_TOKEN,
        "leave-member-9",
        &json!({ "actor": actor(leaving_user, "Member 10", &[]) }),
    )
    .await?;
    assert_eq!(leave_response.status(), StatusCode::OK);
    let after_leave = body_json(leave_response).await?;
    assert_eq!(
        after_leave["data"]["participants"].as_array().map(Vec::len),
        Some(9)
    );
    assert!(find_participant(&after_leave, leaving_user).is_err());

    let replacement_join = bot_request(
        &app,
        Method::POST,
        &format!("/api/guilds/{GUILD_A}/recruitments/{recruitment_id}/members"),
        BOT_TOKEN,
        "join-replacement",
        &json!({ "actor": overflow_member, "member": overflow_member }),
    )
    .await?;
    assert_eq!(replacement_join.status(), StatusCode::OK);
    assert_eq!(
        body_json(replacement_join).await?["data"]["participants"]
            .as_array()
            .map(Vec::len),
        Some(10)
    );
    let replacement_lanes = bot_request(
        &app,
        Method::PUT,
        &format!(
            "/api/guilds/{GUILD_A}/recruitments/{recruitment_id}/members/{}/preferred-lanes",
            MEMBER_IDS[10]
        ),
        BOT_TOKEN,
        "lanes-replacement",
        &json!({ "actor": overflow_member, "preferredLanes": ["SUPPORT"] }),
    )
    .await?;
    assert_eq!(replacement_lanes.status(), StatusCode::OK);

    assert_error(
        bot_request(
            &app,
            Method::POST,
            &format!("/api/guilds/{GUILD_A}/recruitments/{recruitment_id}/close"),
            BOT_TOKEN,
            "non-operator-close",
            &json!({ "actor": actor(MEMBER_IDS[1], "Member 2", &[]) }),
        )
        .await?,
        StatusCode::FORBIDDEN,
        "operator_required",
    )
    .await?;

    let close_response = bot_request(
        &app,
        Method::POST,
        &format!("/api/guilds/{GUILD_A}/recruitments/{recruitment_id}/close"),
        BOT_TOKEN,
        "close-recruitment",
        &json!({ "actor": actor(OPERATOR_ID, "Guild A Operator", &[OPERATOR_ROLE_A]) }),
    )
    .await?;
    assert_eq!(close_response.status(), StatusCode::OK);
    let closed = body_json(close_response).await?;
    assert_eq!(closed["data"]["recruitment"]["status"], "CLOSED");
    assert_eq!(
        closed["data"]["participants"].as_array().map(Vec::len),
        Some(10)
    );
    assert_eq!(
        closed["data"]["links"]["entry"],
        format!("/g/{GUILD_A}/recruitments/{recruitment_id}/entry")
    );

    let confirmation_notifications = sqlx::query_as::<_, (String, String, Value)>(
        "SELECT channel_id, deduplication_key, payload
         FROM bot_notification_outbox
         WHERE guild_id = $1
           AND notification_type = 'RECRUITMENT_PARTICIPANT_CONFIRMATION_DM_CREATE'
           AND payload ->> 'recruitmentId' = $2
         ORDER BY channel_id",
    )
    .bind(GUILD_A)
    .bind(&recruitment_id)
    .fetch_all(&pool)
    .await?;
    assert!(confirmation_notifications.is_empty());

    let close_replay = bot_request(
        &app,
        Method::POST,
        &format!("/api/guilds/{GUILD_A}/recruitments/{recruitment_id}/close"),
        BOT_TOKEN,
        "close-recruitment",
        &json!({ "actor": actor(OPERATOR_ID, "Guild A Operator", &[OPERATOR_ROLE_A]) }),
    )
    .await?;
    assert_eq!(close_replay.status(), StatusCode::OK);
    assert_eq!(body_json(close_replay).await?["data"], closed["data"]);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*)
             FROM bot_notification_outbox
             WHERE guild_id = $1
               AND notification_type = 'RECRUITMENT_PARTICIPANT_CONFIRMATION_DM_CREATE'
               AND payload ->> 'recruitmentId' = $2",
        )
        .bind(GUILD_A)
        .bind(&recruitment_id)
        .fetch_one(&pool)
        .await?,
        0
    );

    assert_auction_recruitment_force_cancel(&app, &pool).await?;

    let setup_b = json!({
        "actor": actor(OTHER_OPERATOR_ID, "Guild B Operator", &[OPERATOR_ROLE_B]),
        "guildName": "Other Guild",
        "guildIconHash": null,
        "operatorRoleId": OPERATOR_ROLE_B,
        "recruitmentChannelId": CHANNEL_ID,
        "entryChannelId": null,
        "resultChannelId": null,
        "logChannelId": null,
        "seasonName": "2026 Summer"
    });
    let setup_b_response = bot_request(
        &app,
        Method::POST,
        &format!("/internal/guilds/{GUILD_B}/setup"),
        BOT_TOKEN,
        "setup-guild-b",
        &setup_b,
    )
    .await?;
    assert_eq!(setup_b_response.status(), StatusCode::OK);
    assert_eq!(
        sqlx::query_as::<_, (String, Option<String>, Option<String>, Option<String>)>(
            "SELECT recruitment_channel_id, entry_channel_id, result_channel_id, log_channel_id
             FROM guild_config WHERE guild_id = $1",
        )
        .bind(GUILD_B)
        .fetch_one(&pool)
        .await?,
        (CHANNEL_ID.to_owned(), None, None, None)
    );

    assert_error(
        bot_request(
            &app,
            Method::PUT,
            &format!("/api/guilds/{GUILD_B}/recruitments/{recruitment_id}/message"),
            BOT_TOKEN,
            "cross-tenant-message-bind",
            &json!({
                "actor": actor(OTHER_OPERATOR_ID, "Guild B Operator", &[OPERATOR_ROLE_B]),
                "channelId": "400000000000000002",
                "messageId": "500000000000000002"
            }),
        )
        .await?,
        StatusCode::NOT_FOUND,
        "not_found",
    )
    .await?;

    pool.close().await;
    drop(server);
    Ok(())
}

async fn assert_auction_recruitment_force_cancel(app: &Router, pool: &sqlx::PgPool) -> TestResult {
    let operator = actor(OPERATOR_ID, "Guild A Operator", &[OPERATOR_ROLE_A]);
    let created = bot_request(
        app,
        Method::POST,
        &format!("/api/guilds/{GUILD_A}/auction-recruitments"),
        BOT_TOKEN,
        "create-auction-force-cancel",
        &json!({
            "actor": operator,
            "format": "TEN_PERSON",
            "semifinalBestOf": null,
            "finalBestOf": 3,
            "hardFearless": false,
            "expectedStartAt": "2026-07-26T11:00:00Z",
        }),
    )
    .await?;
    assert_eq!(created.status(), StatusCode::CREATED);
    let created = body_json(created).await?;
    let recruitment_id = created["data"]["recruitment"]["id"]
        .as_str()
        .ok_or("created auction recruitment had no ID")?
        .to_owned();
    let recruitment_uuid = Uuid::parse_str(&recruitment_id)?;
    let path = format!("/api/guilds/{GUILD_A}/auction-recruitments/{recruitment_id}/force-cancel");

    assert_error(
        bot_request(
            app,
            Method::POST,
            &path,
            BOT_TOKEN,
            "non-operator-auction-force-cancel",
            &json!({ "actor": actor(MEMBER_IDS[1], "Member 2", &[]) }),
        )
        .await?,
        StatusCode::FORBIDDEN,
        "operator_required",
    )
    .await?;

    let request = json!({
        "actor": actor(OPERATOR_ID, "Guild A Operator", &[OPERATOR_ROLE_A])
    });
    let cancelled = bot_request(
        app,
        Method::POST,
        &path,
        BOT_TOKEN,
        "auction-force-cancel",
        &request,
    )
    .await?;
    assert_eq!(cancelled.status(), StatusCode::OK);
    let cancelled = body_json(cancelled).await?;
    assert_eq!(cancelled["data"]["recruitmentId"], recruitment_id);
    assert_eq!(cancelled["data"]["recruitmentStatus"], "CANCELLED");
    assert!(
        cancelled["data"]["recruitmentRevision"]
            .as_i64()
            .is_some_and(|revision| revision > 0)
    );
    assert!(cancelled["data"]["tournamentId"].is_null());
    assert!(cancelled["data"]["tournamentStatus"].is_null());
    assert!(cancelled["data"]["tournamentRevision"].is_null());
    assert_eq!(cancelled["data"]["reversedGameCount"], 0);

    let replay = bot_request(
        app,
        Method::POST,
        &path,
        BOT_TOKEN,
        "auction-force-cancel",
        &request,
    )
    .await?;
    assert_eq!(replay.status(), StatusCode::OK);
    assert_eq!(body_json(replay).await?["data"], cancelled["data"]);
    assert_eq!(
        sqlx::query_as::<_, (String, i64)>(
            "SELECT status, revision
             FROM auction_recruitments
             WHERE guild_id = $1 AND id = $2",
        )
        .bind(GUILD_A)
        .bind(recruitment_uuid)
        .fetch_one(pool)
        .await?,
        (
            "CANCELLED".to_owned(),
            cancelled["data"]["recruitmentRevision"]
                .as_i64()
                .unwrap_or_default()
        )
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*)
             FROM admin_audit_log
             WHERE guild_id = $1
               AND action = 'AUCTION_RECRUITMENT_FORCE_CANCELLED'
               AND target_id = $2",
        )
        .bind(GUILD_A)
        .bind(&recruitment_id)
        .fetch_one(pool)
        .await?,
        1
    );
    Ok(())
}

fn actor(user_id: &str, display_name: &str, role_ids: &[&str]) -> Value {
    json!({
        "id": user_id,
        "globalDisplayName": display_name,
        "displayName": display_name,
        "avatarHash": null,
        "roleIds": role_ids
    })
}

fn find_participant<'a>(body: &'a Value, user_id: &str) -> TestResult<&'a Value> {
    body["data"]["participants"]
        .as_array()
        .and_then(|participants| {
            participants
                .iter()
                .find(|participant| participant["id"] == user_id)
        })
        .ok_or_else(|| format!("participant {user_id} was not found").into())
}

async fn bot_request(
    app: &Router,
    method: Method,
    uri: &str,
    token: &str,
    idempotency_key: &str,
    body: &Value,
) -> TestResult<Response<Body>> {
    let idempotency_key = interaction_id(idempotency_key);
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .header("idempotency-key", idempotency_key)
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
    ]);
    Ok(Config::from_map(&values)?)
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
