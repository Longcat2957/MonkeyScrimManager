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
const POSTGRES_PASSWORD: &str = "monkeybot-data-transfer-endpoint-password";
const DATABASE: &str = "monkeybot_data_transfer_endpoints";
const OPERATOR_ID: &str = "100000000000000001";
const MEMBER_ID: &str = "100000000000000002";
const GUILD_ID: &str = "200000000000000001";
const SOURCE_GUILD_ID: &str = "200000000000000099";
const OPERATOR_ROLE_ID: &str = "300000000000000001";
const DISCORD_CLIENT_ID: &str = "900000000000000001";
const PUBLIC_ORIGIN: &str = "http://localhost:5173";
const OAUTH_KEY_BASE64URL: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const OPERATOR_SESSION: &str = "data-transfer-operator-session";
const OPERATOR_CSRF: &str = "data-transfer-operator-csrf";
const SEASON_ID: &str = "01900000-0000-7000-8000-000000000011";
const TARGET_ACTIVE_SEASON_ID: &str = "01900000-0000-7000-8000-000000000012";
const TARGET_CLOSED_SEASON_ID: &str = "01900000-0000-7000-8000-000000000013";
const TARGET_CLOSED_RECRUITMENT_ID: &str = "01900000-0000-7000-8000-000000000014";
const TARGET_CLOSED_SERIES_ID: &str = "01900000-0000-7000-8000-000000000015";
const CONFLICT_IDEMPOTENCY_KEY: &str = "data-transfer-conflict-preview";
const STALE_IDEMPOTENCY_KEY: &str = "data-transfer-stale-mmr";
const APPLY_IDEMPOTENCY_KEY: &str = "data-transfer-apply";
const FULL_APPLY_IDEMPOTENCY_KEY: &str = "full-data-transfer-apply";
const RECRUITMENT_ID: &str = "01900000-0000-7000-8000-000000000021";
const SERIES_ID: &str = "01900000-0000-7000-8000-000000000022";
const GAME_INPUT_ID: &str = "01900000-0000-7000-8000-000000000023";
const GAME_ID: &str = "01900000-0000-7000-8000-000000000024";
const LEGACY_RIOT_PUUID: &str = "legacy-migration-puuid";
const CURRENT_RIOT_PUUID: &str = "current-monkeybot-puuid";

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn guild_data_transfer_exports_previews_and_applies_mmr_seed_idempotently() -> TestResult {
    tokio::time::timeout(Duration::from_secs(120), run_flow())
        .await
        .map_err(|_| "guild data-transfer endpoint test exceeded 120 seconds")??;
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
    let auth_config = config.auth().cloned().expect("test auth config");
    let auth_service = Arc::new(AuthService::new(
        auth_config.oauth_encryption_key(),
        CookiePolicy::new(auth_config.secure_cookies(), auth_config.session_ttl()),
    )?);
    let auth_repository = Arc::new(AuthRepository::new(pool.clone()));
    create_login(
        &auth_repository,
        OPERATOR_ID,
        "Operator",
        OPERATOR_SESSION,
        OPERATOR_CSRF,
    )
    .await?;
    create_login(
        &auth_repository,
        MEMBER_ID,
        "Member",
        "data-transfer-member-session",
        "data-transfer-member-csrf",
    )
    .await?;
    seed_guild(&pool).await?;
    seed_member_mmr(&pool, 1600.0).await?;

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
    ));
    let app = build_router(state);
    let export_path = format!("/api/guilds/{GUILD_ID}/data-export");
    let preview_path = format!("/api/guilds/{GUILD_ID}/data-imports/preview");

    let exported = operator_post(&app, &export_path, None, None).await?;
    assert_eq!(exported.status(), StatusCode::OK);
    let exported = body_json(exported).await?;
    let archive = exported["data"]["archive"].clone();
    assert_eq!(archive["format"], "MONKEYBOT_DATA_TRANSFER");
    assert_eq!(archive["kind"], "MEMBER_SEASON_MMR_SEED");
    assert_eq!(archive["schemaVersion"], 1);
    assert_eq!(archive["source"]["season"]["id"], SEASON_ID);
    assert!(archive.get("signature").is_none());
    assert_exported_member(&archive)?;

    set_member_mmr(&pool, 1500.0).await?;
    let mut edited_archive = archive.clone();
    set_archive_member_mmr(&mut edited_archive, MEMBER_ID, 1650.0)?;

    set_member_status(&pool, "INACTIVE").await?;
    let conflict_preview = preview_archive(&app, &preview_path, edited_archive.clone()).await?;
    assert_eq!(conflict_preview["summary"]["matchedMembers"], 1);
    assert_eq!(
        conflict_preview["conflicts"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        conflict_preview["conflicts"][0]["reason"],
        "member_not_active"
    );
    let conflict_apply = apply_preview(&app, &conflict_preview, CONFLICT_IDEMPOTENCY_KEY).await?;
    assert_error(
        conflict_apply,
        StatusCode::CONFLICT,
        "preview_conflicts",
        "대상 길드 멤버 상태가 검토 결과와 달라져 가져올 수 없습니다.",
    )
    .await?;

    set_member_status(&pool, "ACTIVE").await?;
    let stale_preview = preview_archive(&app, &preview_path, edited_archive.clone()).await?;
    assert_eq!(stale_preview["summary"]["changes"], 5);
    set_member_lane_mmr(&pool, "MID", 1499.0).await?;
    let stale_apply = apply_preview(&app, &stale_preview, STALE_IDEMPOTENCY_KEY).await?;
    assert_error(
        stale_apply,
        StatusCode::CONFLICT,
        "mmr_state_changed",
        "대상 길드의 MMR이 변경되었습니다. 파일을 다시 검증해 주세요.",
    )
    .await?;

    set_member_lane_mmr(&pool, "MID", 1500.0).await?;
    let successful_preview = preview_archive(&app, &preview_path, edited_archive).await?;
    assert_eq!(successful_preview["summary"]["changes"], 5);
    assert_eq!(successful_preview["conflicts"], json!([]));
    let applied = apply_preview(&app, &successful_preview, APPLY_IDEMPOTENCY_KEY).await?;
    assert_eq!(applied.status(), StatusCode::OK);
    let applied = body_json(applied).await?;
    assert_eq!(applied["data"]["status"], "APPLIED");
    assert_eq!(applied["data"]["summary"]["changes"], 5);
    assert_eq!(applied["data"]["targetRevision"], 1);
    assert_applied_effects(&pool, &successful_preview, 1650.0).await?;

    let replay = apply_preview(&app, &successful_preview, APPLY_IDEMPOTENCY_KEY).await?;
    assert_eq!(replay.status(), StatusCode::OK);
    let replay = body_json(replay).await?;
    assert_eq!(replay["data"], applied["data"]);
    assert_eq!(data_import_ledger_count(&pool).await?, 5);

    seed_record_history(&pool).await?;
    seed_legacy_global_profile(&pool).await?;
    assert_full_data_transfer_flow(&app, &pool).await?;

    pool.close().await;
    drop(server);
    Ok(())
}

async fn assert_full_data_transfer_flow(app: &Router, pool: &PgPool) -> TestResult {
    let export_path = format!("/api/guilds/{GUILD_ID}/full-data-export");
    let preview_path = format!("/api/guilds/{GUILD_ID}/full-data-imports/preview");
    let exported = operator_post(app, &export_path, None, None).await?;
    assert_eq!(exported.status(), StatusCode::OK);
    let exported = body_json(exported).await?;
    let mut archive = exported["data"]["archive"].clone();
    assert_eq!(archive["kind"], "GUILD_FULL_SNAPSHOT");
    assert_eq!(archive["schemaVersion"], 1);
    assert_eq!(exported["data"]["summary"]["games"], 1);
    assert_eq!(exported["data"]["summary"]["mmrLedgerEntries"], 5);
    assert_eq!(
        archive["tables"]["game_input_picks"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        archive["tables"]["game_bans"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        archive["tables"]["riot_accounts"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        archive["tables"]["user_champion_preferences"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    make_cross_guild_archive(&mut archive)?;
    replace_with_current_global_profile(pool).await?;

    clear_importable_domain(pool).await?;
    seed_empty_target_active_season(pool).await?;
    seed_target_closed_history(pool).await?;
    let preserved_domain_rows = importable_domain_count(pool).await?;
    assert!(preserved_domain_rows > 1);
    seed_target_season_activity(pool).await?;
    assert_eq!(active_season_domain_count(pool).await?, 1);
    let blocked = preview_archive(app, &preview_path, archive.clone()).await?;
    assert_eq!(
        blocked["conflicts"][0]["reason"],
        "target_active_season_not_empty"
    );
    assert_eq!(blocked["remapsSourceGuild"], true);
    assert_eq!(blocked["usesTargetActiveSeason"], false);
    assert_eq!(blocked["rebasesDisplayNumbers"], false);
    clear_target_season_activity(pool).await?;
    assert_eq!(active_season_domain_count(pool).await?, 0);
    let preview = preview_archive(app, &preview_path, archive).await?;
    assert_eq!(preview["conflicts"], json!([]));
    assert_eq!(preview["source"]["guildId"], SOURCE_GUILD_ID);
    assert_eq!(preview["remapsSourceGuild"], true);
    assert_eq!(preview["usesTargetActiveSeason"], true);
    assert_eq!(preview["rebasesDisplayNumbers"], true);
    assert_eq!(preview["summary"]["games"], 1);
    assert_eq!(preview["summary"]["mmrLedgerEntries"], 5);
    assert_eq!(
        importable_domain_count(pool).await?,
        preserved_domain_rows,
        "preview must preserve the closed history and empty target active season"
    );

    let import_id = preview["importId"]
        .as_str()
        .ok_or("full import preview id is missing")?;
    let apply_path = format!("/api/guilds/{GUILD_ID}/full-data-imports/{import_id}/apply");
    let applied = operator_post(
        app,
        &apply_path,
        Some(json!({
            "expectedDigest": preview["digest"],
            "expectedTargetDigest": preview["targetDigest"],
        })),
        Some(FULL_APPLY_IDEMPOTENCY_KEY),
    )
    .await?;
    assert_eq!(applied.status(), StatusCode::OK);
    let applied = body_json(applied).await?;
    assert_eq!(applied["data"]["status"], "APPLIED");
    assert_eq!(applied["data"]["summary"]["games"], 1);
    assert_eq!(applied["data"]["remappedSourceGuild"], true);
    assert_eq!(applied["data"]["usedTargetActiveSeason"], true);
    assert_eq!(applied["data"]["rebasedDisplayNumbers"], true);
    assert_restored_full_history(pool).await?;
    assert_current_global_profile_preserved(pool).await?;

    let replay = operator_post(
        app,
        &apply_path,
        Some(json!({
            "expectedDigest": preview["digest"],
            "expectedTargetDigest": preview["targetDigest"],
        })),
        Some(FULL_APPLY_IDEMPOTENCY_KEY),
    )
    .await?;
    assert_eq!(replay.status(), StatusCode::OK);
    assert_eq!(body_json(replay).await?["data"], applied["data"]);
    Ok(())
}

fn make_cross_guild_archive(archive: &mut Value) -> TestResult {
    replace_exact_json_string(archive, GUILD_ID, SOURCE_GUILD_ID);
    archive["source"]["guildName"] = json!("Historical Source Guild");

    let recruitment = archive["tables"]["recruitments"]
        .as_array_mut()
        .and_then(|rows| rows.first_mut())
        .ok_or("full archive recruitment row is missing")?;
    recruitment["channel_id"] = json!("400000000000000001");
    recruitment["message_id"] = json!("400000000000000002");

    let series = archive["tables"]["series"]
        .as_array_mut()
        .and_then(|rows| rows.first_mut())
        .ok_or("full archive series row is missing")?;
    series["end_card_channel_id"] = json!("400000000000000003");
    series["end_card_message_id"] = json!("400000000000000004");
    Ok(())
}

fn replace_exact_json_string(value: &mut Value, source: &str, target: &str) {
    match value {
        Value::String(text) if text == source => target.clone_into(text),
        Value::Array(values) => {
            for value in values {
                replace_exact_json_string(value, source, target);
            }
        }
        Value::Object(object) => {
            for value in object.values_mut() {
                replace_exact_json_string(value, source, target);
            }
        }
        _ => {}
    }
}

fn assert_exported_member(archive: &Value) -> TestResult {
    let members = archive["members"]
        .as_array()
        .ok_or("export archive members are missing")?;
    let member = members
        .iter()
        .find(|member| member["discordId"] == MEMBER_ID)
        .ok_or("export archive member is missing")?;
    assert_eq!(member["displayName"], "Member");
    assert_eq!(member["lanes"].as_array().map(Vec::len), Some(5));
    assert!(
        member["lanes"]
            .as_array()
            .is_some_and(|lanes| lanes.iter().all(|lane| lane["mmr"] == 1600.0))
    );
    Ok(())
}

fn set_archive_member_mmr(archive: &mut Value, discord_id: &str, mmr: f64) -> TestResult {
    let members = archive["members"]
        .as_array_mut()
        .ok_or("export archive members are missing")?;
    let member = members
        .iter_mut()
        .find(|member| member["discordId"] == discord_id)
        .ok_or("export archive member is missing")?;
    let lanes = member["lanes"]
        .as_array_mut()
        .ok_or("export archive member lanes are missing")?;
    if lanes.len() != 5 {
        return Err("export archive member lane count is invalid".into());
    }
    for lane in lanes {
        lane["mmr"] = json!(mmr);
    }
    Ok(())
}

async fn preview_archive(app: &Router, path: &str, archive: Value) -> TestResult<Value> {
    let preview = operator_post(app, path, Some(json!({"archive": archive})), None).await?;
    assert_eq!(preview.status(), StatusCode::OK);
    Ok(body_json(preview).await?["data"].clone())
}

async fn apply_preview(
    app: &Router,
    preview: &Value,
    idempotency_key: &str,
) -> TestResult<Response<Body>> {
    let import_id = preview["importId"]
        .as_str()
        .ok_or("import preview id is missing")?;
    let path = format!("/api/guilds/{GUILD_ID}/data-imports/{import_id}/apply");
    operator_post(
        app,
        &path,
        Some(json!({
            "expectedDigest": preview["digest"],
            "expectedTargetRevision": preview["targetRevision"],
        })),
        Some(idempotency_key),
    )
    .await
}

async fn operator_post(
    app: &Router,
    path: &str,
    body: Option<Value>,
    idempotency_key: Option<&str>,
) -> TestResult<Response<Body>> {
    let mut request = Request::builder()
        .method(Method::POST)
        .uri(path)
        .header(
            header::COOKIE,
            format!("monkeybot_session={OPERATOR_SESSION}; monkeybot_csrf={OPERATOR_CSRF}"),
        )
        .header(header::ORIGIN, PUBLIC_ORIGIN)
        .header("x-csrf-token", OPERATOR_CSRF);
    if let Some(key) = idempotency_key {
        request = request.header("idempotency-key", key);
    }
    let body = if let Some(body) = body {
        request = request.header(header::CONTENT_TYPE, "application/json");
        Body::from(serde_json::to_vec(&body)?)
    } else {
        Body::empty()
    };
    Ok(app.clone().oneshot(request.body(body)?).await?)
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
    assert!(body["error"]["requestId"].is_string());
    Ok(())
}

async fn body_json(response: Response<Body>) -> TestResult<Value> {
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

async fn create_login(
    repository: &AuthRepository,
    user_id: &str,
    display_name: &str,
    session_token: &str,
    csrf_token: &str,
) -> TestResult {
    repository
        .complete_login(&NewLogin {
            discord_user_id: DiscordUserId::parse(user_id)?,
            display_name: display_name.to_owned(),
            avatar_hash: None,
            previous_session_token_hash: None,
            session_id: Uuid::now_v7(),
            token_hash: AuthService::hash_secret(session_token),
            csrf_secret_hash: AuthService::hash_secret(csrf_token),
            expires_at: OffsetDateTime::now_utc() + time::Duration::hours(1),
        })
        .await?;
    Ok(())
}

async fn seed_guild(pool: &PgPool) -> TestResult {
    let now = OffsetDateTime::now_utc();
    let season_id = Uuid::parse_str(SEASON_ID)?;
    sqlx::query(
        "INSERT INTO guilds (
             guild_id, discord_name, icon_hash, status, installed_by,
             installed_at, activated_at, left_at, updated_at
         ) VALUES ($1, 'Data Transfer Test Guild', NULL, 'ACTIVE', $2, $3, $3, NULL, $3)",
    )
    .bind(GUILD_ID)
    .bind(OPERATOR_ID)
    .bind(now)
    .execute(pool)
    .await?;

    for (user_id, display_name) in [(OPERATOR_ID, "Operator"), (MEMBER_ID, "Member")] {
        sqlx::query(
            "INSERT INTO guild_members (
                 guild_id, user_id, display_name, status, main_lane, first_seen_at,
                 registered_at, discord_joined_at, last_active_at, last_synced_at, inactivated_at
             ) VALUES ($1, $2, $3, 'ACTIVE', NULL, $4, $4, $4, $4, $4, NULL)",
        )
        .bind(GUILD_ID)
        .bind(user_id)
        .bind(display_name)
        .bind(now)
        .execute(pool)
        .await?;
    }

    sqlx::query(
        "INSERT INTO guild_config (
             guild_id, display_name, operator_role_id, recruitment_channel_id,
             log_channel_id, is_externally_public, revision, updated_by, updated_at
         ) VALUES ($1, 'Data Transfer Test Guild', $2, NULL, NULL, false, 0, $3, $4)",
    )
    .bind(GUILD_ID)
    .bind(OPERATOR_ROLE_ID)
    .bind(OPERATOR_ID)
    .bind(now)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO guild_member_discord_roles (guild_id, user_id, role_id, last_synced_at)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(GUILD_ID)
    .bind(OPERATOR_ID)
    .bind(OPERATOR_ROLE_ID)
    .bind(now)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO seasons (
             guild_id, id, name, status, started_at, ended_at, created_by, created_at
         ) VALUES ($1, $2, 'Season 1', 'ACTIVE', $3, NULL, $4, $3)",
    )
    .bind(GUILD_ID)
    .bind(season_id)
    .bind(now)
    .bind(OPERATOR_ID)
    .execute(pool)
    .await?;
    Ok(())
}

async fn seed_member_mmr(pool: &PgPool, mmr: f64) -> TestResult {
    let now = OffsetDateTime::now_utc();
    let season_id = Uuid::parse_str(SEASON_ID)?;
    for lane in ["TOP", "JUNGLE", "MID", "BOTTOM", "SUPPORT"] {
        sqlx::query(
            "INSERT INTO user_lane_mmr (
                 guild_id, season_id, user_id, lane, mmr, games_played, wins, revision, updated_at
             ) VALUES ($1, $2, $3, $4, $5, 0, 0, 0, $6)",
        )
        .bind(GUILD_ID)
        .bind(season_id)
        .bind(MEMBER_ID)
        .bind(lane)
        .bind(mmr)
        .bind(now)
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn seed_record_history(pool: &PgPool) -> TestResult {
    let now = OffsetDateTime::now_utc();
    let season_id = Uuid::parse_str(SEASON_ID)?;
    let recruitment_id = Uuid::parse_str(RECRUITMENT_ID)?;
    let series_id = Uuid::parse_str(SERIES_ID)?;
    let input_id = Uuid::parse_str(GAME_INPUT_ID)?;
    let game_id = Uuid::parse_str(GAME_ID)?;
    sqlx::query(
        "INSERT INTO recruitments (
             guild_id, id, season_id, display_number, title, target_count, status, revision,
             channel_id, message_id, created_by, created_at, updated_by, updated_at,
             closed_at, converted_at, cancelled_at
         ) VALUES (
             $1, $2, $3, 1, 'History Recruitment', 10, 'CONVERTED', 1,
             NULL, NULL, $4, $5, $4, $5, $5, $5, NULL
         )",
    )
    .bind(GUILD_ID)
    .bind(recruitment_id)
    .bind(season_id)
    .bind(OPERATOR_ID)
    .bind(now)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO series (
             guild_id, id, season_id, source_recruitment_id, display_number, title, best_of,
             status, winning_team, terminal_reason, revision, started_at, ended_at, created_by,
             end_card_channel_id, end_card_message_id, updated_by, updated_at, deleted_at
         ) VALUES (
             $1, $2, $3, $4, 1, 'History Series', 1, 'COMPLETED', 'TEAM_1',
             'SCORE_REACHED', 1, $5, $5, $6, NULL, NULL, $6, $5, NULL
         )",
    )
    .bind(GUILD_ID)
    .bind(series_id)
    .bind(season_id)
    .bind(recruitment_id)
    .bind(now)
    .bind(OPERATOR_ID)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO series_participants (
             guild_id, series_id, user_id, team, lane, mmr_at_lock, mmr_snapshot_origin
         ) VALUES ($1, $2, $3, 'TEAM_1', 'MID', 1650, 'CAPTURED')",
    )
    .bind(GUILD_ID)
    .bind(series_id)
    .bind(MEMBER_ID)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO game_inputs (
             guild_id, id, series_id, auction_match_id, set_number, side_preset, winning_side,
             patch_version, status, updated_by, updated_at
         ) VALUES ($1, $2, $3, NULL, 1, 'DEFAULT', 'BLUE', '16.13.1', 'SUBMITTED', $4, $5)",
    )
    .bind(GUILD_ID)
    .bind(input_id)
    .bind(series_id)
    .bind(OPERATOR_ID)
    .bind(now)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO game_input_picks (guild_id, game_input_id, user_id, champion_id)
         VALUES ($1, $2, $3, 'Ahri')",
    )
    .bind(GUILD_ID)
    .bind(input_id)
    .bind(MEMBER_ID)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO game_input_bans (guild_id, game_input_id, side, slot, champion_id)
         VALUES ($1, $2, 'BLUE', 1, 'Aatrox')",
    )
    .bind(GUILD_ID)
    .bind(input_id)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO games (
             guild_id, id, season_id, series_id, auction_match_id, source_input_id,
             game_number, side_preset, winning_side, duration_seconds, patch_version,
             riot_match_id, status, mmr_status, played_at, confirmed_by, confirmed_by_type,
             confirmed_at, voided_by, voided_by_type, voided_at, void_reason
         ) VALUES (
             $1, $2, $3, $4, NULL, $5, 1, 'DEFAULT', 'BLUE', 1800, '16.13.1',
             NULL, 'CONFIRMED', 'LEGACY_NOT_APPLIED', $6, $7, 'MIGRATION', $6,
             NULL, NULL, NULL, NULL
         )",
    )
    .bind(GUILD_ID)
    .bind(game_id)
    .bind(season_id)
    .bind(series_id)
    .bind(input_id)
    .bind(now)
    .bind(OPERATOR_ID)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO game_participants (guild_id, game_id, user_id, team, lane, champion_id)
         VALUES ($1, $2, $3, 'TEAM_1', 'MID', 'Ahri')",
    )
    .bind(GUILD_ID)
    .bind(game_id)
    .bind(MEMBER_ID)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO game_participant_stats (
             guild_id, game_id, user_id, kills, deaths, assists, cs
         ) VALUES ($1, $2, $3, 7, 2, 9, 211)",
    )
    .bind(GUILD_ID)
    .bind(game_id)
    .bind(MEMBER_ID)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO game_bans (guild_id, game_id, side, slot, champion_id)
         VALUES ($1, $2, 'BLUE', 1, 'Aatrox')",
    )
    .bind(GUILD_ID)
    .bind(game_id)
    .execute(pool)
    .await?;
    Ok(())
}

async fn seed_legacy_global_profile(pool: &PgPool) -> TestResult {
    let now = OffsetDateTime::now_utc();
    sqlx::query(
        "INSERT INTO riot_accounts (
             puuid, user_id, platform, routing_region, game_name, tag_line,
             normalized_game_name, normalized_tag_line, profile_icon_id, is_main,
             main_position, main_position_updated_at, last_synced_at, created_at,
             updated_at, origin
         ) VALUES (
             $1, $2, 'KR', 'ASIA', 'LegacyMember', 'KR1', 'legacymember', 'kr1',
             1, true, 'MID', $3, $3, $3, $3, 'RIOT'
         )",
    )
    .bind(LEGACY_RIOT_PUUID)
    .bind(MEMBER_ID)
    .bind(now)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO user_champion_preferences (user_id, lane, position, champion_id)
         VALUES ($1, 'MID', 1, 'Ahri'), ($1, 'MID', 2, 'Garen')",
    )
    .bind(MEMBER_ID)
    .execute(pool)
    .await?;
    Ok(())
}

async fn replace_with_current_global_profile(pool: &PgPool) -> TestResult {
    let now = OffsetDateTime::now_utc();
    sqlx::query("DELETE FROM riot_accounts WHERE user_id = $1")
        .bind(MEMBER_ID)
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO riot_accounts (
             puuid, user_id, platform, routing_region, game_name, tag_line,
             normalized_game_name, normalized_tag_line, profile_icon_id, is_main,
             main_position, main_position_updated_at, last_synced_at, created_at,
             updated_at, origin
         ) VALUES (
             $1, $2, 'KR', 'ASIA', 'CurrentMember', 'KR1', 'currentmember', 'kr1',
             2, true, 'JUNGLE', $3, $3, $3, $3, 'RIOT'
         )",
    )
    .bind(CURRENT_RIOT_PUUID)
    .bind(MEMBER_ID)
    .bind(now)
    .execute(pool)
    .await?;
    sqlx::query("DELETE FROM user_champion_preferences WHERE user_id = $1 AND lane = 'MID'")
        .bind(MEMBER_ID)
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO user_champion_preferences (user_id, lane, position, champion_id)
         VALUES ($1, 'MID', 1, 'Garen'), ($1, 'MID', 2, 'Ahri')",
    )
    .bind(MEMBER_ID)
    .execute(pool)
    .await?;
    Ok(())
}

async fn assert_current_global_profile_preserved(pool: &PgPool) -> TestResult {
    let riot_puuids = sqlx::query_scalar::<_, String>(
        "SELECT puuid FROM riot_accounts WHERE user_id = $1 ORDER BY puuid",
    )
    .bind(MEMBER_ID)
    .fetch_all(pool)
    .await?;
    assert_eq!(riot_puuids, vec![CURRENT_RIOT_PUUID.to_owned()]);

    let champions = sqlx::query_scalar::<_, String>(
        "SELECT champion_id
         FROM user_champion_preferences
         WHERE user_id = $1 AND lane = 'MID'
         ORDER BY position",
    )
    .bind(MEMBER_ID)
    .fetch_all(pool)
    .await?;
    assert_eq!(champions, vec!["Garen".to_owned(), "Ahri".to_owned()]);
    Ok(())
}

async fn clear_importable_domain(pool: &PgPool) -> TestResult {
    for statement in [
        "DELETE FROM guild_data_imports WHERE guild_id = $1",
        "DELETE FROM mmr_ledger_entries WHERE guild_id = $1",
        "DELETE FROM user_lane_mmr WHERE guild_id = $1",
        "DELETE FROM game_participant_stats WHERE guild_id = $1",
        "DELETE FROM game_bans WHERE guild_id = $1",
        "DELETE FROM game_participants WHERE guild_id = $1",
        "DELETE FROM games WHERE guild_id = $1",
        "DELETE FROM game_input_picks WHERE guild_id = $1",
        "DELETE FROM game_input_bans WHERE guild_id = $1",
        "DELETE FROM game_inputs WHERE guild_id = $1",
        "DELETE FROM series_participants WHERE guild_id = $1",
        "DELETE FROM series WHERE guild_id = $1",
        "DELETE FROM recruitments WHERE guild_id = $1",
        "DELETE FROM seasons WHERE guild_id = $1",
    ] {
        sqlx::query(statement).bind(GUILD_ID).execute(pool).await?;
    }
    Ok(())
}

async fn seed_empty_target_active_season(pool: &PgPool) -> TestResult {
    let activated_at = sqlx::query_scalar::<_, OffsetDateTime>(
        "SELECT activated_at FROM guilds WHERE guild_id = $1",
    )
    .bind(GUILD_ID)
    .fetch_one(pool)
    .await?;
    sqlx::query(
        "INSERT INTO seasons (
             guild_id, id, name, status, started_at, ended_at, created_by, created_at
         ) VALUES ($1, $2, 'Season 1', 'ACTIVE', $3, NULL, $4, $3)",
    )
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(TARGET_ACTIVE_SEASON_ID)?)
    .bind(activated_at)
    .bind(OPERATOR_ID)
    .execute(pool)
    .await?;
    Ok(())
}

async fn seed_target_closed_history(pool: &PgPool) -> TestResult {
    let ended_at = OffsetDateTime::now_utc();
    let started_at = ended_at - time::Duration::hours(1);
    let season_id = Uuid::parse_str(TARGET_CLOSED_SEASON_ID)?;
    let recruitment_id = Uuid::parse_str(TARGET_CLOSED_RECRUITMENT_ID)?;
    sqlx::query(
        "INSERT INTO seasons (
             guild_id, id, name, status, started_at, ended_at, created_by, created_at
         ) VALUES ($1, $2, 'Closed history', 'CLOSED', $3, $4, $5, $3)",
    )
    .bind(GUILD_ID)
    .bind(season_id)
    .bind(started_at)
    .bind(ended_at)
    .bind(OPERATOR_ID)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO recruitments (
             guild_id, id, season_id, display_number, title, target_count, status, revision,
             channel_id, message_id, created_by, created_at, updated_by, updated_at,
             closed_at, converted_at, cancelled_at
         ) VALUES (
             $1, $2, $3, 1, 'Closed recruitment', 10, 'CLOSED', 0,
             NULL, NULL, $4, $5, NULL, $6, $6, NULL, NULL
         )",
    )
    .bind(GUILD_ID)
    .bind(recruitment_id)
    .bind(season_id)
    .bind(OPERATOR_ID)
    .bind(started_at)
    .bind(ended_at)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO series (
             guild_id, id, season_id, source_recruitment_id, display_number, title, best_of,
             status, winning_team, terminal_reason, revision, started_at, ended_at, created_by,
             end_card_channel_id, end_card_message_id, updated_by, updated_at, deleted_at
         ) VALUES (
             $1, $2, $3, $4, 1, 'Closed series', 3, 'CANCELLED', NULL,
             'CANCELLED', 0, $5, $6, $7, NULL, NULL, NULL, $6, NULL
         )",
    )
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(TARGET_CLOSED_SERIES_ID)?)
    .bind(season_id)
    .bind(recruitment_id)
    .bind(started_at)
    .bind(ended_at)
    .bind(OPERATOR_ID)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO user_lane_mmr (
             guild_id, season_id, user_id, lane, mmr, games_played, wins, revision, updated_at
         ) VALUES ($1, $2, $3, 'TOP', 1510, 1, 1, 1, $4)",
    )
    .bind(GUILD_ID)
    .bind(season_id)
    .bind(OPERATOR_ID)
    .bind(ended_at)
    .execute(pool)
    .await?;
    Ok(())
}

async fn seed_target_season_activity(pool: &PgPool) -> TestResult {
    sqlx::query(
        "INSERT INTO user_lane_mmr (
             guild_id, season_id, user_id, lane, mmr, games_played, wins, revision, updated_at
         ) VALUES ($1, $2, $3, 'TOP', 1500, 0, 0, 0, now())",
    )
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(TARGET_ACTIVE_SEASON_ID)?)
    .bind(OPERATOR_ID)
    .execute(pool)
    .await?;
    Ok(())
}

async fn clear_target_season_activity(pool: &PgPool) -> TestResult {
    sqlx::query(
        "DELETE FROM user_lane_mmr
         WHERE guild_id = $1 AND season_id = $2 AND user_id = $3 AND lane = 'TOP'",
    )
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(TARGET_ACTIVE_SEASON_ID)?)
    .bind(OPERATOR_ID)
    .execute(pool)
    .await?;
    Ok(())
}

async fn importable_domain_count(pool: &PgPool) -> TestResult<i64> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT
             (SELECT count(*) FROM seasons WHERE guild_id = $1)
             + (SELECT count(*) FROM recruitments WHERE guild_id = $1)
             + (SELECT count(*) FROM series WHERE guild_id = $1)
             + (SELECT count(*) FROM games WHERE guild_id = $1)
             + (SELECT count(*) FROM user_lane_mmr WHERE guild_id = $1)
             + (SELECT count(*) FROM mmr_ledger_entries WHERE guild_id = $1)",
    )
    .bind(GUILD_ID)
    .fetch_one(pool)
    .await?)
}

async fn active_season_domain_count(pool: &PgPool) -> TestResult<i64> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT
             (SELECT count(*) FROM recruitments WHERE guild_id = $1 AND season_id = $2)
             + (SELECT count(*) FROM series WHERE guild_id = $1 AND season_id = $2)
             + (SELECT count(*) FROM games WHERE guild_id = $1 AND season_id = $2)
             + (SELECT count(*) FROM user_lane_mmr WHERE guild_id = $1 AND season_id = $2)
             + (SELECT count(*) FROM mmr_ledger_entries WHERE guild_id = $1 AND season_id = $2)",
    )
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(TARGET_ACTIVE_SEASON_ID)?)
    .fetch_one(pool)
    .await?)
}

async fn assert_restored_full_history(pool: &PgPool) -> TestResult {
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM seasons WHERE guild_id = $1 AND id = $2 AND status = 'CLOSED'"
        )
        .bind(GUILD_ID)
        .bind(Uuid::parse_str(TARGET_CLOSED_SEASON_ID)?)
        .fetch_one(pool)
        .await?,
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM seasons WHERE guild_id = $1 AND id = $2"
        )
        .bind(GUILD_ID)
        .bind(Uuid::parse_str(TARGET_ACTIVE_SEASON_ID)?)
        .fetch_one(pool)
        .await?,
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM seasons WHERE guild_id = $1 AND id = $2"
        )
        .bind(GUILD_ID)
        .bind(Uuid::parse_str(SEASON_ID)?)
        .fetch_one(pool)
        .await?,
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM games WHERE guild_id = $1 AND season_id = $2"
        )
        .bind(GUILD_ID)
        .bind(Uuid::parse_str(TARGET_ACTIVE_SEASON_ID)?)
        .fetch_one(pool)
        .await?,
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT display_number FROM recruitments WHERE guild_id = $1 AND id = $2"
        )
        .bind(GUILD_ID)
        .bind(Uuid::parse_str(TARGET_CLOSED_RECRUITMENT_ID)?)
        .fetch_one(pool)
        .await?,
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT display_number FROM recruitments WHERE guild_id = $1 AND id = $2"
        )
        .bind(GUILD_ID)
        .bind(Uuid::parse_str(RECRUITMENT_ID)?)
        .fetch_one(pool)
        .await?,
        2
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT display_number FROM series WHERE guild_id = $1 AND id = $2"
        )
        .bind(GUILD_ID)
        .bind(Uuid::parse_str(SERIES_ID)?)
        .fetch_one(pool)
        .await?,
        2
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM game_participant_stats WHERE guild_id = $1"
        )
        .bind(GUILD_ID)
        .fetch_one(pool)
        .await?,
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM mmr_ledger_entries WHERE guild_id = $1 AND season_id = $2"
        )
        .bind(GUILD_ID)
        .bind(Uuid::parse_str(TARGET_ACTIVE_SEASON_ID)?)
        .fetch_one(pool)
        .await?,
        5
    );
    assert!(
        sqlx::query_scalar::<_, bool>(
            "SELECT channel_id IS NULL AND message_id IS NULL
             FROM recruitments WHERE guild_id = $1 AND id = $2",
        )
        .bind(GUILD_ID)
        .bind(Uuid::parse_str(RECRUITMENT_ID)?)
        .fetch_one(pool)
        .await?
    );
    assert!(
        sqlx::query_scalar::<_, bool>(
            "SELECT end_card_channel_id IS NULL AND end_card_message_id IS NULL
             FROM series WHERE guild_id = $1 AND id = $2",
        )
        .bind(GUILD_ID)
        .bind(Uuid::parse_str(SERIES_ID)?)
        .fetch_one(pool)
        .await?
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM games WHERE guild_id = $1")
            .bind(SOURCE_GUILD_ID)
            .fetch_one(pool)
            .await?,
        0
    );
    let stored = sqlx::query_as::<_, (String, bool, String)>(
        "SELECT status, archive_payload IS NULL, source_guild_id
         FROM guild_full_data_imports
         WHERE guild_id = $1",
    )
    .bind(GUILD_ID)
    .fetch_one(pool)
    .await?;
    assert_eq!(
        stored,
        ("APPLIED".to_owned(), true, SOURCE_GUILD_ID.to_owned())
    );
    Ok(())
}

async fn set_member_mmr(pool: &PgPool, mmr: f64) -> TestResult {
    sqlx::query(
        "UPDATE user_lane_mmr
         SET mmr = $1, updated_at = CURRENT_TIMESTAMP
         WHERE guild_id = $2 AND season_id = $3 AND user_id = $4",
    )
    .bind(mmr)
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(SEASON_ID)?)
    .bind(MEMBER_ID)
    .execute(pool)
    .await?;
    Ok(())
}

async fn set_member_lane_mmr(pool: &PgPool, lane: &str, mmr: f64) -> TestResult {
    sqlx::query(
        "UPDATE user_lane_mmr
         SET mmr = $1, updated_at = CURRENT_TIMESTAMP
         WHERE guild_id = $2 AND season_id = $3 AND user_id = $4 AND lane = $5",
    )
    .bind(mmr)
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(SEASON_ID)?)
    .bind(MEMBER_ID)
    .bind(lane)
    .execute(pool)
    .await?;
    Ok(())
}

async fn set_member_status(pool: &PgPool, status: &str) -> TestResult {
    sqlx::query(
        "UPDATE guild_members
         SET status = $1,
             inactivated_at = CASE WHEN $1 = 'INACTIVE' THEN CURRENT_TIMESTAMP ELSE NULL END
         WHERE guild_id = $2 AND user_id = $3",
    )
    .bind(status)
    .bind(GUILD_ID)
    .bind(MEMBER_ID)
    .execute(pool)
    .await?;
    Ok(())
}

async fn assert_applied_effects(pool: &PgPool, preview: &Value, expected_mmr: f64) -> TestResult {
    let mmr_rows = sqlx::query_as::<_, (String, f64, i32, i32, i64)>(
        "SELECT lane, mmr, games_played, wins, revision
         FROM user_lane_mmr
         WHERE guild_id = $1 AND season_id = $2 AND user_id = $3
         ORDER BY lane",
    )
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(SEASON_ID)?)
    .bind(MEMBER_ID)
    .fetch_all(pool)
    .await?;
    assert_eq!(mmr_rows.len(), 5);
    assert!(mmr_rows.iter().all(|(_, mmr, games, wins, revision)| (
        *mmr, *games, *wins, *revision
    ) == (expected_mmr, 0, 0, 1)));

    let ledger_rows = sqlx::query_as::<_, (String, i16, i16, f64, f64)>(
        "SELECT source_type, games_played_delta, wins_delta, mmr_before, mmr_after
         FROM mmr_ledger_entries
         WHERE guild_id = $1 AND season_id = $2 AND user_id = $3
         ORDER BY lane",
    )
    .bind(GUILD_ID)
    .bind(Uuid::parse_str(SEASON_ID)?)
    .bind(MEMBER_ID)
    .fetch_all(pool)
    .await?;
    assert_eq!(ledger_rows.len(), 5);
    assert!(
        ledger_rows
            .iter()
            .all(|(source_type, games_delta, wins_delta, before, after)| {
                source_type == "DATA_IMPORT_SEED"
                    && (*games_delta, *wins_delta, *before, *after) == (0, 0, 1500.0, expected_mmr)
            })
    );

    let import_id = Uuid::parse_str(
        preview["importId"]
            .as_str()
            .ok_or("successful import preview id is missing")?,
    )?;
    let import = sqlx::query_as::<_, (String, bool)>(
        "SELECT status, archive_payload IS NULL
         FROM guild_data_imports
         WHERE guild_id = $1 AND id = $2",
    )
    .bind(GUILD_ID)
    .bind(import_id)
    .fetch_one(pool)
    .await?;
    assert_eq!(import, ("APPLIED".to_owned(), true));
    Ok(())
}

async fn data_import_ledger_count(pool: &PgPool) -> TestResult<i64> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT count(*)
         FROM mmr_ledger_entries
         WHERE guild_id = $1 AND source_type = 'DATA_IMPORT_SEED'",
    )
    .bind(GUILD_ID)
    .fetch_one(pool)
    .await?)
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
