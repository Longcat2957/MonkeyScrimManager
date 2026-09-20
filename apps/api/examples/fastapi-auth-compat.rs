//! Test-only interop oracle using the existing Rust implementation and a fixed, public zero key.
//! Not included in runtime images. Input/output material must only be used in isolated tests.
use std::{
    error::Error,
    io::{self, Read},
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use monkeybot_api::auth::{AuthService, CookiePolicy};
use secrecy::ExposeSecret;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
enum Input {
    AuctionTournament {
        guild_id: String,
        user_id: String,
        tournament_id: Option<Uuid>,
        action: String,
        key: String,
        body: serde_json::Value,
    },
    AuctionRecruitment {
        guild_id: String,
        user_id: String,
        recruitment_id: Option<Uuid>,
        action: String,
        key: String,
        body: serde_json::Value,
        #[serde(default)]
        bot: bool,
    },
    GuildAdmin {
        guild_id: String,
        user_id: String,
        action: String,
        body: serde_json::Value,
        key: Option<String>,
    },
    Notification {
        action: String,
        body: serde_json::Value,
    },
    RecruitmentLifecycle {
        guild_id: String,
        user_id: String,
        recruitment_id: Uuid,
        action: String,
        key: String,
        expected_revision: i64,
        entry_operation: Option<serde_json::Value>,
        bot_actor: Option<serde_json::Value>,
        lanes: Option<Vec<String>>,
    },
    RecruitmentMember {
        guild_id: String,
        user_id: String,
        member_id: String,
        recruitment_id: Uuid,
        key: String,
        joining: bool,
    },
    RecruitmentCreate {
        guild_id: String,
        user_id: String,
        key: String,
        start: String,
        best_of: i16,
        hard_fearless: bool,
    },
    Material,
    Decrypt {
        attempt_id: Uuid,
        ciphertext: String,
    },
    Hash {
        value: String,
    },
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let auth = AuthService::new(
        &[0_u8; 32],
        CookiePolicy::new(false, Duration::from_secs(3600)),
    )?;
    let output = match serde_json::from_str::<Input>(&input)? {
        Input::AuctionTournament {
            guild_id,
            user_id,
            tournament_id,
            action,
            key,
            body,
        } => {
            use monkeybot_api::{
                domain::{AuctionTournamentId, GuildId, UserId},
                repository::bot_workflow as w,
                state::RequestId,
            };
            let url = std::env::var("FASTAPI_TEST_RUNTIME_URL")?;
            if !url.contains("@127.0.0.1:") || !url.contains("/monkeybot_fastapi_test?") {
                return Err("Tournament oracle requires isolated test database".into());
            }
            tokio::runtime::Runtime::new()?.block_on(async {
                let pool = sqlx::PgPool::connect(&url).await?;
                let guild = GuildId::parse(guild_id)?;
                let user = UserId::parse(user_id)?;
                let id = AuctionTournamentId::from_uuid(tournament_id.unwrap_or_else(Uuid::now_v7));
                let request = RequestId(Uuid::now_v7().to_string());
                let value = match action.as_str() {
                    "create" => serde_json::to_value(
                        w::create_web_auction_tournament(
                            &pool,
                            &guild,
                            &user,
                            serde_json::from_value(body["sourceRecruitmentId"].clone())?,
                            body["expectedRevision"].as_i64().ok_or("revision")?,
                            &key,
                            &request,
                        )
                        .await?,
                    )?,
                    "detail" => serde_json::to_value(
                        w::get_auction_tournament_draft_snapshot(&pool, &guild, id, &user).await?,
                    )?,
                    "command" => serde_json::to_value(
                        w::execute_web_auction_tournament_draft_command(
                            &pool,
                            &guild,
                            id,
                            &user,
                            serde_json::from_value(body)?,
                            &key,
                            &request,
                        )
                        .await?,
                    )?,
                    _ => return Err("unknown tournament oracle action".into()),
                };
                pool.close().await;
                Ok::<_, Box<dyn Error>>(value)
            })?
        }
        Input::AuctionRecruitment {
            guild_id,
            user_id,
            recruitment_id,
            action,
            key,
            body,
            bot,
        } => {
            use monkeybot_api::{
                domain::{AuctionRecruitmentId, DiscordInteractionId, GuildId, UserId},
                repository::{bot_workflow as w, workflow::AuctionFormat},
                state::RequestId,
            };
            let url = std::env::var("FASTAPI_TEST_RUNTIME_URL")?;
            if !url.contains("@127.0.0.1:") || !url.contains("/monkeybot_fastapi_test?") {
                return Err("Auction oracle requires isolated test database".into());
            }
            tokio::runtime::Runtime::new()?.block_on(async {
                let pool = sqlx::PgPool::connect(&url).await?;
                let guild = GuildId::parse(guild_id)?;
                let user = UserId::parse(user_id)?;
                let id =
                    AuctionRecruitmentId::from_uuid(recruitment_id.unwrap_or_else(Uuid::now_v7));
                let request = RequestId(Uuid::now_v7().to_string());
                let expected = body["expectedRevision"].as_i64().unwrap_or_default();
                let value = if action == "create" {
                    let format: AuctionFormat = serde_json::from_value(body["format"].clone())?;
                    let rules = w::AuctionRecruitmentSeriesRules::from_optional(
                        format,
                        body["semifinalBestOf"].as_i64().map(|x| x as i16),
                        body["finalBestOf"].as_i64().map(|x| x as i16),
                        body["hardFearless"].as_bool(),
                    );
                    let start = time::OffsetDateTime::parse(
                        body["expectedStartAt"].as_str().ok_or("start")?,
                        &time::format_description::well_known::Rfc3339,
                    )?;
                    serde_json::to_value(if bot {
                        w::execute_auction_recruitment_command(
                            &pool,
                            &guild,
                            &DiscordInteractionId::parse(key)?,
                            &request,
                            serde_json::from_value(body["actor"].clone())?,
                            format,
                            rules,
                            start,
                        )
                        .await?
                    } else {
                        w::create_web_auction_recruitment(
                            &pool, &guild, &user, format, rules, start, &key, &request,
                        )
                        .await?
                    })?
                } else if bot {
                    let actor: w::BotActor = serde_json::from_value(body["actor"].clone())?;
                    let command = if action == "join" {
                        w::BotAuctionRecruitmentParticipationCommand::Join {
                            participant: if body["member"].is_null() {
                                actor.clone()
                            } else {
                                serde_json::from_value(body["member"].clone())?
                            },
                            actor,
                            recruitment_id: id,
                            declared_lane: serde_json::from_value(body["declaredLane"].clone())?,
                        }
                    } else {
                        w::BotAuctionRecruitmentParticipationCommand::Leave {
                            actor,
                            recruitment_id: id,
                            user_id: serde_json::from_value(body["memberId"].clone())?,
                        }
                    };
                    serde_json::to_value(
                        w::execute_auction_recruitment_participation_command(
                            &pool,
                            &guild,
                            &DiscordInteractionId::parse(key)?,
                            &request,
                            command,
                        )
                        .await?,
                    )?
                } else {
                    match action.as_str() {
                        "detail" => serde_json::to_value(
                            w::get_auction_recruitment_snapshot(&pool, &guild, id, &user).await?,
                        )?,
                        "draft" => serde_json::to_value(
                            w::get_auction_setup_draft_snapshot(&pool, &guild, id, &user).await?,
                        )?,
                        "join" => serde_json::to_value(
                            w::add_web_auction_recruitment_member(
                                &pool,
                                &guild,
                                id,
                                &user,
                                serde_json::from_value(body["memberId"].clone())?,
                                serde_json::from_value(body["declaredLane"].clone())?,
                                &key,
                                &request,
                            )
                            .await?,
                        )?,
                        "leave" => serde_json::to_value(
                            w::remove_web_auction_recruitment_member(
                                &pool,
                                &guild,
                                id,
                                &user,
                                serde_json::from_value(body["memberId"].clone())?,
                                &key,
                                &request,
                            )
                            .await?,
                        )?,
                        "close" => serde_json::to_value(
                            w::close_web_auction_recruitment(
                                &pool, &guild, id, &user, &key, &request,
                            )
                            .await?,
                        )?,
                        "reopen" => serde_json::to_value(
                            w::reopen_web_auction_recruitment(
                                &pool, &guild, id, &user, expected, &key, &request,
                            )
                            .await?,
                        )?,
                        "cancel" => serde_json::to_value(
                            w::cancel_web_auction_recruitment(
                                &pool, &guild, id, &user, expected, &key, &request,
                            )
                            .await?,
                        )?,
                        "restore" => serde_json::to_value(
                            w::restore_web_auction_recruitment(
                                &pool, &guild, id, &user, expected, &key, &request,
                            )
                            .await?,
                        )?,
                        "setup" => serde_json::to_value(
                            w::update_web_auction_setup_draft(
                                &pool,
                                &guild,
                                id,
                                &user,
                                expected,
                                serde_json::from_value(body["teams"].clone())?,
                                &key,
                                &request,
                            )
                            .await?,
                        )?,
                        "dm" => serde_json::to_value(
                            w::dispatch_web_auction_recruitment_participant_confirmation_dms(
                                &pool, &guild, id, &user, &key, &request,
                            )
                            .await?,
                        )?,
                        _ => return Err("unknown auction action".into()),
                    }
                };
                pool.close().await;
                Ok::<_, Box<dyn Error>>(value)
            })?
        }
        Input::GuildAdmin {
            guild_id,
            user_id,
            action,
            body,
            key,
        } => {
            use monkeybot_api::{
                domain::{GuildId, UserId},
                repository::{guild_admin_settings as settings, guild_seasons as seasons},
            };
            let url = std::env::var("FASTAPI_TEST_RUNTIME_URL")?;
            if !url.contains("@127.0.0.1:") || !url.contains("/monkeybot_fastapi_test?") {
                return Err("Guild admin oracle requires isolated test database".into());
            }
            tokio::runtime::Runtime::new()?.block_on(async {
                let pool = sqlx::PgPool::connect(&url).await?;
                let guild = GuildId::parse(guild_id)?;
                let user = UserId::parse(user_id)?;
                let request = Uuid::now_v7().to_string();
                let value = match action.as_str() {
                    "settings" => serde_json::to_value(settings::get(&pool, &guild, &user).await?)?,
                    "update" => serde_json::to_value(
                        settings::update(
                            &pool,
                            &guild,
                            &user,
                            &request,
                            serde_json::from_value(body)?,
                        )
                        .await?,
                    )?,
                    "catalog" => serde_json::to_value(
                        settings::replace_discord_catalog(
                            &pool,
                            &guild,
                            serde_json::from_value(body)?,
                        )
                        .await?,
                    )?,
                    "seasons" => serde_json::to_value(seasons::get(&pool, &guild, &user).await?)?,
                    "rotate" => serde_json::to_value(
                        seasons::rotate(
                            &pool,
                            &guild,
                            &user,
                            key.as_deref().ok_or("key required")?,
                            &request,
                            serde_json::from_value(body)?,
                        )
                        .await?,
                    )?,
                    _ => return Err("unknown guild admin action".into()),
                };
                pool.close().await;
                Ok::<_, Box<dyn Error>>(value)
            })?
        }
        Input::Notification { action, body } => {
            use monkeybot_api::{
                domain::{DiscordMessageId, NotificationId},
                repository::notification_outbox as outbox,
            };
            let url = std::env::var("FASTAPI_TEST_RUNTIME_URL")?;
            if !url.contains("@127.0.0.1:") || !url.contains("/monkeybot_fastapi_test?") {
                return Err("Notification oracle requires isolated test database".into());
            }
            tokio::runtime::Runtime::new()?.block_on(async {
                let pool = sqlx::PgPool::connect(&url).await?;
                let mut transaction = pool.begin().await?;
                let owner = body["instanceId"].as_str().ok_or("instanceId required")?;
                let result = if action == "claim" {
                    let rows = outbox::claim(&mut transaction, &outbox::ClaimNotifications {
                        lease_owner: owner,
                        lease_duration: outbox::DEFAULT_LEASE_DURATION,
                        limit: body["limit"].as_u64().unwrap_or(20).try_into()?,
                    }).await?;
                    let rows = rows.into_iter().map(|row| Ok::<_, Box<dyn Error>>(json!({
                        "id": row.id, "guildId": row.guild_id, "leaseToken": row.lease.token,
                        "leaseUntil": row.lease.until.format(&time::format_description::well_known::Rfc3339)?,
                        "channelId": row.channel_id, "type": row.notification_type,
                        "payload": row.payload, "discordNonce": row.discord_nonce, "attempts": row.attempts,
                    }))).collect::<Result<Vec<_>, _>>()?;
                    json!({"notifications": rows})
                } else {
                    let id: NotificationId = serde_json::from_value(body["notificationId"].clone())?;
                    let token: Uuid = serde_json::from_value(body["leaseToken"].clone())?;
                    if action == "nack" {
                        let failure = &body["failure"];
                        let kind = match failure["kind"].as_str() {
                            Some("HTTP") => outbox::NotificationFailureKind::Http,
                            Some("TRANSPORT") => outbox::NotificationFailureKind::Transport,
                            _ => outbox::NotificationFailureKind::DeliveryUnknown,
                        };
                        let outcome = outbox::nack(&mut transaction, &outbox::NackNotification {
                            notification_id: id, lease_owner: owner, lease_token: token,
                            failure: outbox::NotificationFailure {
                                kind, status: failure["status"].as_u64().map(|n| n as u16),
                                retry_after: failure["retryAfterMs"].as_u64().map(Duration::from_millis),
                                message: failure["message"].as_str().ok_or("message required")?,
                            }
                        }).await?;
                        match outcome {
                            outbox::NackNotificationOutcome::Failed { failure_code, attempts } => json!({
                                "state": "FAILED", "failureCode": failure_code, "attempts": attempts,
                            }),
                            outbox::NackNotificationOutcome::RetryScheduled { available_at, attempts } => json!({
                                "state": "RETRY_SCHEDULED", "attempts": attempts,
                                "availableAt": available_at.format(&time::format_description::well_known::Rfc3339)?,
                            }),
                        }
                    } else {
                        let message: DiscordMessageId = serde_json::from_value(body["discordMessageId"].clone())?;
                        let request = outbox::AckNotification {
                            notification_id: id, lease_owner: owner, lease_token: token, discord_message_id: &message,
                        };
                        let (row, binding) = match action.as_str() {
                            "recruitment-message/ack" => {
                                let value = outbox::ack_recruitment_message(&mut transaction, &request).await?;
                                (value.notification, json!({"recruitment": {
                                    "id": value.recruitment_id, "revision": value.recruitment_revision,
                                }}))
                            },
                            "auction-recruitment-message/ack" => {
                                let value = outbox::ack_auction_recruitment_message(&mut transaction, &request).await?;
                                (value.notification, json!({"auctionRecruitment": {
                                    "id": value.auction_recruitment_id, "revision": value.auction_recruitment_revision,
                                }}))
                            },
                            _ => (outbox::ack(&mut transaction, &request).await?, json!({})),
                        };
                        let mut result = json!({"acknowledged": true,
                            "deliveredAt": row.delivered_at.format(&time::format_description::well_known::Rfc3339)?});
                        result.as_object_mut().unwrap().extend(binding.as_object().unwrap().clone());
                        result
                    }
                };
                transaction.commit().await?;
                pool.close().await;
                Ok::<_, Box<dyn Error>>(result)
            })?
        }
        Input::RecruitmentLifecycle {
            guild_id,
            user_id,
            recruitment_id,
            action,
            key,
            expected_revision,
            entry_operation,
            bot_actor,
            lanes,
        } => {
            use monkeybot_api::{
                domain::{GuildId, RecruitmentId, UserId},
                repository::bot_workflow as workflow,
                state::RequestId,
            };
            let url = std::env::var("FASTAPI_TEST_RUNTIME_URL")?;
            if !url.contains("@127.0.0.1:") || !url.contains("/monkeybot_fastapi_test?") {
                return Err("Recruitment oracle requires isolated test database".into());
            }
            tokio::runtime::Runtime::new()?.block_on(async {
                let pool = sqlx::PgPool::connect(&url).await?;
                let guild = GuildId::parse(guild_id)?;
                let user = UserId::parse(user_id)?;
                let id = RecruitmentId::from_uuid(recruitment_id);
                let request = RequestId(Uuid::now_v7().to_string());
                let result = match action.as_str() {
                    "close" => serde_json::to_value(
                        workflow::close_web_recruitment(&pool, &guild, id, &user, &key, &request)
                            .await?,
                    )?,
                    "cancel" => serde_json::to_value(
                        workflow::cancel_web_recruitment(
                            &pool,
                            &guild,
                            id,
                            &user,
                            expected_revision,
                            &key,
                            &request,
                        )
                        .await?,
                    )?,
                    "restore" => serde_json::to_value(
                        workflow::restore_web_recruitment(
                            &pool,
                            &guild,
                            id,
                            &user,
                            expected_revision,
                            &key,
                            &request,
                        )
                        .await?,
                    )?,
                    "reopen" => serde_json::to_value(
                        workflow::reopen_web_recruitment(
                            &pool,
                            &guild,
                            id,
                            &user,
                            expected_revision,
                            &key,
                            &request,
                        )
                        .await?,
                    )?,
                    "apply" => serde_json::to_value(
                        workflow::update_web_entry_draft(
                            &pool,
                            &guild,
                            id,
                            &user,
                            expected_revision,
                            serde_json::from_value(entry_operation.ok_or("operation required")?)?,
                            &key,
                            &request,
                        )
                        .await?,
                    )?,
                    "reset" => serde_json::to_value(
                        workflow::reset_web_entry_draft(
                            &pool,
                            &guild,
                            id,
                            &user,
                            expected_revision,
                            &key,
                            &request,
                        )
                        .await?,
                    )?,
                    "lanes" => serde_json::to_value(
                        workflow::execute_recruitment_command(
                            &pool,
                            &guild,
                            &key.parse()?,
                            &request,
                            workflow::BotRecruitmentCommand::SetPreferredLanes {
                                actor: serde_json::from_value(bot_actor.ok_or("actor required")?)?,
                                recruitment_id: id,
                                user_id: user,
                                lanes: lanes
                                    .ok_or("lanes required")?
                                    .into_iter()
                                    .map(|lane| lane.parse())
                                    .collect::<Result<Vec<_>, _>>()?,
                            },
                        )
                        .await?,
                    )?,
                    "dm" => serde_json::to_value(
                        workflow::dispatch_web_recruitment_participant_confirmation_dms(
                            &pool, &guild, id, &user, &key, &request,
                        )
                        .await?,
                    )?,
                    "convert" => {
                        let body = entry_operation.ok_or("conversion body required")?;
                        let start = time::OffsetDateTime::parse(
                            body["expectedStartAt"].as_str().ok_or("start required")?,
                            &time::format_description::well_known::Rfc3339,
                        )?;
                        serde_json::to_value(
                            workflow::convert_web_entry_draft_to_auction_recruitment(
                                &pool,
                                &guild,
                                &user,
                                id,
                                expected_revision,
                                start,
                                &key,
                                &request,
                            )
                            .await?,
                        )?
                    }
                    "series" => serde_json::to_value(
                        workflow::create_web_series(
                            &pool,
                            &guild,
                            &user,
                            id,
                            expected_revision,
                            &key,
                            &request,
                        )
                        .await?,
                    )?,
                    "bind" => {
                        let binding = entry_operation.ok_or("binding required")?;
                        serde_json::to_value(
                            workflow::execute_recruitment_command(
                                &pool,
                                &guild,
                                &key.parse()?,
                                &request,
                                workflow::BotRecruitmentCommand::BindMessage {
                                    actor: serde_json::from_value(
                                        bot_actor.ok_or("actor required")?,
                                    )?,
                                    recruitment_id: id,
                                    channel_id: binding["channelId"]
                                        .as_str()
                                        .ok_or("channel required")?
                                        .parse()?,
                                    message_id: binding["messageId"]
                                        .as_str()
                                        .ok_or("message required")?
                                        .parse()?,
                                },
                            )
                            .await?,
                        )?
                    }
                    "series_detail" => serde_json::to_value(
                        workflow::get_series_pick_ban_snapshot(
                            &pool,
                            &guild,
                            monkeybot_api::domain::SeriesId::from_uuid(recruitment_id),
                            &user,
                        )
                        .await?,
                    )?,
                    "series_result" => serde_json::to_value(
                        workflow::get_series_result_snapshot(
                            &pool,
                            &guild,
                            monkeybot_api::domain::SeriesId::from_uuid(recruitment_id),
                            &user,
                        )
                        .await?,
                    )?,
                    "series_pickban" => serde_json::to_value(
                        workflow::update_web_series_pick_ban(
                            &pool,
                            &guild,
                            monkeybot_api::domain::SeriesId::from_uuid(recruitment_id),
                            &user,
                            expected_revision,
                            serde_json::from_value(entry_operation.ok_or("operation required")?)?,
                            &request,
                        )
                        .await?,
                    )?,
                    "series_submit" => serde_json::to_value(
                        workflow::submit_web_series_game(
                            &pool,
                            &guild,
                            monkeybot_api::domain::SeriesId::from_uuid(recruitment_id),
                            &user,
                            &key,
                            &request,
                            serde_json::from_value(entry_operation.ok_or("body required")?)?,
                        )
                        .await?,
                    )?,
                    "series_last" => serde_json::to_value(
                        workflow::revert_last_web_series_game(
                            &pool,
                            &guild,
                            monkeybot_api::domain::SeriesId::from_uuid(recruitment_id),
                            &user,
                            &key,
                            &request,
                            workflow::SeriesGameRevertCommand { expected_revision },
                        )
                        .await?,
                    )?,
                    "series_complete" => serde_json::to_value(
                        workflow::complete_web_series_early(
                            &pool,
                            &guild,
                            monkeybot_api::domain::SeriesId::from_uuid(recruitment_id),
                            &user,
                            &key,
                            &request,
                            serde_json::from_value(entry_operation.ok_or("body required")?)?,
                        )
                        .await?,
                    )?,
                    "series_revert" => serde_json::to_value(
                        workflow::revert_web_series(
                            &pool,
                            &guild,
                            monkeybot_api::domain::SeriesId::from_uuid(recruitment_id),
                            &user,
                            expected_revision,
                            &key,
                            &request,
                        )
                        .await?,
                    )?,
                    "series_delete" => serde_json::to_value(
                        workflow::delete_web_series(
                            &pool,
                            &guild,
                            monkeybot_api::domain::SeriesId::from_uuid(recruitment_id),
                            &user,
                            expected_revision,
                            &key,
                            &request,
                        )
                        .await?,
                    )?,
                    "entry" => serde_json::to_value(
                        workflow::get_entry_draft_snapshot(&pool, &guild, id, &user).await?,
                    )?,
                    _ => return Err("Unknown lifecycle oracle action".into()),
                };
                pool.close().await;
                Ok::<_, Box<dyn Error>>(result)
            })?
        }
        Input::RecruitmentMember {
            guild_id,
            user_id,
            member_id,
            recruitment_id,
            key,
            joining,
        } => {
            use monkeybot_api::{
                domain::{GuildId, RecruitmentId, UserId},
                repository::bot_workflow,
                state::RequestId,
            };
            let url = std::env::var("FASTAPI_TEST_RUNTIME_URL")?;
            if !url.contains("@127.0.0.1:") || !url.contains("/monkeybot_fastapi_test?") {
                return Err("Recruitment oracle requires isolated test database".into());
            }
            tokio::runtime::Runtime::new()?.block_on(async {
                let pool = sqlx::PgPool::connect(&url).await?;
                let guild = GuildId::parse(guild_id)?;
                let user = UserId::parse(user_id)?;
                let member = UserId::parse(member_id)?;
                let recruitment = RecruitmentId::from_uuid(recruitment_id);
                let request = RequestId(Uuid::now_v7().to_string());
                let result = if joining {
                    bot_workflow::add_web_recruitment_member(
                        &pool,
                        &guild,
                        recruitment,
                        &user,
                        member,
                        &key,
                        &request,
                    )
                    .await?
                } else {
                    bot_workflow::remove_web_recruitment_member(
                        &pool,
                        &guild,
                        recruitment,
                        &user,
                        member,
                        &key,
                        &request,
                    )
                    .await?
                };
                pool.close().await;
                Ok::<_, Box<dyn Error>>(serde_json::to_value(result)?)
            })?
        }
        Input::RecruitmentCreate {
            guild_id,
            user_id,
            key,
            start,
            best_of,
            hard_fearless,
        } => {
            use monkeybot_api::{
                domain::{GuildId, UserId},
                repository::bot_workflow,
                state::RequestId,
            };
            let url = std::env::var("FASTAPI_TEST_RUNTIME_URL")?;
            if !url.contains("@127.0.0.1:") || !url.contains("/monkeybot_fastapi_test?") {
                return Err("Recruitment oracle requires isolated test database".into());
            }
            tokio::runtime::Runtime::new()?.block_on(async {
                let pool = sqlx::PgPool::connect(&url).await?;
                let result = bot_workflow::create_web_recruitment(
                    &pool,
                    &GuildId::parse(guild_id)?,
                    &UserId::parse(user_id)?,
                    bot_workflow::RecruitmentSeriesRules {
                        best_of,
                        hard_fearless,
                    },
                    time::OffsetDateTime::parse(
                        &start,
                        &time::format_description::well_known::Rfc3339,
                    )?,
                    &key,
                    &RequestId(Uuid::now_v7().to_string()),
                )
                .await?;
                pool.close().await;
                Ok::<_, Box<dyn Error>>(serde_json::to_value(result)?)
            })?
        }
        Input::Material => {
            let attempt_id = Uuid::now_v7();
            let material = auth.begin_oauth(attempt_id)?;
            let verifier = auth.decrypt_pkce(attempt_id, &material.pkce_verifier_ciphertext)?;
            let session = auth.new_session()?;
            json!({
                "attemptId": attempt_id,
                "state": material.state.expose_secret(),
                "stateHash": URL_SAFE_NO_PAD.encode(material.state_hash),
                "verifier": verifier.expose_secret(),
                "challenge": material.pkce_challenge,
                "ciphertext": URL_SAFE_NO_PAD.encode(material.pkce_verifier_ciphertext),
                "token": session.token.expose_secret(),
                "tokenHash": URL_SAFE_NO_PAD.encode(session.token_hash),
                "csrf": session.csrf_token.expose_secret(),
                "csrfHash": URL_SAFE_NO_PAD.encode(session.csrf_secret_hash),
            })
        }
        Input::Decrypt {
            attempt_id,
            ciphertext,
        } => {
            let verifier = auth.decrypt_pkce(attempt_id, &URL_SAFE_NO_PAD.decode(ciphertext)?)?;
            json!({"verifier": verifier.expose_secret()})
        }
        Input::Hash { value } => {
            json!({"hash": URL_SAFE_NO_PAD.encode(AuthService::hash_secret(&value))})
        }
    };
    println!("{output}");
    Ok(())
}
