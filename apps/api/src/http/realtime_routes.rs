use std::{collections::HashSet, sync::Arc, time::Duration};

use axum::{
    Extension,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use tokio::{sync::broadcast, time::Instant};

use crate::{
    error::ApiError,
    http::extractors::AuthenticatedSession,
    realtime::{
        ClientFrame, RealtimeRuntime, RealtimeTopic, RealtimeTopicError, ServerControlFrame,
        TopicAuthorizationError, parsed_topics,
    },
    repository::auth::AuthRepository,
    state::{AppState, RequestId},
};

const MAX_FRAME_BYTES: usize = 8 * 1024;
const PING_INTERVAL: Duration = Duration::from_secs(25);
const IDLE_TIMEOUT: Duration = Duration::from_secs(75);
const SUBSCRIPTION_RECHECK_INTERVAL: Duration = Duration::from_secs(60);

/// The session extractor intentionally precedes `WebSocketUpgrade`: an unauthenticated upgrade
/// must produce the normal HTTP error envelope instead of an opaque upgrade failure.
pub async fn websocket(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    principal: AuthenticatedSession,
    upgrade: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    require_same_origin(&headers, &state, &request_id)?;
    let runtime = state.realtime().cloned().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            &request_id,
            "realtime_unavailable",
            "실시간 편집 서비스를 일시적으로 사용할 수 없습니다.",
            None,
        )
    })?;
    let pool = state.postgres().cloned().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            &request_id,
            "database_unavailable",
            "데이터베이스를 일시적으로 사용할 수 없습니다.",
            None,
        )
    })?;
    let user_id = principal.session.user_id;
    let token_hash = principal.token_hash;
    let auth_repository = state
        .auth()
        .expect("authenticated session requires an auth runtime")
        .repository()
        .clone();

    Ok(upgrade
        .max_frame_size(MAX_FRAME_BYTES)
        .max_message_size(MAX_FRAME_BYTES)
        .on_upgrade(move |socket| {
            serve_socket(socket, runtime, pool, user_id, auth_repository, token_hash)
        })
        .into_response())
}

fn require_same_origin(
    headers: &HeaderMap,
    state: &AppState,
    request_id: &RequestId,
) -> Result<(), ApiError> {
    let mut values = headers.get_all(header::ORIGIN).iter();
    let origin = values.next().and_then(|value| value.to_str().ok());
    let one_origin = origin.is_some() && values.next().is_none();
    if one_origin && origin == Some(state.config().public_origin_header().as_str()) {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::FORBIDDEN,
            request_id,
            "origin_validation_failed",
            "요청 출처를 확인할 수 없습니다.",
            None,
        ))
    }
}

async fn serve_socket(
    socket: WebSocket,
    runtime: RealtimeRuntime,
    pool: sqlx::PgPool,
    user_id: crate::domain::UserId,
    auth_repository: Arc<AuthRepository>,
    token_hash: Vec<u8>,
) {
    let (mut sender, mut receiver) = socket.split();
    let mut events = runtime.subscribe();
    let mut subscriptions = HashSet::<String>::new();
    let mut heartbeat = tokio::time::interval(PING_INTERVAL);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut subscription_recheck = tokio::time::interval(SUBSCRIPTION_RECHECK_INTERVAL);
    subscription_recheck.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_seen = Instant::now();

    loop {
        tokio::select! {
            incoming = receiver.next() => {
                let Some(incoming) = incoming else {
                    return;
                };
                let message = match incoming {
                    Ok(message) => message,
                    Err(_) => return,
                };
                last_seen = Instant::now();
                match message {
                    Message::Text(text) => {
                        let frame = match serde_json::from_str::<ClientFrame>(&text) {
                            Ok(frame) => frame,
                            Err(_) => {
                                if !send_frame(&mut sender, &ServerControlFrame::Error {
                                    code: "invalid_message",
                                    message: "실시간 요청 형식이 올바르지 않습니다.",
                                }).await {
                                    return;
                                }
                                continue;
                            }
                        };
                        let continue_socket = match frame {
                            ClientFrame::Subscribe { topics } => {
                                subscribe_topics(
                                    &mut sender,
                                    &runtime,
                                    &pool,
                                    &user_id,
                                    &mut subscriptions,
                                    topics,
                                ).await
                            }
                            ClientFrame::Unsubscribe { topics } => {
                                unsubscribe_topics(&mut sender, &mut subscriptions, topics).await
                            }
                        };
                        if !continue_socket {
                            return;
                        }
                    }
                    Message::Close(_) => return,
                    Message::Binary(_) => {
                        if !send_frame(&mut sender, &ServerControlFrame::Error {
                            code: "invalid_message",
                            message: "실시간 요청 형식이 올바르지 않습니다.",
                        }).await {
                            return;
                        }
                    }
                    Message::Ping(_) | Message::Pong(_) => {}
                }
            }
            event = events.recv() => {
                match event {
                    Ok(event) if subscriptions.contains(event.topic()) => {
                        if !send_frame(&mut sender, &event).await {
                            return;
                        }
                    }
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        let topics = (!subscriptions.is_empty())
                            .then(|| subscriptions.iter().cloned().collect());
                        if !send_frame(&mut sender, &ServerControlFrame::ResyncRequired { topics }).await {
                            return;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
            _ = heartbeat.tick() => {
                if last_seen.elapsed() >= IDLE_TIMEOUT {
                    let _ = sender.send(Message::Close(None)).await;
                    return;
                }
                if sender.send(Message::Ping(Vec::new().into())).await.is_err() {
                    return;
                }
            }
            _ = subscription_recheck.tick(), if !subscriptions.is_empty() => {
                if !recheck_subscriptions(
                    &runtime,
                    &pool,
                    &user_id,
                    &auth_repository,
                    &token_hash,
                    &mut subscriptions,
                ).await {
                    return;
                }
                let topics = subscriptions.iter().cloned().collect();
                if !send_frame(&mut sender, &ServerControlFrame::Subscribed { topics }).await {
                    return;
                }
            }
        }
    }
}

async fn subscribe_topics(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    runtime: &RealtimeRuntime,
    pool: &sqlx::PgPool,
    user_id: &crate::domain::UserId,
    subscriptions: &mut HashSet<String>,
    topics: Vec<String>,
) -> bool {
    let parsed = match parsed_topics(topics) {
        Ok(topics) => topics,
        Err(error) => return send_topic_error(sender, error).await,
    };
    for topic in &parsed {
        match runtime.authorize_entry_topic(pool, user_id, topic).await {
            Ok(()) => {}
            Err(error) => return send_authorization_error(sender, error).await,
        }
    }
    subscriptions.extend(parsed.into_iter().map(|topic| topic.as_str().to_owned()));
    let topics = subscriptions.iter().cloned().collect();
    send_frame(sender, &ServerControlFrame::Subscribed { topics }).await
}

async fn unsubscribe_topics(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    subscriptions: &mut HashSet<String>,
    topics: Vec<String>,
) -> bool {
    let parsed = match parsed_topics(topics) {
        Ok(topics) => topics,
        Err(error) => return send_topic_error(sender, error).await,
    };
    for topic in parsed {
        subscriptions.remove(topic.as_str());
    }
    let topics = subscriptions.iter().cloned().collect();
    send_frame(sender, &ServerControlFrame::Subscribed { topics }).await
}

async fn recheck_subscriptions(
    runtime: &RealtimeRuntime,
    pool: &sqlx::PgPool,
    user_id: &crate::domain::UserId,
    auth_repository: &AuthRepository,
    token_hash: &[u8],
    subscriptions: &mut HashSet<String>,
) -> bool {
    match auth_repository.resolve_session(token_hash).await {
        Ok(Some(_)) => {}
        Ok(None) => return false,
        Err(error) => {
            tracing::warn!(error = %error, "failed to recheck realtime session");
            return false;
        }
    }
    let current = subscriptions.iter().cloned().collect::<Vec<_>>();
    for raw in current {
        let Ok(topic) = RealtimeTopic::parse(&raw) else {
            subscriptions.remove(&raw);
            continue;
        };
        match runtime.authorize_entry_topic(pool, user_id, &topic).await {
            Ok(()) => {}
            Err(TopicAuthorizationError::Membership(error)) => {
                tracing::warn!(error = %error, "failed to recheck realtime topic access");
                return false;
            }
            Err(TopicAuthorizationError::Database(error)) => {
                tracing::warn!(error = %error, "failed to recheck realtime topic access");
                return false;
            }
            Err(_) => {
                subscriptions.remove(topic.as_str());
            }
        }
    }
    true
}

async fn send_topic_error(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    error: RealtimeTopicError,
) -> bool {
    let (code, message) = match error {
        RealtimeTopicError::UnsupportedKind => (
            "unsupported_topic",
            "이 화면에서는 해당 실시간 topic을 사용할 수 없습니다.",
        ),
        _ => ("invalid_topic", "실시간 topic 형식이 올바르지 않습니다."),
    };
    send_frame(sender, &ServerControlFrame::Error { code, message }).await
}

async fn send_authorization_error(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    error: TopicAuthorizationError,
) -> bool {
    let (code, message) = match error {
        TopicAuthorizationError::Unsupported => (
            "unsupported_topic",
            "이 화면에서는 해당 실시간 topic을 사용할 수 없습니다.",
        ),
        TopicAuthorizationError::Forbidden => (
            "subscription_forbidden",
            "해당 길드의 실시간 정보를 볼 권한이 없습니다.",
        ),
        TopicAuthorizationError::NotFound => (
            "subscription_not_found",
            "실시간으로 볼 수 있는 대상을 찾을 수 없습니다.",
        ),
        TopicAuthorizationError::Membership(error) => {
            tracing::warn!(error = %error, "failed to authorize realtime topic");
            (
                "subscription_unavailable",
                "실시간 구독 권한을 확인할 수 없습니다.",
            )
        }
        TopicAuthorizationError::Database(error) => {
            tracing::warn!(error = %error, "failed to authorize realtime topic");
            (
                "subscription_unavailable",
                "실시간 구독 권한을 확인할 수 없습니다.",
            )
        }
    };
    send_frame(sender, &ServerControlFrame::Error { code, message }).await
}

async fn send_frame<T: Serialize>(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    frame: &T,
) -> bool {
    let Ok(payload) = serde_json::to_string(frame) else {
        return false;
    };
    sender.send(Message::Text(payload.into())).await.is_ok()
}
