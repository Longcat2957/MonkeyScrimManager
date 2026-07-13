use std::{fmt, time::Duration};

use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use sqlx::{FromRow, PgPool, Postgres, Transaction, types::Json};
use thiserror::Error;
use time::OffsetDateTime;
use uuid::Uuid;

use super::riot::normalize_riot_id_v1;

pub const RIOT_RECORDS_REFRESH_COOLDOWN: Duration = Duration::from_secs(30);
pub const RIOT_RECORDS_REFRESH_LEASE_DURATION: Duration = Duration::from_secs(5 * 60);
pub const RIOT_RECORDS_SNAPSHOT_SCHEMA_VERSION: i32 = 1;

const MAX_GAME_NAME_CHARS: usize = 64;
const MAX_TAG_LINE_CHARS: usize = 16;
const MAX_PUUID_BYTES: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RiotRecordsLookup {
    platform: String,
    game_name: String,
    tag_line: String,
    normalized_game_name: String,
    normalized_tag_line: String,
}

impl RiotRecordsLookup {
    pub fn new(
        platform: &str,
        game_name: &str,
        tag_line: &str,
    ) -> Result<Self, RiotRecordsRepositoryError> {
        let platform = normalize_platform(platform)?;
        let game_name = validate_riot_id_component(game_name, MAX_GAME_NAME_CHARS)?;
        let tag_line = validate_riot_id_component(tag_line, MAX_TAG_LINE_CHARS)?;
        let normalized_game_name = normalize_riot_id_v1(&game_name);
        let normalized_tag_line = normalize_riot_id_v1(&tag_line);
        if normalized_game_name.is_empty()
            || normalized_game_name.chars().count() > MAX_GAME_NAME_CHARS
            || normalized_tag_line.is_empty()
            || normalized_tag_line.chars().count() > MAX_TAG_LINE_CHARS
        {
            return Err(RiotRecordsRepositoryError::InvalidLookup);
        }

        Ok(Self {
            platform,
            game_name,
            tag_line,
            normalized_game_name,
            normalized_tag_line,
        })
    }

    pub fn platform(&self) -> &str {
        &self.platform
    }

    pub fn game_name(&self) -> &str {
        &self.game_name
    }

    pub fn tag_line(&self) -> &str {
        &self.tag_line
    }
}

pub struct CanonicalRiotRecordsIdentity {
    platform: String,
    routing_region: &'static str,
    puuid: String,
    game_name: String,
    tag_line: String,
    normalized_game_name: String,
    normalized_tag_line: String,
}

impl CanonicalRiotRecordsIdentity {
    pub fn new(
        platform: &str,
        puuid: &str,
        game_name: &str,
        tag_line: &str,
    ) -> Result<Self, RiotRecordsRepositoryError> {
        let lookup = RiotRecordsLookup::new(platform, game_name, tag_line)
            .map_err(|_| RiotRecordsRepositoryError::InvalidCanonicalIdentity)?;
        if puuid.is_empty()
            || puuid.len() > MAX_PUUID_BYTES
            || !puuid
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(RiotRecordsRepositoryError::InvalidCanonicalIdentity);
        }
        let routing_region = routing_region(&lookup.platform);

        Ok(Self {
            platform: lookup.platform,
            routing_region,
            puuid: puuid.to_owned(),
            game_name: lookup.game_name,
            tag_line: lookup.tag_line,
            normalized_game_name: lookup.normalized_game_name,
            normalized_tag_line: lookup.normalized_tag_line,
        })
    }
}

impl fmt::Debug for CanonicalRiotRecordsIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CanonicalRiotRecordsIdentity")
            .field("platform", &self.platform)
            .field("routing_region", &self.routing_region)
            .field("puuid", &"[REDACTED]")
            .field("game_name", &self.game_name)
            .field("tag_line", &self.tag_line)
            .finish()
    }
}

#[derive(Clone, PartialEq)]
pub struct CachedRiotRecords {
    puuid: String,
    pub platform: String,
    pub routing_region: String,
    pub game_name: String,
    pub tag_line: String,
    pub schema_version: i32,
    pub snapshot: Value,
    pub refreshed_at: OffsetDateTime,
}

impl CachedRiotRecords {
    pub fn puuid(&self) -> &str {
        &self.puuid
    }

    pub fn decode<T: DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_value(self.snapshot.clone())
    }
}

impl fmt::Debug for CachedRiotRecords {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CachedRiotRecords")
            .field("puuid", &"[REDACTED]")
            .field("platform", &self.platform)
            .field("routing_region", &self.routing_region)
            .field("game_name", &self.game_name)
            .field("tag_line", &self.tag_line)
            .field("schema_version", &self.schema_version)
            .field("snapshot", &"[JSON]")
            .field("refreshed_at", &self.refreshed_at)
            .finish()
    }
}

#[derive(Clone, PartialEq)]
pub struct CachedRiotRecordsRecentPage {
    pub platform: String,
    pub game_name: String,
    pub tag_line: String,
    pub offset: i32,
    pub requested_count: i32,
    pub schema_version: i32,
    pub page: Value,
    pub fetched_at: OffsetDateTime,
}

impl CachedRiotRecordsRecentPage {
    pub fn decode<T: DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_value(self.page.clone())
    }
}

impl fmt::Debug for CachedRiotRecordsRecentPage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CachedRiotRecordsRecentPage")
            .field("platform", &self.platform)
            .field("game_name", &self.game_name)
            .field("tag_line", &self.tag_line)
            .field("offset", &self.offset)
            .field("requested_count", &self.requested_count)
            .field("schema_version", &self.schema_version)
            .field("page", &"[JSON]")
            .field("fetched_at", &self.fetched_at)
            .finish()
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct RiotRecordsRefreshLease {
    lookup: RiotRecordsLookup,
    token: Uuid,
    pub acquired_at: OffsetDateTime,
    pub expires_at: OffsetDateTime,
}

impl RiotRecordsRefreshLease {
    pub fn lookup(&self) -> &RiotRecordsLookup {
        &self.lookup
    }
}

impl fmt::Debug for RiotRecordsRefreshLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RiotRecordsRefreshLease")
            .field("lookup", &self.lookup)
            .field("token", &"[REDACTED]")
            .field("acquired_at", &self.acquired_at)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum AcquireRiotRecordsRefresh {
    Acquired(RiotRecordsRefreshLease),
    Cooldown {
        snapshot: CachedRiotRecords,
        retry_at: OffsetDateTime,
    },
    InProgress {
        snapshot: Option<CachedRiotRecords>,
        retry_at: OffsetDateTime,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredRiotRecordsSnapshot {
    pub platform: String,
    pub game_name: String,
    pub tag_line: String,
    pub schema_version: i32,
    pub refreshed_at: OffsetDateTime,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredRiotRecordsRecentPage {
    pub platform: String,
    pub game_name: String,
    pub tag_line: String,
    pub offset: i32,
    pub requested_count: i32,
    pub schema_version: i32,
    pub fetched_at: OffsetDateTime,
}

pub async fn get_snapshot(
    pool: &PgPool,
    lookup: &RiotRecordsLookup,
) -> Result<Option<CachedRiotRecords>, RiotRecordsRepositoryError> {
    fetch_snapshot(pool, lookup).await
}

pub async fn get_recent_page(
    pool: &PgPool,
    lookup: &RiotRecordsLookup,
    offset: i32,
    requested_count: i32,
) -> Result<Option<CachedRiotRecordsRecentPage>, RiotRecordsRepositoryError> {
    validate_page_key(offset, requested_count)?;
    let row = sqlx::query_as::<_, CachedRecentPageRow>(
        "SELECT snapshot.platform, snapshot.game_name, snapshot.tag_line,
                page.page_offset, page.requested_count, page.schema_version,
                page.page, page.fetched_at
         FROM riot_records_lookup_aliases AS alias
         JOIN riot_records_snapshots AS snapshot
           ON snapshot.platform = alias.snapshot_platform
          AND snapshot.puuid = alias.snapshot_puuid
         JOIN riot_records_recent_pages AS page
           ON page.platform = snapshot.platform
          AND page.puuid = snapshot.puuid
         WHERE alias.platform = $1
           AND alias.normalized_game_name = $2
           AND alias.normalized_tag_line = $3
           AND page.page_offset = $4
           AND page.requested_count = $5",
    )
    .bind(&lookup.platform)
    .bind(&lookup.normalized_game_name)
    .bind(&lookup.normalized_tag_line)
    .bind(offset)
    .bind(requested_count)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(CachedRecentPageRow::into_record))
}

/// Stores one validated provider-neutral recent-match page for an exact offset/count request.
pub async fn store_recent_page<T: Serialize + ?Sized>(
    pool: &PgPool,
    lookup: &RiotRecordsLookup,
    offset: i32,
    requested_count: i32,
    page: &T,
) -> Result<StoredRiotRecordsRecentPage, RiotRecordsRepositoryError> {
    validate_page_key(offset, requested_count)?;
    let page = serde_json::to_value(page)?;
    if !page.is_object() {
        return Err(RiotRecordsRepositoryError::InvalidRecentPage);
    }

    let mut transaction = pool.begin().await?;
    let owner = sqlx::query_as::<_, RecentPageOwner>(
        "SELECT snapshot.platform, snapshot.puuid, snapshot.game_name, snapshot.tag_line
         FROM riot_records_lookup_aliases AS alias
         JOIN riot_records_snapshots AS snapshot
           ON snapshot.platform = alias.snapshot_platform
          AND snapshot.puuid = alias.snapshot_puuid
         WHERE alias.platform = $1
           AND alias.normalized_game_name = $2
           AND alias.normalized_tag_line = $3
         FOR UPDATE OF snapshot",
    )
    .bind(&lookup.platform)
    .bind(&lookup.normalized_game_name)
    .bind(&lookup.normalized_tag_line)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(RiotRecordsRepositoryError::SnapshotNotFound)?;
    let stored = sqlx::query_as::<_, StoredRecentPageRow>(
        "INSERT INTO riot_records_recent_pages (
             platform, puuid, page_offset, requested_count,
             schema_version, page, fetched_at, created_at, updated_at
         ) VALUES (
             $1, $2, $3, $4,
             $5, $6, clock_timestamp(), clock_timestamp(), clock_timestamp()
         )
         ON CONFLICT (platform, puuid, page_offset, requested_count) DO UPDATE
         SET schema_version = EXCLUDED.schema_version,
             page = EXCLUDED.page,
             fetched_at = EXCLUDED.fetched_at,
             updated_at = EXCLUDED.updated_at
         RETURNING page_offset, requested_count, schema_version, fetched_at",
    )
    .bind(&owner.platform)
    .bind(&owner.puuid)
    .bind(offset)
    .bind(requested_count)
    .bind(RIOT_RECORDS_SNAPSHOT_SCHEMA_VERSION)
    .bind(Json(page))
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;

    Ok(StoredRiotRecordsRecentPage {
        platform: owner.platform,
        game_name: owner.game_name,
        tag_line: owner.tag_line,
        offset: stored.page_offset,
        requested_count: stored.requested_count,
        schema_version: stored.schema_version,
        fetched_at: stored.fetched_at,
    })
}

/// Atomically claims the only provider call allowed for this Riot ID.
///
/// The 30-second cooldown uses PostgreSQL's clock and the committed snapshot timestamp, so it
/// survives process restarts. Callers must complete or abort the lease after the provider call.
pub async fn acquire_refresh(
    pool: &PgPool,
    lookup: &RiotRecordsLookup,
) -> Result<AcquireRiotRecordsRefresh, RiotRecordsRepositoryError> {
    let mut transaction = pool.begin().await?;
    sqlx::query(
        "INSERT INTO riot_records_refresh_gates (
             platform, normalized_game_name, normalized_tag_line,
             lease_token, lease_acquired_at, lease_until, created_at, updated_at
         ) VALUES ($1, $2, $3, NULL, NULL, NULL, clock_timestamp(), clock_timestamp())
         ON CONFLICT (platform, normalized_game_name, normalized_tag_line) DO NOTHING",
    )
    .bind(&lookup.platform)
    .bind(&lookup.normalized_game_name)
    .bind(&lookup.normalized_tag_line)
    .execute(&mut *transaction)
    .await?;

    let gate = sqlx::query_as::<_, RefreshGateRow>(
        "SELECT lease_until
         FROM riot_records_refresh_gates
         WHERE platform = $1
           AND normalized_game_name = $2
           AND normalized_tag_line = $3
         FOR UPDATE",
    )
    .bind(&lookup.platform)
    .bind(&lookup.normalized_game_name)
    .bind(&lookup.normalized_tag_line)
    .fetch_one(&mut *transaction)
    .await?;
    let snapshot = fetch_snapshot_in_transaction(&mut transaction, lookup).await?;
    let database_now = sqlx::query_scalar::<_, OffsetDateTime>("SELECT clock_timestamp()")
        .fetch_one(&mut *transaction)
        .await?;

    if let Some(snapshot) = snapshot.as_ref() {
        let retry_at = snapshot.refreshed_at
            + time::Duration::seconds(RIOT_RECORDS_REFRESH_COOLDOWN.as_secs() as i64);
        if retry_at > database_now {
            transaction.commit().await?;
            return Ok(AcquireRiotRecordsRefresh::Cooldown {
                snapshot: snapshot.clone(),
                retry_at,
            });
        }
    }

    if let Some(retry_at) = gate
        .lease_until
        .filter(|lease_until| *lease_until > database_now)
    {
        transaction.commit().await?;
        return Ok(AcquireRiotRecordsRefresh::InProgress { snapshot, retry_at });
    }

    let token = Uuid::now_v7();
    let lease_times = sqlx::query_as::<_, LeaseTimes>(
        "UPDATE riot_records_refresh_gates
         SET lease_token = $4,
             lease_acquired_at = statement_timestamp(),
             lease_until = statement_timestamp() + ($5 * INTERVAL '1 second'),
             updated_at = clock_timestamp()
         WHERE platform = $1
           AND normalized_game_name = $2
           AND normalized_tag_line = $3
         RETURNING lease_acquired_at AS acquired_at, lease_until AS expires_at",
    )
    .bind(&lookup.platform)
    .bind(&lookup.normalized_game_name)
    .bind(&lookup.normalized_tag_line)
    .bind(token)
    .bind(RIOT_RECORDS_REFRESH_LEASE_DURATION.as_secs() as f64)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;

    Ok(AcquireRiotRecordsRefresh::Acquired(
        RiotRecordsRefreshLease {
            lookup: lookup.clone(),
            token,
            acquired_at: lease_times.acquired_at,
            expires_at: lease_times.expires_at,
        },
    ))
}

/// Commits a validated provider-neutral snapshot and releases the matching refresh lease.
///
/// A completion whose lease was replaced after expiry is rejected, preventing a stale provider
/// response from overwriting the newer refresh.
pub async fn complete_refresh<T: Serialize + ?Sized>(
    pool: &PgPool,
    lease: &RiotRecordsRefreshLease,
    identity: &CanonicalRiotRecordsIdentity,
    snapshot: &T,
) -> Result<StoredRiotRecordsSnapshot, RiotRecordsRepositoryError> {
    if lease.lookup.platform != identity.platform {
        return Err(RiotRecordsRepositoryError::InvalidCanonicalIdentity);
    }
    let snapshot = serde_json::to_value(snapshot)?;
    if !snapshot.is_object() {
        return Err(RiotRecordsRepositoryError::InvalidSnapshot);
    }

    let mut transaction = pool.begin().await?;
    require_lease(&mut transaction, lease).await?;
    let stored = sqlx::query_as::<_, StoredSnapshotRow>(
        "INSERT INTO riot_records_snapshots (
             platform, puuid, routing_region, game_name, tag_line,
             normalized_game_name, normalized_tag_line,
             schema_version, snapshot, refreshed_at, created_at, updated_at
         ) VALUES (
             $1, $2, $3, $4, $5,
             $6, $7,
             $8, $9, clock_timestamp(), clock_timestamp(), clock_timestamp()
         )
         ON CONFLICT (platform, puuid) DO UPDATE
         SET routing_region = EXCLUDED.routing_region,
             game_name = EXCLUDED.game_name,
             tag_line = EXCLUDED.tag_line,
             normalized_game_name = EXCLUDED.normalized_game_name,
             normalized_tag_line = EXCLUDED.normalized_tag_line,
             schema_version = EXCLUDED.schema_version,
             snapshot = EXCLUDED.snapshot,
             refreshed_at = EXCLUDED.refreshed_at,
             updated_at = EXCLUDED.updated_at
         RETURNING platform, game_name, tag_line, schema_version, refreshed_at",
    )
    .bind(&identity.platform)
    .bind(&identity.puuid)
    .bind(identity.routing_region)
    .bind(&identity.game_name)
    .bind(&identity.tag_line)
    .bind(&identity.normalized_game_name)
    .bind(&identity.normalized_tag_line)
    .bind(RIOT_RECORDS_SNAPSHOT_SCHEMA_VERSION)
    .bind(Json(snapshot))
    .fetch_one(&mut *transaction)
    .await?;

    sqlx::query(
        "DELETE FROM riot_records_recent_pages
         WHERE platform = $1 AND puuid = $2",
    )
    .bind(&identity.platform)
    .bind(&identity.puuid)
    .execute(&mut *transaction)
    .await?;

    upsert_alias(
        &mut transaction,
        &lease.lookup.platform,
        &lease.lookup.normalized_game_name,
        &lease.lookup.normalized_tag_line,
        identity,
    )
    .await?;
    upsert_alias(
        &mut transaction,
        &identity.platform,
        &identity.normalized_game_name,
        &identity.normalized_tag_line,
        identity,
    )
    .await?;
    clear_lease(&mut transaction, lease).await?;
    transaction.commit().await?;

    Ok(StoredRiotRecordsSnapshot {
        platform: stored.platform,
        game_name: stored.game_name,
        tag_line: stored.tag_line,
        schema_version: stored.schema_version,
        refreshed_at: stored.refreshed_at,
    })
}

/// Releases a failed provider call without changing or deleting the last good snapshot.
pub async fn abort_refresh(
    pool: &PgPool,
    lease: &RiotRecordsRefreshLease,
) -> Result<bool, RiotRecordsRepositoryError> {
    let result = sqlx::query(
        "UPDATE riot_records_refresh_gates
         SET lease_token = NULL,
             lease_acquired_at = NULL,
             lease_until = NULL,
             updated_at = clock_timestamp()
         WHERE platform = $1
           AND normalized_game_name = $2
           AND normalized_tag_line = $3
           AND lease_token = $4",
    )
    .bind(&lease.lookup.platform)
    .bind(&lease.lookup.normalized_game_name)
    .bind(&lease.lookup.normalized_tag_line)
    .bind(lease.token)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

async fn fetch_snapshot<'executor, Executor>(
    executor: Executor,
    lookup: &RiotRecordsLookup,
) -> Result<Option<CachedRiotRecords>, RiotRecordsRepositoryError>
where
    Executor: sqlx::Executor<'executor, Database = Postgres>,
{
    let row = sqlx::query_as::<_, CachedSnapshotRow>(SNAPSHOT_SELECT)
        .bind(&lookup.platform)
        .bind(&lookup.normalized_game_name)
        .bind(&lookup.normalized_tag_line)
        .fetch_optional(executor)
        .await?;
    Ok(row.map(CachedSnapshotRow::into_record))
}

async fn fetch_snapshot_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    lookup: &RiotRecordsLookup,
) -> Result<Option<CachedRiotRecords>, RiotRecordsRepositoryError> {
    fetch_snapshot(&mut **transaction, lookup).await
}

async fn require_lease(
    transaction: &mut Transaction<'_, Postgres>,
    lease: &RiotRecordsRefreshLease,
) -> Result<(), RiotRecordsRepositoryError> {
    let token = sqlx::query_scalar::<_, Option<Uuid>>(
        "SELECT lease_token
         FROM riot_records_refresh_gates
         WHERE platform = $1
           AND normalized_game_name = $2
           AND normalized_tag_line = $3
         FOR UPDATE",
    )
    .bind(&lease.lookup.platform)
    .bind(&lease.lookup.normalized_game_name)
    .bind(&lease.lookup.normalized_tag_line)
    .fetch_optional(&mut **transaction)
    .await?
    .flatten();
    if token != Some(lease.token) {
        return Err(RiotRecordsRepositoryError::LeaseLost);
    }
    Ok(())
}

async fn upsert_alias(
    transaction: &mut Transaction<'_, Postgres>,
    platform: &str,
    normalized_game_name: &str,
    normalized_tag_line: &str,
    identity: &CanonicalRiotRecordsIdentity,
) -> Result<(), RiotRecordsRepositoryError> {
    sqlx::query(
        "INSERT INTO riot_records_lookup_aliases (
             platform, normalized_game_name, normalized_tag_line,
             snapshot_platform, snapshot_puuid, resolved_at
         ) VALUES ($1, $2, $3, $4, $5, clock_timestamp())
         ON CONFLICT (platform, normalized_game_name, normalized_tag_line) DO UPDATE
         SET snapshot_platform = EXCLUDED.snapshot_platform,
             snapshot_puuid = EXCLUDED.snapshot_puuid,
             resolved_at = EXCLUDED.resolved_at",
    )
    .bind(platform)
    .bind(normalized_game_name)
    .bind(normalized_tag_line)
    .bind(&identity.platform)
    .bind(&identity.puuid)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn clear_lease(
    transaction: &mut Transaction<'_, Postgres>,
    lease: &RiotRecordsRefreshLease,
) -> Result<(), RiotRecordsRepositoryError> {
    let result = sqlx::query(
        "UPDATE riot_records_refresh_gates
         SET lease_token = NULL,
             lease_acquired_at = NULL,
             lease_until = NULL,
             updated_at = clock_timestamp()
         WHERE platform = $1
           AND normalized_game_name = $2
           AND normalized_tag_line = $3
           AND lease_token = $4",
    )
    .bind(&lease.lookup.platform)
    .bind(&lease.lookup.normalized_game_name)
    .bind(&lease.lookup.normalized_tag_line)
    .bind(lease.token)
    .execute(&mut **transaction)
    .await?;
    if result.rows_affected() != 1 {
        return Err(RiotRecordsRepositoryError::LeaseLost);
    }
    Ok(())
}

fn normalize_platform(value: &str) -> Result<String, RiotRecordsRepositoryError> {
    let value = value.trim().to_ascii_uppercase();
    if !matches!(
        value.as_str(),
        "BR1"
            | "EUN1"
            | "EUW1"
            | "JP1"
            | "KR"
            | "LA1"
            | "LA2"
            | "NA1"
            | "OC1"
            | "PH2"
            | "RU"
            | "SG2"
            | "TH2"
            | "TR1"
            | "TW2"
            | "VN2"
    ) {
        return Err(RiotRecordsRepositoryError::InvalidLookup);
    }
    Ok(value)
}

fn routing_region(platform: &str) -> &'static str {
    match platform {
        "BR1" | "LA1" | "LA2" | "NA1" | "OC1" => "AMERICAS",
        "EUN1" | "EUW1" | "RU" | "TR1" => "EUROPE",
        "JP1" | "KR" => "ASIA",
        "PH2" | "SG2" | "TH2" | "TW2" | "VN2" => "SEA",
        _ => unreachable!("platform was validated before routing"),
    }
}

fn validate_riot_id_component(
    value: &str,
    max_chars: usize,
) -> Result<String, RiotRecordsRepositoryError> {
    let value = value.trim();
    let count = value.chars().count();
    if count == 0 || count > max_chars || value.contains('#') || value.chars().any(char::is_control)
    {
        return Err(RiotRecordsRepositoryError::InvalidLookup);
    }
    Ok(value.to_owned())
}

fn validate_page_key(offset: i32, requested_count: i32) -> Result<(), RiotRecordsRepositoryError> {
    if offset < 0 || requested_count <= 0 {
        return Err(RiotRecordsRepositoryError::InvalidRecentPageKey);
    }
    Ok(())
}

const SNAPSHOT_SELECT: &str = "SELECT snapshot.puuid, snapshot.platform, snapshot.routing_region,
            snapshot.game_name, snapshot.tag_line,
            snapshot.schema_version, snapshot.snapshot, snapshot.refreshed_at
     FROM riot_records_lookup_aliases AS alias
     JOIN riot_records_snapshots AS snapshot
       ON snapshot.platform = alias.snapshot_platform
      AND snapshot.puuid = alias.snapshot_puuid
     WHERE alias.platform = $1
       AND alias.normalized_game_name = $2
       AND alias.normalized_tag_line = $3";

#[derive(Debug, FromRow)]
struct CachedSnapshotRow {
    puuid: String,
    platform: String,
    routing_region: String,
    game_name: String,
    tag_line: String,
    schema_version: i32,
    snapshot: Json<Value>,
    refreshed_at: OffsetDateTime,
}

impl CachedSnapshotRow {
    fn into_record(self) -> CachedRiotRecords {
        CachedRiotRecords {
            puuid: self.puuid,
            platform: self.platform,
            routing_region: self.routing_region,
            game_name: self.game_name,
            tag_line: self.tag_line,
            schema_version: self.schema_version,
            snapshot: self.snapshot.0,
            refreshed_at: self.refreshed_at,
        }
    }
}

#[derive(Debug, FromRow)]
struct CachedRecentPageRow {
    platform: String,
    game_name: String,
    tag_line: String,
    page_offset: i32,
    requested_count: i32,
    schema_version: i32,
    page: Json<Value>,
    fetched_at: OffsetDateTime,
}

impl CachedRecentPageRow {
    fn into_record(self) -> CachedRiotRecordsRecentPage {
        CachedRiotRecordsRecentPage {
            platform: self.platform,
            game_name: self.game_name,
            tag_line: self.tag_line,
            offset: self.page_offset,
            requested_count: self.requested_count,
            schema_version: self.schema_version,
            page: self.page.0,
            fetched_at: self.fetched_at,
        }
    }
}

#[derive(Debug, FromRow)]
struct RecentPageOwner {
    platform: String,
    puuid: String,
    game_name: String,
    tag_line: String,
}

#[derive(Debug, FromRow)]
struct StoredRecentPageRow {
    page_offset: i32,
    requested_count: i32,
    schema_version: i32,
    fetched_at: OffsetDateTime,
}

#[derive(Debug, FromRow)]
struct RefreshGateRow {
    lease_until: Option<OffsetDateTime>,
}

#[derive(Debug, FromRow)]
struct LeaseTimes {
    acquired_at: OffsetDateTime,
    expires_at: OffsetDateTime,
}

#[derive(Debug, FromRow)]
struct StoredSnapshotRow {
    platform: String,
    game_name: String,
    tag_line: String,
    schema_version: i32,
    refreshed_at: OffsetDateTime,
}

#[derive(Debug, Error)]
pub enum RiotRecordsRepositoryError {
    #[error("PostgreSQL Riot records repository operation failed")]
    Sql(#[from] sqlx::Error),
    #[error("Riot records lookup is invalid")]
    InvalidLookup,
    #[error("canonical Riot records identity is invalid")]
    InvalidCanonicalIdentity,
    #[error("Riot records snapshot must be a JSON object")]
    InvalidSnapshot,
    #[error("Riot records recent page key is invalid")]
    InvalidRecentPageKey,
    #[error("Riot records recent page must be a JSON object")]
    InvalidRecentPage,
    #[error("Riot records snapshot was not found")]
    SnapshotNotFound,
    #[error("failed to serialize Riot records snapshot")]
    SnapshotSerialization(#[from] serde_json::Error),
    #[error("Riot records refresh lease is no longer current")]
    LeaseLost,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_normalizes_without_exposing_a_free_form_database_key() {
        let lookup = RiotRecordsLookup::new(" kr ", "  ＬＯＮＧＣＡＴ ", " Kr1 ").unwrap();
        assert_eq!(lookup.platform(), "KR");
        assert_eq!(lookup.game_name(), "ＬＯＮＧＣＡＴ");
        assert_eq!(lookup.tag_line(), "Kr1");
        assert_eq!(lookup.normalized_game_name, "longcat");
        assert_eq!(lookup.normalized_tag_line, "kr1");
    }

    #[test]
    fn invalid_lookup_and_puuid_are_rejected_before_postgres() {
        assert!(RiotRecordsLookup::new("ASIA", "Longcat", "KR1").is_err());
        assert!(RiotRecordsLookup::new("KR", "Longcat#KR1", "KR1").is_err());
        assert!(CanonicalRiotRecordsIdentity::new("KR", "not valid", "Longcat", "KR1").is_err());
    }

    #[test]
    fn canonical_identity_debug_redacts_puuid() {
        let identity =
            CanonicalRiotRecordsIdentity::new("KR", "private-puuid", "Longcat", "KR1").unwrap();
        let debug = format!("{identity:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("private-puuid"));
    }
}
