use super::*;

const EXTRA_MEMBERS: [&str; 10] = [
    "100000000000000012",
    "100000000000000013",
    "100000000000000014",
    "100000000000000015",
    "100000000000000016",
    "100000000000000017",
    "100000000000000018",
    "100000000000000019",
    "100000000000000020",
    "100000000000000021",
];

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn operators_can_finish_auction_tournaments_without_losing_games_or_mmr() -> TestResult {
    tokio::time::timeout(Duration::from_secs(120), run_finish_flow())
        .await
        .map_err(|_| "auction finish endpoint test exceeded 120 seconds")??;
    Ok(())
}

#[derive(Clone, Copy)]
enum FixtureKind {
    Ten,
    TwentySemifinal,
    TwentyFinal,
    Bidding,
}

async fn run_finish_flow() -> TestResult {
    let server = TestPostgres::start().await?;
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&server.database_url())
        .await?;
    MIGRATOR.run(&pool).await?;
    let app = auction_lineup_test_app(&pool, test_config(server.database_url())?).await?;
    seed_cross_guild_operator_context(&pool).await?;
    let auth_repository = AuthRepository::new(pool.clone());
    for user_id in EXTRA_MEMBERS {
        create_login(
            &auth_repository,
            user_id,
            "20인 참가자",
            &format!("finish-{user_id}-session"),
            &format!("finish-{user_id}-csrf"),
        )
        .await?;
        sqlx::query("INSERT INTO guild_members (guild_id, user_id, display_name, status, first_seen_at, registered_at, last_synced_at) VALUES ($1, $2, '20인 참가자', 'ACTIVE', now(), now(), now())")
            .bind(GUILD_ID).bind(user_id).execute(&pool).await?;
    }
    set_result_channel(&pool, Some(RESULT_CHANNEL_ID)).await?;

    let scenarios: [(&str, i16, &[&str], FixtureKind); 6] = [
        ("bo3-tied", 3, &["BLUE", "RED"], FixtureKind::Ten),
        ("bo5-leading", 5, &["BLUE", "BLUE"], FixtureKind::Ten),
        ("bracket-no-games", 3, &[], FixtureKind::Ten),
        ("bidding", 5, &[], FixtureKind::Bidding),
        (
            "twenty-semifinal-only",
            3,
            &["BLUE"],
            FixtureKind::TwentySemifinal,
        ),
        (
            "twenty-final-leading",
            5,
            &["RED", "RED"],
            FixtureKind::TwentyFinal,
        ),
    ];
    for (index, (name, best_of, winners, kind)) in scenarios.into_iter().enumerate() {
        let fixture =
            seed_finish_fixture(&pool, &app, 60 + i64::try_from(index)?, best_of, kind).await?;
        play_games(&app, fixture.match_id, winners, name).await?;
        let tournament_path = format!(
            "/api/guilds/{GUILD_ID}/auction-tournaments/{}",
            fixture.tournament_id
        );
        let finish_path = format!("{tournament_path}/finish");
        let preview = browser_get(
            &app,
            &format!("{tournament_path}/finish-preview"),
            MEMBER_SESSION,
        )
        .await?;
        assert_eq!(preview.status(), StatusCode::OK, "{name}");
        let preview = body_json(preview).await?;
        assert_eq!(
            preview["data"]["tournament"]["id"],
            fixture.tournament_id.to_string()
        );
        let revision = preview["data"]["tournament"]["revision"]
            .as_i64()
            .ok_or("finish preview missing revision")?;
        let body = json!({"expectedRevision": revision, "reason": "남은 경기 진행 불가"});

        if index == 0 {
            assert_finish_boundaries(&app, &finish_path, fixture.tournament_id, revision).await?;
            // Another tournament advances the same players' MMR before finishing this one.
            let later = seed_finish_fixture(&pool, &app, 70, 3, FixtureKind::Ten).await?;
            play_games(&app, later.match_id, &["BLUE"], "later-mmr-history").await?;
        }
        let preserved_before = preserved_game_state(&pool).await?;
        let completed = operator_mutation(
            &app,
            Method::POST,
            &finish_path,
            &format!("finish-{name}"),
            body.clone(),
        )
        .await?;
        let status = completed.status();
        let completed = body_json(completed).await?;
        assert_eq!(status, StatusCode::OK, "{name}: {completed}");
        assert_eq!(completed["data"]["tournamentStatus"], "COMPLETED");
        assert_eq!(completed["data"]["tournamentRevision"], revision + 1);
        assert_eq!(
            completed["data"]["tournamentId"],
            fixture.tournament_id.to_string()
        );
        let champion = match name {
            "bo5-leading" => Some(fixture.left_team_id),
            "twenty-final-leading" => Some(fixture.right_team_id),
            _ => None,
        };
        assert_eq!(completed["data"]["championTeamId"], json!(champion));
        assert_eq!(preserved_game_state(&pool).await?, preserved_before);

        let result =
            browser_get(&app, &format!("{tournament_path}/result"), MEMBER_SESSION).await?;
        let status = result.status();
        let result = body_json(result).await?;
        assert_eq!(status, StatusCode::OK, "{name}: {result}");
        assert_eq!(result["data"]["tournament"]["status"], "COMPLETED");
        assert_eq!(result["data"]["tournament"]["endedEarly"], true);
        assert_eq!(result["data"]["permissions"]["canRevertResults"], false);
        let matches = result["data"]["matches"]
            .as_array()
            .ok_or("finish result missing matches")?;
        assert!(matches.iter().all(|item| item["status"] == "COMPLETED"));
        assert_eq!(
            matches
                .iter()
                .map(|item| item["games"].as_array().map_or(0, Vec::len))
                .sum::<usize>(),
            winners.len()
                + if matches!(kind, FixtureKind::TwentyFinal) {
                    4
                } else {
                    0
                }
        );
        for item in matches {
            let completed_before_finish = matches!(kind, FixtureKind::TwentyFinal)
                && item["id"] != fixture.match_id.to_string();
            assert_eq!(item["endedEarly"], !completed_before_finish, "{name}");
        }
        let lifecycle = sqlx::query_as::<_, (String, i16, String, i16, bool)>(
            "SELECT tournament.status, tournament.final_best_of,
                    recruitment.status, recruitment.final_best_of,
                    tournament.ended_at IS NOT NULL
             FROM auction_tournaments AS tournament
             JOIN auction_recruitments AS recruitment
               ON recruitment.guild_id = tournament.guild_id
              AND recruitment.id = tournament.source_recruitment_id
             WHERE tournament.guild_id = $1 AND tournament.id = $2",
        )
        .bind(GUILD_ID)
        .bind(fixture.tournament_id)
        .fetch_one(&pool)
        .await?;
        assert_eq!(
            lifecycle,
            (
                "COMPLETED".to_owned(),
                best_of,
                "CONVERTED".to_owned(),
                best_of,
                true
            )
        );

        let replay = operator_mutation(
            &app,
            Method::POST,
            &finish_path,
            &format!("finish-{name}"),
            body,
        )
        .await?;
        assert_eq!(replay.status(), StatusCode::OK);
        assert_eq!(body_json(replay).await?["data"], completed["data"]);
        assert_finish_effects(&pool, fixture.tournament_id).await?;

        let repeated = operator_mutation(
            &app,
            Method::POST,
            &finish_path,
            &format!("finish-again-{name}"),
            json!({"expectedRevision": revision + 1, "reason": "다시 마무리"}),
        )
        .await?;
        assert_public_error(
            repeated,
            StatusCode::CONFLICT,
            "auction_tournament_finish_unavailable",
            "완료되었거나 취소된 경매 내전은 마무리할 수 없습니다.",
        )
        .await?;
        if !winners.is_empty() {
            assert_match_cannot_change(&app, fixture.match_id, name).await?;
        }
        assert_eq!(preserved_game_state(&pool).await?, preserved_before);
    }
    pool.close().await;
    drop(server);
    Ok(())
}

async fn operator_mutation(
    app: &Router,
    method: Method,
    path: &str,
    key: &str,
    body: Value,
) -> TestResult<Response<Body>> {
    browser_mutation(
        app,
        method,
        path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        key,
        true,
        body,
        None,
    )
    .await
}

async fn assert_public_error(
    response: Response<Body>,
    status: StatusCode,
    code: &str,
    message: &str,
) -> TestResult {
    assert_eq!(response.status(), status);
    let body = body_json(response).await?;
    assert_eq!(body["error"]["code"], code);
    assert_eq!(body["error"]["message"], message);
    assert!(
        body["error"]["requestId"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );
    Ok(())
}

async fn assert_finish_boundaries(
    app: &Router,
    path: &str,
    tournament_id: Uuid,
    revision: i64,
) -> TestResult {
    let body = json!({"expectedRevision": revision, "reason": "마무리"});
    let unauthenticated = app
        .clone()
        .oneshot(
            Request::post(path)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&body)?))?,
        )
        .await?;
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);
    let missing_csrf = browser_mutation(
        app,
        Method::POST,
        path,
        MEMBER_SESSION,
        MEMBER_CSRF,
        "finish-missing-csrf",
        false,
        body.clone(),
        None,
    )
    .await?;
    assert_public_error(
        missing_csrf,
        StatusCode::FORBIDDEN,
        "csrf_validation_failed",
        "요청 출처를 확인할 수 없습니다.",
    )
    .await?;
    let non_operator = browser_mutation(
        app,
        Method::POST,
        path,
        CANDIDATE_SESSION,
        CANDIDATE_CSRF,
        "finish-non-operator",
        true,
        body.clone(),
        None,
    )
    .await?;
    assert_public_error(
        non_operator,
        StatusCode::FORBIDDEN,
        "operator_required",
        "경매 내전을 마무리하려면 현재 길드 운영자 권한이 필요합니다.",
    )
    .await?;
    let cross_tenant = operator_mutation(
        app,
        Method::POST,
        &format!(
            "/api/guilds/{}/auction-tournaments/{tournament_id}/finish",
            cross_guild_id()
        ),
        "finish-cross-tenant",
        body,
    )
    .await?;
    assert_public_error(
        cross_tenant,
        StatusCode::NOT_FOUND,
        "auction_tournament_not_found",
        "요청한 경매 내전을 찾을 수 없습니다.",
    )
    .await?;
    let stale = operator_mutation(
        app,
        Method::POST,
        path,
        "finish-stale",
        json!({"expectedRevision": revision + 10, "reason": "마무리"}),
    )
    .await?;
    assert_public_error(
        stale,
        StatusCode::CONFLICT,
        "auction_tournament_revision_conflict",
        "다른 운영자가 경매 내전을 변경했습니다. 최신 상태를 확인해 주세요.",
    )
    .await
}

fn game_body(revision: i64, set_number: usize, winner: &str) -> Value {
    let picks: serde_json::Map<String, Value> = FULL_ROSTER_MEMBER_IDS
        .into_iter()
        .map(|user_id| (user_id.to_owned(), Value::Null))
        .collect();
    json!({
        "expectedRevision": revision,
        "setNumber": set_number,
        "sidePreset": "DEFAULT",
        "winner": winner,
        "picks": picks,
        "bans": {"BLUE": [null, null, null, null, null], "RED": [null, null, null, null, null]},
        "patchVersion": "26.17"
    })
}

async fn lock_match(app: &Router, match_id: Uuid, name: &str) -> TestResult<Value> {
    let path = format!("/api/guilds/{GUILD_ID}/auction-matches/{match_id}");
    let snapshot = browser_get(app, &path, MEMBER_SESSION).await?;
    assert_eq!(snapshot.status(), StatusCode::OK);
    let snapshot = body_json(snapshot).await?;
    if snapshot["data"]["match"]["lineupsLocked"] == true {
        return Ok(snapshot);
    }
    let locked = operator_mutation(
        app,
        Method::POST,
        &format!("{path}/lineup/lock"),
        &format!("lock-{name}"),
        json!({"expectedRevision": snapshot["data"]["match"]["revision"]}),
    )
    .await?;
    let status = locked.status();
    let locked = body_json(locked).await?;
    assert_eq!(status, StatusCode::OK, "{name}: {locked}");
    Ok(locked)
}

async fn play_games(app: &Router, match_id: Uuid, winners: &[&str], name: &str) -> TestResult {
    if winners.is_empty() {
        return Ok(());
    }
    let path = format!("/api/guilds/{GUILD_ID}/auction-matches/{match_id}");
    let locked = lock_match(app, match_id, name).await?;
    let mut revision = locked["data"]["match"]["revision"]
        .as_i64()
        .ok_or("lock revision missing")?;
    let picks: serde_json::Map<String, Value> = locked["data"]["lineups"]
        .as_array()
        .ok_or("lineups missing")?
        .iter()
        .map(|lineup| {
            (
                lineup["userId"].as_str().expect("lineup user").to_owned(),
                Value::Null,
            )
        })
        .collect();
    for (index, winner) in winners.iter().enumerate() {
        let mut body = game_body(revision, index + 1, winner);
        body["picks"] = json!(picks);
        let recorded = operator_mutation(
            app,
            Method::POST,
            &format!("{path}/games"),
            &format!("game-{name}-{index}"),
            body,
        )
        .await?;
        let status = recorded.status();
        let recorded = body_json(recorded).await?;
        assert_eq!(status, StatusCode::CREATED, "{name}: {recorded}");
        revision = recorded["data"]["match"]["revision"]
            .as_i64()
            .ok_or("game revision missing")?;
    }
    Ok(())
}

async fn assert_match_cannot_change(app: &Router, match_id: Uuid, name: &str) -> TestResult {
    let path = format!("/api/guilds/{GUILD_ID}/auction-matches/{match_id}");
    let snapshot = browser_get(app, &path, MEMBER_SESSION).await?;
    assert_eq!(snapshot.status(), StatusCode::OK);
    let snapshot = body_json(snapshot).await?;
    assert_eq!(snapshot["data"]["match"]["status"], "COMPLETED");
    assert_eq!(snapshot["data"]["permissions"]["canRevertLastGame"], false);
    assert_eq!(
        snapshot["data"]["permissions"]["canFinishTournament"],
        false
    );
    let revision = snapshot["data"]["match"]["revision"]
        .as_i64()
        .ok_or("match revision missing")?;
    for (method, suffix, body) in [
        (
            Method::DELETE,
            "games/last",
            json!({"expectedRevision": revision}),
        ),
        (Method::POST, "games", game_body(revision, 3, "BLUE")),
    ] {
        let rejected = operator_mutation(
            app,
            method,
            &format!("{path}/{suffix}"),
            &format!("after-finish-{name}-{suffix}"),
            body,
        )
        .await?;
        assert_eq!(rejected.status(), StatusCode::CONFLICT);
        let rejected = body_json(rejected).await?;
        assert!(
            rejected["error"]["code"]
                .as_str()
                .is_some_and(|code| code.starts_with("auction_"))
        );
        assert!(
            rejected["error"]["message"]
                .as_str()
                .is_some_and(|message| !message.is_empty())
        );
    }
    Ok(())
}

async fn preserved_game_state(pool: &PgPool) -> TestResult<Value> {
    Ok(sqlx::query_scalar(
        "SELECT jsonb_build_object(
            'games', (SELECT jsonb_agg(to_jsonb(game) ORDER BY id) FROM games AS game WHERE guild_id = $1),
            'participants', (SELECT jsonb_agg(to_jsonb(player) ORDER BY game_id, user_id)
                FROM game_participants AS player WHERE guild_id = $1),
            'ledger', (SELECT jsonb_agg(to_jsonb(entry) ORDER BY id) FROM mmr_ledger_entries AS entry WHERE guild_id = $1),
            'mmr', (SELECT jsonb_agg(to_jsonb(mmr) ORDER BY user_id, lane) FROM user_lane_mmr AS mmr WHERE guild_id = $1)
        )",
    )
    .bind(GUILD_ID)
    .fetch_one(pool)
    .await?)
}

async fn assert_finish_effects(pool: &PgPool, tournament_id: Uuid) -> TestResult {
    let counts = sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT
            (SELECT count(*) FROM admin_audit_log WHERE guild_id = $1 AND target_id = $2
                AND action = 'AUCTION_TOURNAMENT_FINISHED' AND after_summary ->> 'reason' = '남은 경기 진행 불가'),
            (SELECT count(*) FROM outbox_events WHERE guild_id = $1 AND aggregate_id = $2
                AND event_type = 'AUCTION_TOURNAMENT_FINISHED'),
            (SELECT count(*) FROM bot_notification_outbox WHERE guild_id = $1
                AND payload ->> 'tournamentId' = $2 AND notification_type = 'AUCTION_TOURNAMENT_RESULT_CREATE')",
    )
    .bind(GUILD_ID)
    .bind(tournament_id.to_string())
    .fetch_one(pool)
    .await?;
    assert_eq!(counts.0, 1);
    assert!(counts.1 > 0);
    assert_eq!(counts.2, 1);
    Ok(())
}

async fn seed_finish_fixture(
    pool: &PgPool,
    app: &Router,
    display_number: i64,
    best_of: i16,
    kind: FixtureKind,
) -> TestResult<AuctionMatchLineupFixture> {
    let fixture = seed_complete_auction_match_lineup(pool, display_number).await?;
    let format = if matches!(
        kind,
        FixtureKind::TwentySemifinal | FixtureKind::TwentyFinal
    ) {
        "TWENTY_PERSON"
    } else {
        "TEN_PERSON"
    };
    sqlx::query(
        "UPDATE auction_recruitments SET format = $3, final_best_of = $4,
            target_count = CASE WHEN $3 = 'TWENTY_PERSON' THEN 20 ELSE 10 END,
            semifinal_best_of = CASE WHEN $3 = 'TWENTY_PERSON' THEN 3 ELSE NULL END
         WHERE guild_id = $1 AND id = (
             SELECT source_recruitment_id FROM auction_tournaments WHERE guild_id = $1 AND id = $2
         )",
    )
    .bind(GUILD_ID)
    .bind(fixture.tournament_id)
    .bind(format)
    .bind(best_of)
    .execute(pool)
    .await?;
    sqlx::query(
        "UPDATE auction_tournaments SET format = $3, final_best_of = $4,
            semifinal_best_of = CASE WHEN $3 = 'TWENTY_PERSON' THEN 3 ELSE NULL END,
            status = $5
         WHERE guild_id = $1 AND id = $2",
    )
    .bind(GUILD_ID)
    .bind(fixture.tournament_id)
    .bind(format)
    .bind(best_of)
    .bind(if matches!(kind, FixtureKind::Bidding) {
        "BIDDING"
    } else {
        "BRACKET_SETUP"
    })
    .execute(pool)
    .await?;
    sqlx::query("UPDATE auction_matches SET best_of = $3 WHERE guild_id = $1 AND id = $2")
        .bind(GUILD_ID)
        .bind(fixture.match_id)
        .bind(best_of)
        .execute(pool)
        .await?;
    if matches!(kind, FixtureKind::Bidding) {
        sqlx::query("DELETE FROM auction_match_lineups WHERE guild_id = $1 AND match_id = $2")
            .bind(GUILD_ID)
            .bind(fixture.match_id)
            .execute(pool)
            .await?;
        sqlx::query("DELETE FROM auction_matches WHERE guild_id = $1 AND id = $2")
            .bind(GUILD_ID)
            .bind(fixture.match_id)
            .execute(pool)
            .await?;
    } else if matches!(
        kind,
        FixtureKind::TwentySemifinal | FixtureKind::TwentyFinal
    ) {
        seed_remaining_twenty_matches(pool, app, fixture, kind).await?;
    }
    Ok(fixture)
}

async fn seed_remaining_twenty_matches(
    pool: &PgPool,
    app: &Router,
    fixture: AuctionMatchLineupFixture,
    kind: FixtureKind,
) -> TestResult {
    let third_team = Uuid::now_v7();
    let fourth_team = Uuid::now_v7();
    for (team_id, team_index) in [(third_team, 3_i16), (fourth_team, 4_i16)] {
        sqlx::query(
            "INSERT INTO auction_teams (guild_id, tournament_id, id, team_index, team_name,
                initial_points, current_points, point_revision, created_at)
             VALUES ($1, $2, $3, $4, '준결승 팀', 1000, 1000, 0, now())",
        )
        .bind(GUILD_ID)
        .bind(fixture.tournament_id)
        .bind(team_id)
        .bind(team_index)
        .execute(pool)
        .await?;
    }
    let semifinal_only = matches!(kind, FixtureKind::TwentySemifinal);
    let semifinal_a = if semifinal_only {
        fixture.match_id
    } else {
        Uuid::now_v7()
    };
    let semifinal_b = Uuid::now_v7();
    let other_matches = if semifinal_only {
        sqlx::query(
            "UPDATE auction_matches SET match_key = 'SEMIFINAL_A' WHERE guild_id = $1 AND id = $2",
        )
        .bind(GUILD_ID)
        .bind(fixture.match_id)
        .execute(pool)
        .await?;
        [
            (
                semifinal_b,
                "SEMIFINAL_B",
                Some(third_team),
                Some(fourth_team),
            ),
            (Uuid::now_v7(), "FINAL", None, None),
        ]
    } else {
        [
            (
                semifinal_a,
                "SEMIFINAL_A",
                Some(fixture.left_team_id),
                Some(third_team),
            ),
            (
                semifinal_b,
                "SEMIFINAL_B",
                Some(fixture.right_team_id),
                Some(fourth_team),
            ),
        ]
    };
    for (match_id, key, left, right) in other_matches {
        sqlx::query(
            "INSERT INTO auction_matches (guild_id, tournament_id, id, season_id, match_key,
                best_of, hard_fearless, left_team_id, right_team_id, status, revision, updated_by, updated_at)
             VALUES ($1, $2, $3, $4, $5, 3, true, $6, $7, 'LINEUP_PENDING', 0, $8, now())",
        ).bind(GUILD_ID).bind(fixture.tournament_id).bind(match_id).bind(Uuid::parse_str(SEASON_ID)?)
            .bind(key).bind(left).bind(right).bind(MEMBER_ID).execute(pool).await?;
    }
    let third_team_match = if semifinal_only {
        semifinal_b
    } else {
        semifinal_a
    };
    seed_auction_team_lineup(
        pool,
        fixture.tournament_id,
        third_team_match,
        third_team,
        2,
        &EXTRA_MEMBERS[..5],
    )
    .await?;
    seed_auction_team_lineup(
        pool,
        fixture.tournament_id,
        semifinal_b,
        fourth_team,
        3,
        &EXTRA_MEMBERS[5..],
    )
    .await?;
    if !semifinal_only {
        for (team_id, semifinal_id) in [
            (fixture.left_team_id, semifinal_a),
            (fixture.right_team_id, semifinal_b),
        ] {
            sqlx::query("UPDATE auction_match_lineups SET match_id = $3 WHERE guild_id = $1 AND match_id = $2 AND team_id = $4")
                .bind(GUILD_ID).bind(fixture.match_id).bind(semifinal_id).bind(team_id).execute(pool).await?;
        }
        sqlx::query("UPDATE auction_matches SET left_team_id = NULL, right_team_id = NULL WHERE guild_id = $1 AND id = $2")
            .bind(GUILD_ID).bind(fixture.match_id).execute(pool).await?;
    }
    let name_a = format!("{}-semi-a", fixture.tournament_id);
    let name_b = format!("{}-semi-b", fixture.tournament_id);
    lock_match(app, semifinal_a, &name_a).await?;
    lock_match(app, semifinal_b, &name_b).await?;
    if !semifinal_only {
        play_games(app, semifinal_a, &["BLUE", "BLUE"], &name_a).await?;
        play_games(app, semifinal_b, &["BLUE", "BLUE"], &name_b).await?;
    }
    Ok(())
}
