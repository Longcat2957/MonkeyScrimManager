use thiserror::Error;
use time::{Duration, OffsetDateTime};

use sqlx::{FromRow, PgPool};

pub const NORMAL_HEARTBEAT_MAX_AGE: Duration = Duration::seconds(90);
pub const DELAYED_HEARTBEAT_MAX_AGE: Duration = Duration::minutes(5);

const MAX_INSTANCE_ID_BYTES: usize = 128;
const MAX_VERSION_BYTES: usize = 100;
const MAX_COMMIT_SHA_BYTES: usize = 128;

#[derive(Clone, Debug)]
pub struct RecordBotHeartbeat<'a> {
    pub instance_id: &'a str,
    pub version: &'a str,
    pub commit_sha: Option<&'a str>,
    pub recorded_at: OffsetDateTime,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BotInstanceHeartbeat {
    pub instance_id: String,
    pub version: String,
    pub commit_sha: Option<String>,
    pub started_at: OffsetDateTime,
    pub last_heartbeat_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BotHeartbeatStatus {
    Normal,
    Delayed,
    Down,
}

pub async fn record_heartbeat(
    pool: &PgPool,
    command: &RecordBotHeartbeat<'_>,
) -> Result<BotInstanceHeartbeat, ServiceStatusRepositoryError> {
    validate_heartbeat(command)?;

    let row = sqlx::query_as::<_, BotInstanceRow>(
        "INSERT INTO bot_instances (
		     instance_id,
		     version,
		     commit_sha,
		     started_at,
		     last_heartbeat_at,
		     updated_at
		 ) VALUES ($1, $2, $3, $4, $4, $4)
		 ON CONFLICT (instance_id) DO UPDATE
		 SET version = EXCLUDED.version,
		     commit_sha = EXCLUDED.commit_sha,
		     last_heartbeat_at = EXCLUDED.last_heartbeat_at,
		     updated_at = EXCLUDED.updated_at
		 RETURNING instance_id, version, commit_sha, started_at, last_heartbeat_at, updated_at",
    )
    .bind(command.instance_id)
    .bind(command.version)
    .bind(command.commit_sha)
    .bind(command.recorded_at)
    .fetch_one(pool)
    .await?;

    Ok(row.into_heartbeat())
}

pub async fn latest_heartbeat(
    pool: &PgPool,
) -> Result<Option<BotInstanceHeartbeat>, ServiceStatusRepositoryError> {
    let row = sqlx::query_as::<_, BotInstanceRow>(
        "SELECT instance_id, version, commit_sha, started_at, last_heartbeat_at, updated_at
		 FROM bot_instances
		 ORDER BY last_heartbeat_at DESC, updated_at DESC, instance_id ASC
		 LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(BotInstanceRow::into_heartbeat))
}

pub fn classify_heartbeat(
    last_heartbeat_at: Option<OffsetDateTime>,
    now: OffsetDateTime,
) -> BotHeartbeatStatus {
    let Some(last_heartbeat_at) = last_heartbeat_at else {
        return BotHeartbeatStatus::Down;
    };
    let age = now - last_heartbeat_at;
    if age <= NORMAL_HEARTBEAT_MAX_AGE {
        BotHeartbeatStatus::Normal
    } else if age <= DELAYED_HEARTBEAT_MAX_AGE {
        BotHeartbeatStatus::Delayed
    } else {
        BotHeartbeatStatus::Down
    }
}

fn validate_heartbeat(
    command: &RecordBotHeartbeat<'_>,
) -> Result<(), ServiceStatusRepositoryError> {
    let instance_id = command.instance_id.as_bytes();
    if instance_id.is_empty()
        || instance_id.len() > MAX_INSTANCE_ID_BYTES
        || !instance_id[0].is_ascii_alphanumeric()
        || !instance_id[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
    {
        return Err(ServiceStatusRepositoryError::InvalidHeartbeat);
    }

    if !is_visible_ascii(command.version, MAX_VERSION_BYTES) {
        return Err(ServiceStatusRepositoryError::InvalidHeartbeat);
    }

    if command
        .commit_sha
        .is_some_and(|commit_sha| !is_commit_sha(commit_sha))
    {
        return Err(ServiceStatusRepositoryError::InvalidHeartbeat);
    }

    Ok(())
}

fn is_visible_ascii(value: &str, maximum_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum_bytes
        && value.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
}

fn is_commit_sha(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_COMMIT_SHA_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

#[derive(Debug, FromRow)]
struct BotInstanceRow {
    instance_id: String,
    version: String,
    commit_sha: Option<String>,
    started_at: OffsetDateTime,
    last_heartbeat_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

impl BotInstanceRow {
    fn into_heartbeat(self) -> BotInstanceHeartbeat {
        BotInstanceHeartbeat {
            instance_id: self.instance_id,
            version: self.version,
            commit_sha: self.commit_sha,
            started_at: self.started_at,
            last_heartbeat_at: self.last_heartbeat_at,
            updated_at: self.updated_at,
        }
    }
}

#[derive(Debug, Error)]
pub enum ServiceStatusRepositoryError {
    #[error("bot heartbeat input is invalid")]
    InvalidHeartbeat,
    #[error("PostgreSQL service status operation failed")]
    Database(#[from] sqlx::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heartbeat_health_uses_fixed_normal_delayed_and_down_boundaries() {
        let now = OffsetDateTime::from_unix_timestamp(1_750_000_000).expect("valid timestamp");

        assert_eq!(classify_heartbeat(None, now), BotHeartbeatStatus::Down);
        assert_eq!(
            classify_heartbeat(Some(now - NORMAL_HEARTBEAT_MAX_AGE), now),
            BotHeartbeatStatus::Normal
        );
        assert_eq!(
            classify_heartbeat(Some(now - NORMAL_HEARTBEAT_MAX_AGE - Duration::SECOND), now),
            BotHeartbeatStatus::Delayed
        );
        assert_eq!(
            classify_heartbeat(
                Some(now - DELAYED_HEARTBEAT_MAX_AGE - Duration::SECOND),
                now
            ),
            BotHeartbeatStatus::Down
        );
    }

    #[test]
    fn heartbeat_validation_matches_the_bot_instance_contract() {
        let now = OffsetDateTime::now_utc();
        let valid = RecordBotHeartbeat {
            instance_id: "bot-main-1",
            version: "0.1.0",
            commit_sha: Some("abc1234"),
            recorded_at: now,
        };
        assert!(validate_heartbeat(&valid).is_ok());

        for instance_id in ["", "bot instance", "-bot"] {
            let invalid = RecordBotHeartbeat {
                instance_id,
                ..valid.clone()
            };
            assert!(matches!(
                validate_heartbeat(&invalid),
                Err(ServiceStatusRepositoryError::InvalidHeartbeat)
            ));
        }
    }
}
