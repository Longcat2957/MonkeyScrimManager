use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use futures_util::StreamExt;
use redis::AsyncCommands;
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::sync::{broadcast, watch};
use uuid::Uuid;

use crate::{
    config::{Config, Environment},
    domain::{GuildId, RecruitmentId, UserId},
    infrastructure::postgres::begin,
    repository::{outbox, workflow},
};

const HUB_CAPACITY: usize = 256;
const OUTBOX_BATCH_SIZE: u32 = 64;
const OUTBOX_LEASE_DURATION: Duration = Duration::from_secs(30);
const OUTBOX_IDLE_POLL_INTERVAL: Duration = Duration::from_millis(250);
const OUTBOX_RETRY_DELAY: Duration = Duration::from_secs(5);
const VALKEY_RECONNECT_DELAY: Duration = Duration::from_secs(1);
const VALKEY_OPERATION_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone)]
pub struct RealtimeRuntime {
    hub: broadcast::Sender<RealtimeInvalidate>,
    mode: Arc<RealtimeMode>,
    shutdown: watch::Sender<bool>,
    started: Arc<AtomicBool>,
}

#[derive(Clone)]
enum RealtimeMode {
    Local,
    Valkey(ValkeyRuntime),
}

#[derive(Clone)]
struct ValkeyRuntime {
    client: Arc<redis::Client>,
    namespace: String,
}

impl RealtimeRuntime {
    /// Creates the runtime boundary. Local/test runs can use an in-process hub, while deployed
    /// instances require Valkey so an invalidate reaches every API instance.
    pub fn from_config(config: &Config) -> Result<Option<Self>, RealtimeRuntimeError> {
        let mode = if let Some(valkey) = config.valkey() {
            let client = redis::Client::open(valkey.url().expose_secret())
                .map_err(|_| RealtimeRuntimeError::InvalidValkeyUrl)?;
            RealtimeMode::Valkey(ValkeyRuntime {
                client: Arc::new(client),
                namespace: format!("monkeybot:{}:pubsub", config.environment().as_str()),
            })
        } else if matches!(config.environment(), Environment::Local | Environment::Test) {
            RealtimeMode::Local
        } else {
            return Ok(None);
        };

        let (hub, _) = broadcast::channel(HUB_CAPACITY);
        let (shutdown, _) = watch::channel(false);
        Ok(Some(Self {
            hub,
            mode: Arc::new(mode),
            shutdown,
            started: Arc::new(AtomicBool::new(false)),
        }))
    }

    pub fn subscribe(&self) -> broadcast::Receiver<RealtimeInvalidate> {
        self.hub.subscribe()
    }

    pub fn start(&self, pool: sqlx::PgPool) {
        if self
            .started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }

        let relay_runtime = self.clone();
        let relay_shutdown = self.shutdown.subscribe();
        tokio::spawn(async move {
            run_outbox_relay(pool, relay_runtime, relay_shutdown).await;
        });

        if let RealtimeMode::Valkey(valkey) = self.mode.as_ref() {
            let subscriber_runtime = self.clone();
            let subscriber_shutdown = self.shutdown.subscribe();
            let valkey = valkey.clone();
            tokio::spawn(async move {
                run_valkey_subscriber(subscriber_runtime, valkey, subscriber_shutdown).await;
            });
        }
    }

    pub fn shutdown(&self) {
        let _ = self.shutdown.send(true);
    }

    pub(crate) fn valkey_client(&self) -> Option<Arc<redis::Client>> {
        match self.mode.as_ref() {
            RealtimeMode::Local => None,
            RealtimeMode::Valkey(valkey) => Some(valkey.client.clone()),
        }
    }

    pub async fn authorize_topic(
        &self,
        pool: &sqlx::PgPool,
        user_id: &UserId,
        topic: &RealtimeTopic,
    ) -> Result<(), TopicAuthorizationError> {
        match topic.kind() {
            RealtimeTopicKind::Recruitment | RealtimeTopicKind::RecruitmentEntryDraft => {
                self.authorize_entry_topic(pool, user_id, topic).await
            }
            RealtimeTopicKind::Series => self.authorize_series_topic(pool, user_id, topic).await,
            RealtimeTopicKind::AuctionRecruitment
            | RealtimeTopicKind::AuctionTournament
            | RealtimeTopicKind::AuctionMatch => Err(TopicAuthorizationError::Unsupported),
        }
    }

    async fn authorize_entry_topic(
        &self,
        pool: &sqlx::PgPool,
        user_id: &UserId,
        topic: &RealtimeTopic,
    ) -> Result<(), TopicAuthorizationError> {
        if !workflow::has_guild_access(pool, user_id, topic.guild_id())
            .await
            .map_err(TopicAuthorizationError::Membership)?
        {
            return Err(TopicAuthorizationError::Forbidden);
        }

        let recruitment_id = topic
            .recruitment_id()
            .expect("entry-related topic always contains a recruitment ID");
        let visible = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (
                 SELECT 1
                 FROM recruitments
                 WHERE guild_id = $1
                   AND id = $2
                   AND status IN ('OPEN', 'CLOSED')
             )",
        )
        .bind(topic.guild_id())
        .bind(recruitment_id.as_uuid())
        .fetch_one(pool)
        .await
        .map_err(TopicAuthorizationError::Database)?;

        if visible {
            Ok(())
        } else {
            Err(TopicAuthorizationError::NotFound)
        }
    }

    async fn authorize_series_topic(
        &self,
        pool: &sqlx::PgPool,
        user_id: &UserId,
        topic: &RealtimeTopic,
    ) -> Result<(), TopicAuthorizationError> {
        let authorized = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (
                 SELECT 1
                 FROM series AS series
                 JOIN guilds AS guild
                   ON guild.guild_id = series.guild_id
                  AND guild.status = 'ACTIVE'
                 JOIN guild_members AS member
                   ON member.guild_id = series.guild_id
                  AND member.user_id = $2
                  AND member.status = 'ACTIVE'
                 JOIN guild_config AS config
                   ON config.guild_id = series.guild_id
                 JOIN guild_member_discord_roles AS role
                   ON role.guild_id = member.guild_id
                  AND role.user_id = member.user_id
                  AND role.role_id = config.operator_role_id
                 WHERE series.guild_id = $1
                   AND series.id = $3
                   AND series.status = 'IN_PROGRESS'
                   AND series.deleted_at IS NULL
             )",
        )
        .bind(topic.guild_id())
        .bind(user_id)
        .bind(topic.resource_id())
        .fetch_one(pool)
        .await
        .map_err(TopicAuthorizationError::Database)?;

        if authorized {
            Ok(())
        } else {
            Err(TopicAuthorizationError::Forbidden)
        }
    }

    async fn publish(&self, event: RealtimeInvalidate) -> Result<(), RealtimePublishError> {
        match self.mode.as_ref() {
            RealtimeMode::Local => {
                let _ = self.hub.send(event);
                Ok(())
            }
            RealtimeMode::Valkey(valkey) => {
                let payload = serde_json::to_string(&event)
                    .map_err(|_| RealtimePublishError::Serialization)?;
                let channel = valkey.channel_for(event.topic());
                let mut connection = tokio::time::timeout(
                    VALKEY_OPERATION_TIMEOUT,
                    valkey.client.get_multiplexed_async_connection(),
                )
                .await
                .map_err(|_| RealtimePublishError::ValkeyUnavailable)?
                .map_err(|_| RealtimePublishError::ValkeyUnavailable)?;
                tokio::time::timeout(
                    VALKEY_OPERATION_TIMEOUT,
                    connection.publish::<_, _, i64>(channel, payload),
                )
                .await
                .map_err(|_| RealtimePublishError::ValkeyUnavailable)?
                .map_err(|_| RealtimePublishError::ValkeyUnavailable)?;
                // The subscriber task receives this publication too, but fan-out here removes a
                // startup/reconnect race for sockets attached to this API instance. Duplicates
                // are part of the at-least-once invalidate contract.
                self.fan_out(event);
                Ok(())
            }
        }
    }

    fn fan_out(&self, event: RealtimeInvalidate) {
        let _ = self.hub.send(event);
    }
}

impl ValkeyRuntime {
    fn channel_for(&self, topic: &str) -> String {
        format!("{}:{topic}", self.namespace)
    }

    fn pattern(&self) -> String {
        format!("{}:*", self.namespace)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RealtimeTopic {
    raw: String,
    guild_id: GuildId,
    kind: RealtimeTopicKind,
    resource_id: Uuid,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RealtimeTopicKind {
    Recruitment,
    RecruitmentEntryDraft,
    Series,
    AuctionRecruitment,
    AuctionTournament,
    AuctionMatch,
}

impl RealtimeTopic {
    pub fn parse(raw: &str) -> Result<Self, RealtimeTopicError> {
        let mut parts = raw.split(':');
        let (Some(prefix), Some(guild_id), Some(kind), Some(resource_id), None) = (
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
        ) else {
            return Err(RealtimeTopicError::InvalidFormat);
        };
        if prefix != "guild" {
            return Err(RealtimeTopicError::InvalidFormat);
        }
        let guild_id = GuildId::parse(guild_id).map_err(|_| RealtimeTopicError::InvalidGuildId)?;
        let kind = match kind {
            "recruitment" => RealtimeTopicKind::Recruitment,
            "recruitment-entry-draft" => RealtimeTopicKind::RecruitmentEntryDraft,
            "series" => RealtimeTopicKind::Series,
            "auction-recruitment" => RealtimeTopicKind::AuctionRecruitment,
            "auction-tournament" => RealtimeTopicKind::AuctionTournament,
            "auction-match" => RealtimeTopicKind::AuctionMatch,
            _ => return Err(RealtimeTopicError::UnsupportedKind),
        };
        let resource_id =
            Uuid::parse_str(resource_id).map_err(|_| RealtimeTopicError::InvalidId)?;
        let canonical = format!("guild:{guild_id}:{}:{resource_id}", kind.as_str());
        if raw != canonical {
            return Err(RealtimeTopicError::NonCanonical);
        }
        Ok(Self {
            raw: canonical,
            guild_id,
            kind,
            resource_id,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }

    pub fn guild_id(&self) -> &GuildId {
        &self.guild_id
    }

    pub const fn kind(&self) -> RealtimeTopicKind {
        self.kind
    }

    pub const fn resource_id(&self) -> Uuid {
        self.resource_id
    }

    pub const fn is_entry_related(&self) -> bool {
        matches!(
            self.kind,
            RealtimeTopicKind::Recruitment | RealtimeTopicKind::RecruitmentEntryDraft
        )
    }

    pub fn recruitment_id(&self) -> Option<RecruitmentId> {
        self.is_entry_related()
            .then_some(RecruitmentId::from_uuid(self.resource_id))
    }
}

impl RealtimeTopicKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Recruitment => "recruitment",
            Self::RecruitmentEntryDraft => "recruitment-entry-draft",
            Self::Series => "series",
            Self::AuctionRecruitment => "auction-recruitment",
            Self::AuctionTournament => "auction-tournament",
            Self::AuctionMatch => "auction-match",
        }
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum RealtimeTopicError {
    #[error("topic format is invalid")]
    InvalidFormat,
    #[error("topic guild ID is invalid")]
    InvalidGuildId,
    #[error("topic kind is unsupported")]
    UnsupportedKind,
    #[error("topic resource ID is invalid")]
    InvalidId,
    #[error("topic is not canonical")]
    NonCanonical,
}

#[derive(Debug, Error)]
pub enum TopicAuthorizationError {
    #[error("topic is not available for realtime subscription")]
    Unsupported,
    #[error("current user cannot access the guild")]
    Forbidden,
    #[error("topic resource was not found")]
    NotFound,
    #[error("failed to check realtime guild membership")]
    Membership(#[source] crate::repository::workflow::WorkflowRepositoryError),
    #[error("failed to check realtime topic access")]
    Database(#[source] sqlx::Error),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClientFrame {
    Subscribe { topics: Vec<String> },
    Unsubscribe { topics: Vec<String> },
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ServerControlFrame {
    Subscribed {
        topics: Vec<String>,
    },
    #[serde(rename = "resync_required")]
    ResyncRequired {
        #[serde(skip_serializing_if = "Option::is_none")]
        topics: Option<Vec<String>>,
    },
    Error {
        code: &'static str,
        message: &'static str,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RealtimeInvalidate {
    #[serde(rename = "type")]
    kind: RealtimeEventType,
    event_id: String,
    topic: String,
    resource_id: String,
    revision: i64,
    occurred_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum RealtimeEventType {
    Invalidate,
}

impl RealtimeInvalidate {
    fn from_claimed(event: &outbox::ClaimedOutboxEvent) -> Result<Self, RealtimeEventError> {
        let topic = RealtimeTopic::parse(&event.topic).map_err(RealtimeEventError::InvalidTopic)?;
        if &event.guild_id != topic.guild_id() {
            return Err(RealtimeEventError::MismatchedGuildId);
        }
        if event.aggregate_id != topic.resource_id().to_string() {
            return Err(RealtimeEventError::MismatchedAggregateId);
        }
        if event.aggregate_revision < 0 {
            return Err(RealtimeEventError::InvalidRevision);
        }
        let occurred_at = event
            .occurred_at
            .format(&Rfc3339)
            .map_err(|_| RealtimeEventError::InvalidOccurredAt)?;
        Ok(Self {
            kind: RealtimeEventType::Invalidate,
            event_id: event.id.to_string(),
            topic: topic.raw,
            resource_id: event.aggregate_id.clone(),
            revision: event.aggregate_revision,
            occurred_at,
        })
    }

    fn from_valkey(payload: &str) -> Result<Self, RealtimeEventError> {
        let event =
            serde_json::from_str::<Self>(payload).map_err(|_| RealtimeEventError::InvalidWire)?;
        let topic = RealtimeTopic::parse(&event.topic).map_err(RealtimeEventError::InvalidTopic)?;
        if event.resource_id != topic.resource_id().to_string() {
            return Err(RealtimeEventError::MismatchedAggregateId);
        }
        if event.revision < 0 {
            return Err(RealtimeEventError::InvalidRevision);
        }
        Uuid::parse_str(&event.event_id).map_err(|_| RealtimeEventError::InvalidWire)?;
        OffsetDateTime::parse(&event.occurred_at, &Rfc3339)
            .map_err(|_| RealtimeEventError::InvalidOccurredAt)?;
        Ok(event)
    }

    pub fn topic(&self) -> &str {
        &self.topic
    }
}

#[derive(Clone, Debug, Error)]
enum RealtimeEventError {
    #[error("realtime outbox topic is invalid")]
    InvalidTopic(#[source] RealtimeTopicError),
    #[error("realtime outbox aggregate ID does not match the topic")]
    MismatchedAggregateId,
    #[error("realtime outbox guild ID does not match the topic")]
    MismatchedGuildId,
    #[error("realtime outbox revision is invalid")]
    InvalidRevision,
    #[error("realtime outbox occurrence timestamp is invalid")]
    InvalidOccurredAt,
    #[error("realtime Valkey event is invalid")]
    InvalidWire,
}

#[derive(Debug, Error)]
pub enum RealtimeRuntimeError {
    #[error("Valkey URL is invalid")]
    InvalidValkeyUrl,
}

#[derive(Debug, Error)]
enum RealtimePublishError {
    #[error("realtime event could not be serialized")]
    Serialization,
    #[error("Valkey is unavailable")]
    ValkeyUnavailable,
}

async fn run_outbox_relay(
    pool: sqlx::PgPool,
    runtime: RealtimeRuntime,
    mut shutdown: watch::Receiver<bool>,
) {
    let owner = format!("realtime-{}", Uuid::now_v7());
    loop {
        if *shutdown.borrow() {
            return;
        }
        let claimed = match claim_outbox_batch(&pool, &owner).await {
            Ok(claimed) => claimed,
            Err(error) => {
                tracing::warn!(error = %error, "failed to claim realtime outbox events");
                if wait_or_shutdown(&mut shutdown, OUTBOX_IDLE_POLL_INTERVAL).await {
                    return;
                }
                continue;
            }
        };

        if claimed.is_empty() {
            if wait_or_shutdown(&mut shutdown, OUTBOX_IDLE_POLL_INTERVAL).await {
                return;
            }
            continue;
        }

        for event in claimed {
            let invalidate = match RealtimeInvalidate::from_claimed(&event) {
                Ok(event) => event,
                Err(error) => {
                    tracing::error!(event_id = %event.id, error = %error, "invalid realtime outbox event");
                    nack_outbox_event(&pool, &event, "invalid_realtime_event").await;
                    continue;
                }
            };
            match runtime.publish(invalidate).await {
                Ok(()) => ack_outbox_event(&pool, &event).await,
                Err(error) => {
                    tracing::warn!(event_id = %event.id, error = %error, "failed to publish realtime event");
                    nack_outbox_event(&pool, &event, "realtime_publish_failed").await;
                }
            }
        }
    }
}

async fn run_valkey_subscriber(
    runtime: RealtimeRuntime,
    valkey: ValkeyRuntime,
    mut shutdown: watch::Receiver<bool>,
) {
    loop {
        if *shutdown.borrow() {
            return;
        }
        let mut pubsub = match valkey.client.get_async_pubsub().await {
            Ok(pubsub) => pubsub,
            Err(_) => {
                tracing::warn!("failed to connect realtime Valkey subscriber");
                if wait_or_shutdown(&mut shutdown, VALKEY_RECONNECT_DELAY).await {
                    return;
                }
                continue;
            }
        };
        if pubsub.psubscribe(valkey.pattern()).await.is_err() {
            tracing::warn!("failed to subscribe realtime Valkey channel pattern");
            if wait_or_shutdown(&mut shutdown, VALKEY_RECONNECT_DELAY).await {
                return;
            }
            continue;
        }

        let mut messages = pubsub.on_message();
        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        return;
                    }
                }
                message = messages.next() => {
                    let Some(message) = message else {
                        tracing::warn!("realtime Valkey subscription ended");
                        break;
                    };
                    let payload = match message.get_payload::<String>() {
                        Ok(payload) => payload,
                        Err(_) => {
                            tracing::warn!("received non-text realtime Valkey payload");
                            continue;
                        }
                    };
                    match RealtimeInvalidate::from_valkey(&payload) {
                        Ok(event) => runtime.fan_out(event),
                        Err(error) => tracing::warn!(error = %error, "received invalid realtime Valkey event"),
                    }
                }
            }
        }
        if wait_or_shutdown(&mut shutdown, VALKEY_RECONNECT_DELAY).await {
            return;
        }
    }
}

async fn claim_outbox_batch(
    pool: &sqlx::PgPool,
    owner: &str,
) -> Result<Vec<outbox::ClaimedOutboxEvent>, OutboxRelayError> {
    let mut transaction = begin(pool).await?;
    let events = outbox::claim_any(
        &mut transaction,
        &outbox::ClaimAnyOutbox {
            lease_owner: owner,
            lease_duration: OUTBOX_LEASE_DURATION,
            limit: OUTBOX_BATCH_SIZE,
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(events)
}

async fn ack_outbox_event(pool: &sqlx::PgPool, event: &outbox::ClaimedOutboxEvent) {
    let outcome: Result<(), OutboxRelayError> = async {
        let mut transaction = begin(pool).await?;
        let _ = outbox::ack(
            &mut transaction,
            &outbox::AckOutbox {
                guild_id: &event.guild_id,
                event_id: event.id,
                lease_owner: &event.lease.owner,
                lease_token: event.lease.token,
                published_at: OffsetDateTime::now_utc(),
            },
        )
        .await
        .map_err(OutboxRelayError::Claim)?;
        transaction
            .commit()
            .await
            .map_err(OutboxRelayError::Transaction)
    }
    .await;
    if let Err(error) = outcome {
        tracing::warn!(event_id = %event.id, error = %error, "failed to acknowledge realtime outbox event");
    }
}

async fn nack_outbox_event(
    pool: &sqlx::PgPool,
    event: &outbox::ClaimedOutboxEvent,
    reason: &'static str,
) {
    let outcome: Result<(), OutboxRelayError> = async {
        let mut transaction = begin(pool).await?;
        let _ = outbox::nack(
            &mut transaction,
            &outbox::NackOutbox {
                guild_id: &event.guild_id,
                event_id: event.id,
                lease_owner: &event.lease.owner,
                lease_token: event.lease.token,
                retry_delay: OUTBOX_RETRY_DELAY,
                last_error: reason,
            },
        )
        .await
        .map_err(OutboxRelayError::Claim)?;
        transaction
            .commit()
            .await
            .map_err(OutboxRelayError::Transaction)
    }
    .await;
    if let Err(error) = outcome {
        tracing::warn!(event_id = %event.id, error = %error, "failed to return realtime outbox lease");
    }
}

async fn wait_or_shutdown(shutdown: &mut watch::Receiver<bool>, duration: Duration) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(duration) => false,
        changed = shutdown.changed() => changed.is_err() || *shutdown.borrow(),
    }
}

#[derive(Debug, Error)]
enum OutboxRelayError {
    #[error("failed to begin or commit realtime outbox transaction")]
    Transaction(#[from] sqlx::Error),
    #[error("failed to claim realtime outbox events")]
    Claim(#[from] outbox::OutboxRepositoryError),
}

pub fn parsed_topics(topics: Vec<String>) -> Result<Vec<RealtimeTopic>, RealtimeTopicError> {
    if topics.is_empty() || topics.len() > 16 {
        return Err(RealtimeTopicError::InvalidFormat);
    }
    let mut unique = HashSet::with_capacity(topics.len());
    let mut parsed = Vec::with_capacity(topics.len());
    for raw in topics {
        let topic = RealtimeTopic::parse(&raw)?;
        if !unique.insert(topic.as_str().to_owned()) {
            return Err(RealtimeTopicError::NonCanonical);
        }
        parsed.push(topic);
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{GuildId, OutboxEventId},
        repository::outbox::{ClaimedOutboxEvent, OutboxLease},
    };
    use serde_json::json;

    const GUILD_ID: &str = "123456789012345678";
    const RECRUITMENT_ID: &str = "01900000-0000-7000-8000-000000000001";
    const EVENT_ID: &str = "01900000-0000-7000-8000-000000000002";
    const LEASE_TOKEN: &str = "01900000-0000-7000-8000-000000000003";

    fn entry_topic() -> String {
        format!("guild:{GUILD_ID}:recruitment-entry-draft:{RECRUITMENT_ID}")
    }

    fn claimed_entry_event() -> ClaimedOutboxEvent {
        let occurred_at = OffsetDateTime::parse("2026-07-14T12:34:56Z", &Rfc3339)
            .expect("fixed occurred-at timestamp");
        ClaimedOutboxEvent {
            id: OutboxEventId::from_uuid(Uuid::parse_str(EVENT_ID).expect("event ID")),
            guild_id: GuildId::parse(GUILD_ID).expect("guild ID"),
            topic: entry_topic(),
            aggregate_type: "RECRUITMENT_ENTRY_DRAFT".to_owned(),
            aggregate_id: RECRUITMENT_ID.to_owned(),
            aggregate_revision: 42,
            event_type: "ENTRY_DRAFT_MOVED".to_owned(),
            // The relay must never reflect this arbitrary application payload to websocket clients.
            payload: json!({
                "type": "different_event",
                "resourceId": "01900000-0000-7000-8000-000000000099",
                "revision": 9_999,
                "secret": "must-not-reach-browser"
            }),
            occurred_at,
            available_at: occurred_at,
            attempts: 0,
            lease: OutboxLease {
                owner: "realtime-test".to_owned(),
                token: Uuid::parse_str(LEASE_TOKEN).expect("lease token"),
                until: occurred_at + time::Duration::seconds(30),
            },
        }
    }

    #[test]
    fn topic_parser_accepts_only_canonical_known_topics() {
        let topic = RealtimeTopic::parse(&format!(
            "guild:{GUILD_ID}:recruitment-entry-draft:{RECRUITMENT_ID}"
        ))
        .expect("canonical entry topic");
        assert_eq!(topic.guild_id(), &GuildId::parse(GUILD_ID).expect("guild"));
        assert!(topic.is_entry_related());
        assert_eq!(
            topic.recruitment_id().expect("recruitment").to_string(),
            RECRUITMENT_ID
        );

        assert!(matches!(
            RealtimeTopic::parse(&format!("guild:{GUILD_ID}:unknown:{RECRUITMENT_ID}")),
            Err(RealtimeTopicError::UnsupportedKind)
        ));
        let hex_resource_id = "01900000-0000-7000-8000-0000000000ab";
        assert!(matches!(
            RealtimeTopic::parse(&format!(
                "guild:{GUILD_ID}:recruitment:{}",
                hex_resource_id.to_uppercase()
            )),
            Err(RealtimeTopicError::NonCanonical)
        ));
    }

    #[test]
    fn client_frames_reject_unknown_fields() {
        assert!(serde_json::from_str::<ClientFrame>(
            r#"{"type":"subscribe","topics":["guild:123456789012345678:recruitment:01900000-0000-7000-8000-000000000001"]}"#
        )
        .is_ok());
        assert!(
            serde_json::from_str::<ClientFrame>(r#"{"type":"subscribe","topics":[],"other":true}"#)
                .is_err()
        );
    }

    #[test]
    fn parsed_topics_require_a_nonempty_unique_bounded_canonical_set() {
        let topic = entry_topic();

        assert!(matches!(
            parsed_topics(Vec::new()),
            Err(RealtimeTopicError::InvalidFormat)
        ));
        assert!(matches!(
            parsed_topics(vec![topic.clone(), topic.clone()]),
            Err(RealtimeTopicError::NonCanonical)
        ));
        assert!(matches!(
            parsed_topics(std::iter::repeat_n(topic, 17).collect()),
            Err(RealtimeTopicError::InvalidFormat)
        ));
    }

    #[test]
    fn server_control_frames_match_the_browser_protocol() {
        let topic = entry_topic();

        assert_eq!(
            serde_json::to_value(ServerControlFrame::Subscribed {
                topics: vec![topic.clone()],
            })
            .expect("subscribed frame"),
            json!({ "type": "subscribed", "topics": [topic] })
        );
        assert_eq!(
            serde_json::to_value(ServerControlFrame::ResyncRequired { topics: None })
                .expect("resync frame"),
            json!({ "type": "resync_required" })
        );
        assert_eq!(
            serde_json::to_value(ServerControlFrame::Error {
                code: "subscription_forbidden",
                message: "해당 길드의 실시간 정보를 볼 권한이 없습니다.",
            })
            .expect("error frame"),
            json!({
                "type": "error",
                "code": "subscription_forbidden",
                "message": "해당 길드의 실시간 정보를 볼 권한이 없습니다.",
            })
        );
    }

    #[test]
    fn claimed_outbox_event_is_sanitized_into_the_browser_invalidate_frame() {
        let event = claimed_entry_event();
        let invalidate = RealtimeInvalidate::from_claimed(&event).expect("valid claimed event");

        assert_eq!(
            serde_json::to_value(&invalidate).expect("invalidate frame"),
            json!({
                "type": "invalidate",
                "eventId": EVENT_ID,
                "topic": entry_topic(),
                "resourceId": RECRUITMENT_ID,
                "revision": 42,
                "occurredAt": "2026-07-14T12:34:56Z",
            })
        );
    }

    #[test]
    fn claimed_outbox_event_rejects_a_topic_from_another_guild() {
        let mut event = claimed_entry_event();
        event.guild_id = GuildId::parse("123456789012345679").expect("different guild ID");

        assert!(matches!(
            RealtimeInvalidate::from_claimed(&event),
            Err(RealtimeEventError::MismatchedGuildId)
        ));
    }

    #[test]
    fn valkey_wire_event_rejects_a_topic_resource_or_revision_mismatch() {
        let event = claimed_entry_event();
        let invalidate = RealtimeInvalidate::from_claimed(&event).expect("valid claimed event");
        let valid_wire = serde_json::to_string(&invalidate).expect("wire event");
        assert!(RealtimeInvalidate::from_valkey(&valid_wire).is_ok());

        let invalid_resource = json!({
            "type": "invalidate",
            "eventId": EVENT_ID,
            "topic": entry_topic(),
            "resourceId": "01900000-0000-7000-8000-000000000099",
            "revision": 42,
            "occurredAt": "2026-07-14T12:34:56Z",
        });
        assert!(matches!(
            RealtimeInvalidate::from_valkey(&invalid_resource.to_string()),
            Err(RealtimeEventError::MismatchedAggregateId)
        ));

        let invalid_revision = json!({
            "type": "invalidate",
            "eventId": EVENT_ID,
            "topic": entry_topic(),
            "resourceId": RECRUITMENT_ID,
            "revision": -1,
            "occurredAt": "2026-07-14T12:34:56Z",
        });
        assert!(matches!(
            RealtimeInvalidate::from_valkey(&invalid_revision.to_string()),
            Err(RealtimeEventError::InvalidRevision)
        ));

        let unknown_property = json!({
            "type": "invalidate",
            "eventId": EVENT_ID,
            "topic": entry_topic(),
            "resourceId": RECRUITMENT_ID,
            "revision": 42,
            "occurredAt": "2026-07-14T12:34:56Z",
            "payload": { "secret": "must-not-be-accepted" },
        });
        assert!(matches!(
            RealtimeInvalidate::from_valkey(&unknown_property.to_string()),
            Err(RealtimeEventError::InvalidWire)
        ));
    }
}
