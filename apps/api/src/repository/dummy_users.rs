use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sqlx::{FromRow, PgConnection, PgPool, Postgres, Transaction};
use thiserror::Error;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use super::{
    TenantScope,
    idempotency::{
        self, AcquireIdempotency, AcquireIdempotencyOutcome, CompleteIdempotency,
        CompleteIdempotencyOutcome, IdempotencyPrincipalType, IdempotencyRepositoryError,
        IdempotentResponse,
    },
};
use crate::{
    auth::AuthService,
    domain::{GuildId, GuildMemberStatus, GuildStatus, IdempotencyRecordId, Lane, UserId},
    infrastructure::postgres::DatabaseTransaction,
};

pub const MAX_BULK_USERS: usize = 100;
pub const MAX_GROUP_MEMBERS: usize = 100;
const MAX_DISPLAY_NAME_CHARS: usize = 100;
const MAX_AVATAR_SEED_CHARS: usize = 128;
const MAX_NOTES_CHARS: usize = 1_000;
const MAX_GROUP_NAME_CHARS: usize = 100;
const MAX_GROUP_DESCRIPTION_CHARS: usize = 500;
const LANES: [Lane; 5] = [
    Lane::Top,
    Lane::Jungle,
    Lane::Mid,
    Lane::Bottom,
    Lane::Support,
];
const IDEMPOTENCY_TTL: Duration = Duration::hours(24);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, sqlx::Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(type_name = "text", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DummyUserState {
    Active,
    Archived,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DummyPrivacySettings {
    pub public_profile_enabled: bool,
    pub show_riot_ids: bool,
    pub allow_external_record_search: bool,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct DummyChampionPreferences {
    #[serde(rename = "TOP")]
    pub top: Vec<String>,
    #[serde(rename = "JUNGLE")]
    pub jungle: Vec<String>,
    #[serde(rename = "MID")]
    pub mid: Vec<String>,
    #[serde(rename = "BOTTOM")]
    pub bottom: Vec<String>,
    #[serde(rename = "SUPPORT")]
    pub support: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DummyLaneMmr {
    pub top: f64,
    pub jungle: f64,
    pub mid: f64,
    pub bottom: f64,
    pub support: f64,
}

impl Default for DummyLaneMmr {
    fn default() -> Self {
        Self {
            top: 1500.0,
            jungle: 1500.0,
            mid: 1500.0,
            bottom: 1500.0,
            support: 1500.0,
        }
    }
}

impl DummyLaneMmr {
    fn value(&self, lane: Lane) -> f64 {
        match lane {
            Lane::Top => self.top,
            Lane::Jungle => self.jungle,
            Lane::Mid => self.mid,
            Lane::Bottom => self.bottom,
            Lane::Support => self.support,
        }
    }

    fn set(&mut self, lane: Lane, value: f64) {
        match lane {
            Lane::Top => self.top = value,
            Lane::Jungle => self.jungle = value,
            Lane::Mid => self.mid = value,
            Lane::Bottom => self.bottom = value,
            Lane::Support => self.support = value,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DummyGuildMembership {
    pub guild_id: GuildId,
    pub guild_name: String,
    pub display_name: String,
    pub status: GuildMemberStatus,
    pub main_lane: Option<Lane>,
    pub lane_mmr: Option<DummyLaneMmr>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpsertDummyRiotAccountCommand {
    pub platform: String,
    pub routing_region: String,
    pub game_name: String,
    pub tag_line: String,
    #[serde(default)]
    pub profile_icon_id: Option<i32>,
    #[serde(default)]
    pub main_position: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DummyRiotAccount {
    pub platform: String,
    pub routing_region: String,
    pub game_name: String,
    pub tag_line: String,
    pub profile_icon_id: Option<i32>,
    pub main_position: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DummyUser {
    pub user_id: UserId,
    pub identity_kind: String,
    pub stable_key: String,
    pub display_name: String,
    pub avatar_seed: Option<String>,
    pub notes: Option<String>,
    pub state: DummyUserState,
    pub revision: i64,
    pub privacy: DummyPrivacySettings,
    pub riot_account: Option<DummyRiotAccount>,
    pub champion_preferences: DummyChampionPreferences,
    pub guild_memberships: Vec<DummyGuildMembership>,
    pub group_ids: Vec<Uuid>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DummyUsersPage {
    pub users: Vec<DummyUser>,
    pub total: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateDummyUserCommand {
    pub stable_key: String,
    pub display_name: String,
    #[serde(default)]
    pub avatar_seed: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub privacy: DummyPrivacySettings,
    #[serde(default)]
    pub guild_memberships: Vec<CreateDummyGuildMembershipInput>,
    #[serde(default)]
    pub riot_account: Option<UpsertDummyRiotAccountCommand>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateDummyUserCommand {
    pub expected_revision: i64,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub avatar_seed: Option<Option<String>>,
    #[serde(default)]
    pub notes: Option<Option<String>>,
    #[serde(default)]
    pub privacy: Option<DummyPrivacySettings>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExpectedRevisionCommand {
    pub expected_revision: i64,
}

#[derive(Clone, Debug, Default)]
pub struct DummyUserListFilter {
    pub query: Option<String>,
    pub state: Option<DummyUserState>,
    pub guild_id: Option<GuildId>,
    pub group_id: Option<Uuid>,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpsertDummyGuildMembershipCommand {
    pub display_name: String,
    pub status: GuildMemberStatus,
    #[serde(default)]
    pub main_lane: Option<Lane>,
    #[serde(default)]
    pub lane_mmr: Option<DummyLaneMmr>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateDummyGuildMembershipInput {
    pub guild_id: GuildId,
    #[serde(flatten)]
    pub membership: UpsertDummyGuildMembershipCommand,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DummyGuildCatalog {
    pub guilds: Vec<DummyGuildCatalogItem>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DummyDeletionImpact {
    pub can_delete: bool,
    pub blocking_references: DummyDeletionBlockingReferences,
    pub removable_references: DummyDeletionRemovableReferences,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DummyDeletionBlockingReferences {
    pub guild_memberships: i64,
    pub recruitment_participants: i64,
    pub auction_participants: i64,
    pub game_participants: i64,
    pub user_sessions: i64,
    pub discord_identities: i64,
    pub real_riot_accounts: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DummyDeletionRemovableReferences {
    pub group_memberships: i64,
    pub champion_preferences: i64,
    pub synthetic_riot_accounts: i64,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DummyGuildCatalogItem {
    pub guild_id: GuildId,
    pub display_name: String,
    pub status: GuildStatus,
    pub active_season_id: Option<Uuid>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DummyGroupMember {
    pub user_id: UserId,
    pub display_name: String,
    pub position: i16,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DummyUserGroup {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub revision: i64,
    pub member_count: i64,
    pub members: Vec<DummyGroupMember>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DummyUserGroupsData {
    pub groups: Vec<DummyUserGroup>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateDummyUserGroupCommand {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub member_ids: Vec<UserId>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateDummyUserGroupCommand {
    pub expected_revision: i64,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<Option<String>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplaceDummyGroupMembersCommand {
    pub expected_revision: i64,
    pub member_ids: Vec<UserId>,
}

pub async fn list(
    pool: &PgPool,
    filter: DummyUserListFilter,
) -> Result<DummyUsersPage, DummyUsersRepositoryError> {
    let query = filter
        .query
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let state = filter.state.map(|state| match state {
        DummyUserState::Active => "ACTIVE",
        DummyUserState::Archived => "ARCHIVED",
    });
    let rows = sqlx::query_as::<_, DummyUserRow>(
        "SELECT user_record.user_id,
                user_record.display_name,
                profile.stable_key,
                profile.avatar_seed,
                profile.notes,
                profile.state,
                profile.revision,
                COALESCE(privacy.public_profile_enabled, false) AS public_profile_enabled,
                COALESCE(privacy.show_riot_ids, false) AS show_riot_ids,
                COALESCE(privacy.allow_external_record_search, false)
                    AS allow_external_record_search,
                profile.created_at,
                profile.updated_at
         FROM dummy_user_profiles AS profile
         JOIN users AS user_record
           ON user_record.user_id = profile.user_id
          AND user_record.identity_kind = 'DUMMY'
         LEFT JOIN user_privacy_settings AS privacy ON privacy.user_id = profile.user_id
         WHERE ($1::text IS NULL OR profile.state = $1)
           AND ($2::text IS NULL OR
                profile.stable_key ILIKE '%' || $2 || '%' OR
                user_record.display_name ILIKE '%' || $2 || '%')
           AND ($3::text IS NULL OR EXISTS (
               SELECT 1 FROM guild_members AS member
               WHERE member.user_id = profile.user_id AND member.guild_id = $3
           ))
           AND ($4::uuid IS NULL OR EXISTS (
               SELECT 1 FROM dummy_user_group_members AS grouped
               WHERE grouped.user_id = profile.user_id AND grouped.group_id = $4
           ))
         ORDER BY profile.updated_at DESC, profile.user_id
         LIMIT $5 OFFSET $6",
    )
    .bind(state)
    .bind(query)
    .bind(filter.guild_id.as_ref())
    .bind(filter.group_id)
    .bind(filter.limit)
    .bind(filter.offset)
    .fetch_all(pool)
    .await?;
    let total = sqlx::query_scalar::<_, i64>(
        "SELECT count(*)
         FROM dummy_user_profiles AS profile
         JOIN users AS user_record ON user_record.user_id = profile.user_id
         WHERE ($1::text IS NULL OR profile.state = $1)
           AND ($2::text IS NULL OR
                profile.stable_key ILIKE '%' || $2 || '%' OR
                user_record.display_name ILIKE '%' || $2 || '%')
           AND ($3::text IS NULL OR EXISTS (
               SELECT 1 FROM guild_members AS member
               WHERE member.user_id = profile.user_id AND member.guild_id = $3
           ))
           AND ($4::uuid IS NULL OR EXISTS (
               SELECT 1 FROM dummy_user_group_members AS grouped
               WHERE grouped.user_id = profile.user_id AND grouped.group_id = $4
           ))",
    )
    .bind(state)
    .bind(query)
    .bind(filter.guild_id.as_ref())
    .bind(filter.group_id)
    .fetch_one(pool)
    .await?;

    let users = fetch_user_page_relations(pool, rows).await?;
    Ok(DummyUsersPage { users, total })
}

pub async fn get(pool: &PgPool, user_id: &UserId) -> Result<DummyUser, DummyUsersRepositoryError> {
    fetch_user(pool, user_id).await
}

pub async fn create(
    pool: &PgPool,
    actor_id: &UserId,
    request_id: &str,
    idempotency_key: &str,
    command: CreateDummyUserCommand,
) -> Result<DummyUser, DummyUsersRepositoryError> {
    validate_create(&command)?;
    let request_hash = request_hash(&command)?;
    let mut transaction = pool.begin().await?;
    let scope = TenantScope::global();
    let record_id = match acquire_idempotency::<StoredDummyUser>(
        &mut transaction,
        &scope,
        actor_id,
        "POST",
        "/api/dev/dummy-users",
        idempotency_key,
        &request_hash,
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(stored) => {
            transaction.commit().await?;
            return Ok(stored.user);
        }
    };
    let user_id = create_in_transaction(&mut transaction, actor_id, request_id, &command).await?;
    let snapshot = fetch_user_in_transaction(&mut transaction, &user_id).await?;
    complete_idempotency(
        &mut transaction,
        &scope,
        record_id,
        201,
        &StoredDummyUser {
            user: snapshot.clone(),
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(snapshot)
}

pub async fn create_bulk(
    pool: &PgPool,
    actor_id: &UserId,
    request_id: &str,
    idempotency_key: &str,
    commands: Vec<CreateDummyUserCommand>,
) -> Result<Vec<DummyUser>, DummyUsersRepositoryError> {
    if commands.is_empty() || commands.len() > MAX_BULK_USERS {
        return Err(DummyUsersRepositoryError::InvalidInput { field: "users" });
    }
    let mut stable_keys = HashSet::with_capacity(commands.len());
    for command in &commands {
        validate_create(command)?;
        if !stable_keys.insert(command.stable_key.as_str()) {
            return Err(DummyUsersRepositoryError::DuplicateStableKey);
        }
    }

    let request_hash = request_hash(&commands)?;
    let mut transaction = pool.begin().await?;
    let scope = TenantScope::global();
    let record_id = match acquire_idempotency::<StoredDummyUsers>(
        &mut transaction,
        &scope,
        actor_id,
        "POST",
        "/api/dev/dummy-users/bulk",
        idempotency_key,
        &request_hash,
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(stored) => {
            transaction.commit().await?;
            return Ok(stored.users);
        }
    };
    let mut ids = Vec::with_capacity(commands.len());
    for command in &commands {
        ids.push(create_in_transaction(&mut transaction, actor_id, request_id, command).await?);
    }
    let mut snapshots = Vec::with_capacity(ids.len());
    for id in &ids {
        snapshots.push(fetch_user_in_transaction(&mut transaction, id).await?);
    }
    complete_idempotency(
        &mut transaction,
        &scope,
        record_id,
        201,
        &StoredDummyUsers {
            users: snapshots.clone(),
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(snapshots)
}

pub async fn update(
    pool: &PgPool,
    user_id: &UserId,
    actor_id: &UserId,
    request_id: &str,
    idempotency_key: &str,
    command: UpdateDummyUserCommand,
) -> Result<DummyUser, DummyUsersRepositoryError> {
    validate_update(&command)?;
    let request_hash = request_hash(&(user_id, &command))?;
    let mut transaction = pool.begin().await?;
    let scope = TenantScope::global();
    let record_id = match acquire_idempotency::<StoredDummyUser>(
        &mut transaction,
        &scope,
        actor_id,
        "PATCH",
        "/api/dev/dummy-users/{userId}",
        idempotency_key,
        &request_hash,
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(stored) => {
            transaction.commit().await?;
            return Ok(stored.user);
        }
    };
    let current = lock_profile(&mut transaction, user_id).await?;
    ensure_revision(command.expected_revision, current.revision)?;
    let display_name = command
        .display_name
        .as_deref()
        .unwrap_or(&current.display_name);
    let avatar_seed = command
        .avatar_seed
        .as_ref()
        .map_or(current.avatar_seed.as_deref(), |value| value.as_deref());
    let notes = command
        .notes
        .as_ref()
        .map_or(current.notes.as_deref(), |value| value.as_deref());
    let now = OffsetDateTime::now_utc();
    sqlx::query(
        "UPDATE users SET display_name = $1, updated_at = $2
         WHERE user_id = $3 AND identity_kind = 'DUMMY'",
    )
    .bind(display_name)
    .bind(now)
    .bind(user_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE dummy_user_profiles
         SET avatar_seed = $1, notes = $2, revision = revision + 1,
             updated_by = $3, updated_at = $4
         WHERE user_id = $5",
    )
    .bind(avatar_seed)
    .bind(notes)
    .bind(actor_id)
    .bind(now)
    .bind(user_id)
    .execute(&mut *transaction)
    .await?;
    if let Some(privacy) = &command.privacy {
        upsert_privacy(&mut transaction, user_id, privacy, now).await?;
    }
    insert_audit(
        &mut transaction,
        actor_id,
        "DUMMY_USER_UPDATED",
        "DUMMY_USER",
        user_id.as_str(),
        request_id,
        Some(json!({
            "displayName": current.display_name,
            "avatarSeed": current.avatar_seed,
            "notes": current.notes,
            "revision": current.revision,
        })),
        Some(json!({ "revision": current.revision + 1 })),
        now,
    )
    .await?;
    let snapshot = fetch_user_in_transaction(&mut transaction, user_id).await?;
    complete_idempotency(
        &mut transaction,
        &scope,
        record_id,
        200,
        &StoredDummyUser {
            user: snapshot.clone(),
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(snapshot)
}

pub async fn set_archived(
    pool: &PgPool,
    user_id: &UserId,
    actor_id: &UserId,
    request_id: &str,
    idempotency_key: &str,
    expected_revision: i64,
    archived: bool,
) -> Result<DummyUser, DummyUsersRepositoryError> {
    if expected_revision < 0 {
        return Err(DummyUsersRepositoryError::InvalidExpectedRevision);
    }
    let request_hash = request_hash(&(user_id, expected_revision, archived))?;
    let mut transaction = pool.begin().await?;
    let scope = TenantScope::global();
    let record_id = match acquire_idempotency::<StoredDummyUser>(
        &mut transaction,
        &scope,
        actor_id,
        "POST",
        if archived {
            "/api/dev/dummy-users/{userId}/archive"
        } else {
            "/api/dev/dummy-users/{userId}/restore"
        },
        idempotency_key,
        &request_hash,
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(stored) => {
            transaction.commit().await?;
            return Ok(stored.user);
        }
    };
    let current = lock_profile(&mut transaction, user_id).await?;
    ensure_revision(expected_revision, current.revision)?;
    let desired = if archived {
        DummyUserState::Archived
    } else {
        DummyUserState::Active
    };
    if current.state == desired {
        let snapshot = fetch_user_in_transaction(&mut transaction, user_id).await?;
        complete_idempotency(
            &mut transaction,
            &scope,
            record_id,
            200,
            &StoredDummyUser {
                user: snapshot.clone(),
            },
        )
        .await?;
        transaction.commit().await?;
        return Ok(snapshot);
    }

    if archived {
        let (recruitments, auction_recruitments) = sqlx::query_as::<_, (i64, i64)>(
            "SELECT
                 (SELECT count(*)
                  FROM recruitment_participants AS participant
                  JOIN recruitments AS recruitment
                    ON recruitment.guild_id = participant.guild_id
                   AND recruitment.id = participant.recruitment_id
                  WHERE participant.user_id = $1
                    AND participant.status = 'JOINED'
                    AND recruitment.status IN ('OPEN', 'CLOSED')),
                 (SELECT count(*)
                  FROM auction_recruitment_participants AS participant
                  JOIN auction_recruitments AS recruitment
                    ON recruitment.guild_id = participant.guild_id
                   AND recruitment.id = participant.recruitment_id
                  WHERE participant.user_id = $1
                    AND participant.status = 'JOINED'
                    AND recruitment.status IN ('OPEN', 'CLOSED'))",
        )
        .bind(user_id)
        .fetch_one(&mut *transaction)
        .await?;
        if recruitments > 0 || auction_recruitments > 0 {
            return Err(
                DummyUsersRepositoryError::ArchiveBlockedByActiveRecruitment {
                    recruitments,
                    auction_recruitments,
                },
            );
        }
    }

    let candidate_guild_ids = if archived {
        sqlx::query_scalar::<_, GuildId>(
            "SELECT guild_id
             FROM guild_members
             WHERE user_id = $1 AND status = 'ACTIVE'
             ORDER BY guild_id",
        )
        .bind(user_id)
        .fetch_all(&mut *transaction)
        .await?
    } else {
        Vec::new()
    };
    let now = OffsetDateTime::now_utc();
    sqlx::query(
        "UPDATE dummy_user_profiles
         SET state = $1, revision = revision + 1, updated_by = $2, updated_at = $3
         WHERE user_id = $4",
    )
    .bind(desired)
    .bind(actor_id)
    .bind(now)
    .bind(user_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query("UPDATE users SET deleted_at = NULL, updated_at = $1 WHERE user_id = $2")
        .bind(now)
        .bind(user_id)
        .execute(&mut *transaction)
        .await?;
    let mut inactivated_guild_ids = Vec::with_capacity(candidate_guild_ids.len());
    if archived {
        for guild_id in candidate_guild_ids {
            lock_members_state(&mut transaction, &guild_id, now).await?;
            let current_status = sqlx::query_scalar::<_, GuildMemberStatus>(
                "SELECT status
                 FROM guild_members
                 WHERE guild_id = $1 AND user_id = $2
                 FOR UPDATE",
            )
            .bind(&guild_id)
            .bind(user_id)
            .fetch_optional(&mut *transaction)
            .await?;
            if current_status != Some(GuildMemberStatus::Active) {
                continue;
            }
            let updated = sqlx::query(
                "UPDATE guild_members
                 SET status = 'INACTIVE', inactivated_at = $1
                 WHERE guild_id = $2 AND user_id = $3 AND status = 'ACTIVE'",
            )
            .bind(now)
            .bind(&guild_id)
            .bind(user_id)
            .execute(&mut *transaction)
            .await?;
            if updated.rows_affected() != 1 {
                return Err(DummyUsersRepositoryError::StoredInvariantViolation);
            }
            touch_members_state(&mut transaction, &guild_id, now).await?;
            insert_guild_audit(
                &mut transaction,
                &guild_id,
                actor_id,
                "MEMBER_STATUS_CHANGED",
                user_id,
                request_id,
                Some(json!({
                    "status": GuildMemberStatus::Active,
                    "source": "DUMMY_USER_ARCHIVE",
                })),
                Some(json!({
                    "status": GuildMemberStatus::Inactive,
                    "source": "DUMMY_USER_ARCHIVE",
                })),
                now,
            )
            .await?;
            inactivated_guild_ids.push(guild_id);
        }
    }
    insert_audit(
        &mut transaction,
        actor_id,
        if archived {
            "DUMMY_USER_ARCHIVED"
        } else {
            "DUMMY_USER_RESTORED"
        },
        "DUMMY_USER",
        user_id.as_str(),
        request_id,
        Some(json!({ "state": current.state, "revision": current.revision })),
        Some(json!({
            "state": desired,
            "revision": current.revision + 1,
            "inactivatedGuildIds": inactivated_guild_ids,
        })),
        now,
    )
    .await?;
    let snapshot = fetch_user_in_transaction(&mut transaction, user_id).await?;
    complete_idempotency(
        &mut transaction,
        &scope,
        record_id,
        200,
        &StoredDummyUser {
            user: snapshot.clone(),
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(snapshot)
}

pub async fn upsert_guild_membership(
    pool: &PgPool,
    user_id: &UserId,
    guild_id: &GuildId,
    actor_id: &UserId,
    request_id: &str,
    idempotency_key: &str,
    command: UpsertDummyGuildMembershipCommand,
) -> Result<DummyUser, DummyUsersRepositoryError> {
    validate_membership(&command)?;
    let request_hash = request_hash(&(user_id, guild_id, &command))?;
    let mut transaction = pool.begin().await?;
    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<StoredDummyUser>(
        &mut transaction,
        &scope,
        actor_id,
        "PUT",
        "/api/dev/dummy-users/{userId}/guilds/{guildId}",
        idempotency_key,
        &request_hash,
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(stored) => {
            transaction.commit().await?;
            return Ok(stored.user);
        }
    };
    let profile = lock_profile(&mut transaction, user_id).await?;
    if profile.state == DummyUserState::Archived {
        return Err(DummyUsersRepositoryError::ArchivedUser);
    }
    let guild_exists = sqlx::query_scalar::<_, bool>(
        "SELECT true FROM guilds WHERE guild_id = $1 AND status <> 'LEFT' FOR SHARE",
    )
    .bind(guild_id)
    .fetch_optional(&mut *transaction)
    .await?
    .unwrap_or(false);
    if !guild_exists {
        return Err(DummyUsersRepositoryError::GuildNotFound);
    }
    let now = OffsetDateTime::now_utc();
    let previous = sqlx::query_as::<_, MembershipBeforeRow>(
        "SELECT display_name, status, main_lane
         FROM guild_members WHERE guild_id = $1 AND user_id = $2 FOR UPDATE",
    )
    .bind(guild_id)
    .bind(user_id)
    .fetch_optional(&mut *transaction)
    .await?;
    sqlx::query(
        "INSERT INTO guild_members (
             guild_id, user_id, display_name, status, main_lane,
             first_seen_at, registered_at, discord_joined_at, last_active_at,
             last_synced_at, inactivated_at
         ) VALUES (
             $1, $2, $3, $4, $5, $6, $6, NULL, NULL, NULL,
             CASE WHEN $4 = 'INACTIVE' THEN $6 ELSE NULL END
         )
         ON CONFLICT (guild_id, user_id) DO UPDATE
         SET display_name = EXCLUDED.display_name,
             status = EXCLUDED.status,
             main_lane = EXCLUDED.main_lane,
             registered_at = COALESCE(guild_members.registered_at, EXCLUDED.registered_at),
             inactivated_at = CASE
                 WHEN EXCLUDED.status = 'INACTIVE' THEN EXCLUDED.inactivated_at
                 ELSE NULL
             END",
    )
    .bind(guild_id)
    .bind(user_id)
    .bind(command.display_name.trim())
    .bind(command.status)
    .bind(command.main_lane)
    .bind(now)
    .execute(&mut *transaction)
    .await?;

    if let Some(lane_mmr) = &command.lane_mmr {
        let season_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM seasons
             WHERE guild_id = $1 AND status = 'ACTIVE'
             FOR SHARE",
        )
        .bind(guild_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(DummyUsersRepositoryError::NoActiveSeason)?;
        for lane in LANES {
            sqlx::query(
                "INSERT INTO user_lane_mmr (
                     guild_id, season_id, user_id, lane, mmr,
                     games_played, wins, revision, updated_at
                 ) VALUES ($1, $2, $3, $4, $5, 0, 0, 0, $6)
                 ON CONFLICT (guild_id, season_id, user_id, lane) DO UPDATE
                 SET mmr = EXCLUDED.mmr,
                     revision = user_lane_mmr.revision + 1,
                     updated_at = EXCLUDED.updated_at",
            )
            .bind(guild_id)
            .bind(season_id)
            .bind(user_id)
            .bind(lane)
            .bind(lane_mmr.value(lane))
            .bind(now)
            .execute(&mut *transaction)
            .await?;
        }
    }
    touch_members_state(&mut transaction, guild_id, now).await?;
    insert_guild_audit(
        &mut transaction,
        guild_id,
        actor_id,
        if previous.is_some() {
            "DUMMY_MEMBER_UPDATED"
        } else {
            "DUMMY_MEMBER_ATTACHED"
        },
        user_id,
        request_id,
        previous.as_ref().map(|row| {
            json!({
                "displayName": row.display_name,
                "status": row.status,
                "mainLane": row.main_lane,
            })
        }),
        Some(json!({
            "displayName": command.display_name.trim(),
            "status": command.status,
            "mainLane": command.main_lane,
            "initialMmrProvided": command.lane_mmr.is_some(),
        })),
        now,
    )
    .await?;
    insert_audit(
        &mut transaction,
        actor_id,
        if previous.is_some() {
            "DUMMY_MEMBER_UPDATED"
        } else {
            "DUMMY_MEMBER_ATTACHED"
        },
        "GUILD_MEMBER",
        &format!("{}:{}", guild_id.as_str(), user_id.as_str()),
        request_id,
        previous.map(|row| json!(row)),
        Some(json!({
            "guildId": guild_id,
            "displayName": command.display_name.trim(),
            "status": command.status,
            "mainLane": command.main_lane,
        })),
        now,
    )
    .await?;
    let snapshot = fetch_user_in_transaction(&mut transaction, user_id).await?;
    complete_idempotency(
        &mut transaction,
        &scope,
        record_id,
        200,
        &StoredDummyUser {
            user: snapshot.clone(),
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(snapshot)
}

pub async fn remove_guild_membership(
    pool: &PgPool,
    user_id: &UserId,
    guild_id: &GuildId,
    actor_id: &UserId,
    request_id: &str,
    idempotency_key: &str,
) -> Result<DummyUser, DummyUsersRepositoryError> {
    let request_hash = request_hash(&(user_id, guild_id))?;
    let mut transaction = pool.begin().await?;
    let scope = TenantScope::guild(guild_id.clone());
    let record_id = match acquire_idempotency::<StoredDummyUser>(
        &mut transaction,
        &scope,
        actor_id,
        "DELETE",
        "/api/dev/dummy-users/{userId}/guilds/{guildId}",
        idempotency_key,
        &request_hash,
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(stored) => {
            transaction.commit().await?;
            return Ok(stored.user);
        }
    };
    lock_profile(&mut transaction, user_id).await?;
    let has_history = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1 FROM recruitment_participants
             WHERE guild_id = $1 AND user_id = $2
         ) OR EXISTS (
             SELECT 1 FROM game_participants
             WHERE guild_id = $1 AND user_id = $2
         ) OR EXISTS (
             SELECT 1 FROM auction_recruitment_participants
             WHERE guild_id = $1 AND user_id = $2
         ) OR EXISTS (
             SELECT 1 FROM mmr_ledger_entries
             WHERE guild_id = $1 AND user_id = $2
         ) OR EXISTS (
             SELECT 1 FROM series_participants
             WHERE guild_id = $1 AND user_id = $2
         ) OR EXISTS (
             SELECT 1 FROM auction_bid_candidates
             WHERE guild_id = $1 AND user_id = $2
         ) OR EXISTS (
             SELECT 1 FROM auction_team_members
             WHERE guild_id = $1 AND user_id = $2
         ) OR EXISTS (
             SELECT 1 FROM game_input_picks
             WHERE guild_id = $1 AND user_id = $2
         )",
    )
    .bind(guild_id)
    .bind(user_id)
    .fetch_one(&mut *transaction)
    .await?;
    if has_history {
        return Err(DummyUsersRepositoryError::MembershipHasHistory);
    }
    sqlx::query("DELETE FROM user_lane_mmr WHERE guild_id = $1 AND user_id = $2")
        .bind(guild_id)
        .bind(user_id)
        .execute(&mut *transaction)
        .await
        .map_err(map_membership_delete_error)?;
    let removed = sqlx::query("DELETE FROM guild_members WHERE guild_id = $1 AND user_id = $2")
        .bind(guild_id)
        .bind(user_id)
        .execute(&mut *transaction)
        .await
        .map_err(map_membership_delete_error)?;
    if removed.rows_affected() != 1 {
        return Err(DummyUsersRepositoryError::MembershipNotFound);
    }
    let now = OffsetDateTime::now_utc();
    touch_members_state(&mut transaction, guild_id, now).await?;
    insert_guild_audit(
        &mut transaction,
        guild_id,
        actor_id,
        "DUMMY_MEMBER_DETACHED",
        user_id,
        request_id,
        None,
        None,
        now,
    )
    .await?;
    insert_audit(
        &mut transaction,
        actor_id,
        "DUMMY_MEMBER_DETACHED",
        "GUILD_MEMBER",
        &format!("{}:{}", guild_id.as_str(), user_id.as_str()),
        request_id,
        None,
        None,
        now,
    )
    .await?;
    let snapshot = fetch_user_in_transaction(&mut transaction, user_id).await?;
    complete_idempotency(
        &mut transaction,
        &scope,
        record_id,
        200,
        &StoredDummyUser {
            user: snapshot.clone(),
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(snapshot)
}

pub async fn replace_champion_preferences(
    pool: &PgPool,
    user_id: &UserId,
    actor_id: &UserId,
    request_id: &str,
    idempotency_key: &str,
    lane: Lane,
    champion_ids: Vec<String>,
) -> Result<DummyUser, DummyUsersRepositoryError> {
    validate_champion_ids(&champion_ids)?;
    let request_hash = request_hash(&(user_id, lane, &champion_ids))?;
    let mut transaction = pool.begin().await?;
    let scope = TenantScope::global();
    let record_id = match acquire_idempotency::<StoredDummyUser>(
        &mut transaction,
        &scope,
        actor_id,
        "PUT",
        "/api/dev/dummy-users/{userId}/champion-preferences/{lane}",
        idempotency_key,
        &request_hash,
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(stored) => {
            transaction.commit().await?;
            return Ok(stored.user);
        }
    };
    let profile = lock_profile(&mut transaction, user_id).await?;
    if profile.state == DummyUserState::Archived {
        return Err(DummyUsersRepositoryError::ArchivedUser);
    }
    sqlx::query("DELETE FROM user_champion_preferences WHERE user_id = $1 AND lane = $2")
        .bind(user_id)
        .bind(lane)
        .execute(&mut *transaction)
        .await?;
    sqlx::query(
        "INSERT INTO user_champion_preferences (user_id, lane, position, champion_id)
         SELECT $1, $2, requested.position::smallint, requested.champion_id
         FROM unnest($3::text[]) WITH ORDINALITY AS requested(champion_id, position)",
    )
    .bind(user_id)
    .bind(lane)
    .bind(&champion_ids)
    .execute(&mut *transaction)
    .await?;
    let now = OffsetDateTime::now_utc();
    sqlx::query(
        "UPDATE dummy_user_profiles
         SET revision = revision + 1, updated_by = $1, updated_at = $2
         WHERE user_id = $3",
    )
    .bind(actor_id)
    .bind(now)
    .bind(user_id)
    .execute(&mut *transaction)
    .await?;
    insert_audit(
        &mut transaction,
        actor_id,
        "DUMMY_CHAMPION_PREFERENCES_UPDATED",
        "DUMMY_USER",
        user_id.as_str(),
        request_id,
        None,
        Some(json!({ "lane": lane, "championIds": champion_ids })),
        now,
    )
    .await?;
    let snapshot = fetch_user_in_transaction(&mut transaction, user_id).await?;
    complete_idempotency(
        &mut transaction,
        &scope,
        record_id,
        200,
        &StoredDummyUser {
            user: snapshot.clone(),
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(snapshot)
}

pub async fn upsert_riot_account(
    pool: &PgPool,
    user_id: &UserId,
    actor_id: &UserId,
    request_id: &str,
    idempotency_key: &str,
    command: UpsertDummyRiotAccountCommand,
) -> Result<DummyUser, DummyUsersRepositoryError> {
    validate_riot_account(&command)?;
    let request_hash = request_hash(&(user_id, &command))?;
    let mut transaction = pool.begin().await?;
    let scope = TenantScope::global();
    let record_id = match acquire_idempotency::<StoredDummyUser>(
        &mut transaction,
        &scope,
        actor_id,
        "PUT",
        "/api/dev/dummy-users/{userId}/riot-account",
        idempotency_key,
        &request_hash,
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(stored) => {
            transaction.commit().await?;
            return Ok(stored.user);
        }
    };
    let profile = lock_profile(&mut transaction, user_id).await?;
    if profile.state == DummyUserState::Archived {
        return Err(DummyUsersRepositoryError::ArchivedUser);
    }
    let existing_puuid = sqlx::query_scalar::<_, String>(
        "SELECT puuid FROM riot_accounts
         WHERE user_id = $1 AND origin = 'DUMMY' FOR UPDATE",
    )
    .bind(user_id)
    .fetch_optional(&mut *transaction)
    .await?;
    let puuid = existing_puuid.unwrap_or_else(|| format!("dummy:{}", Uuid::now_v7()));
    let normalized_game_name = super::riot::normalize_riot_id_v1(&command.game_name);
    let normalized_tag_line = super::riot::normalize_riot_id_v1(&command.tag_line);
    let now = OffsetDateTime::now_utc();
    sqlx::query(
        "INSERT INTO riot_accounts (
             puuid, user_id, platform, routing_region, game_name, tag_line,
             normalized_game_name, normalized_tag_line, profile_icon_id, is_main,
             main_position, main_position_updated_at, last_synced_at,
             created_at, updated_at, origin
         ) VALUES (
             $1, $2, $3, $4, $5, $6, $7, $8, $9, true,
             $10, CASE WHEN $10::text IS NULL THEN NULL ELSE $11 END,
             NULL, $11, $11, 'DUMMY'
         )
         ON CONFLICT (puuid) DO UPDATE
         SET platform = EXCLUDED.platform,
             routing_region = EXCLUDED.routing_region,
             game_name = EXCLUDED.game_name,
             tag_line = EXCLUDED.tag_line,
             normalized_game_name = EXCLUDED.normalized_game_name,
             normalized_tag_line = EXCLUDED.normalized_tag_line,
             profile_icon_id = EXCLUDED.profile_icon_id,
             main_position = EXCLUDED.main_position,
             main_position_updated_at = EXCLUDED.main_position_updated_at,
             updated_at = EXCLUDED.updated_at
         WHERE riot_accounts.user_id = EXCLUDED.user_id
           AND riot_accounts.origin = 'DUMMY'",
    )
    .bind(&puuid)
    .bind(user_id)
    .bind(command.platform.trim())
    .bind(command.routing_region.trim())
    .bind(command.game_name.trim())
    .bind(command.tag_line.trim())
    .bind(normalized_game_name)
    .bind(normalized_tag_line)
    .bind(command.profile_icon_id)
    .bind(command.main_position.as_deref())
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE dummy_user_profiles
         SET revision = revision + 1, updated_by = $1, updated_at = $2
         WHERE user_id = $3",
    )
    .bind(actor_id)
    .bind(now)
    .bind(user_id)
    .execute(&mut *transaction)
    .await?;
    insert_audit(
        &mut transaction,
        actor_id,
        "DUMMY_RIOT_ACCOUNT_UPSERTED",
        "DUMMY_USER",
        user_id.as_str(),
        request_id,
        None,
        Some(json!({
            "platform": command.platform.trim(),
            "routingRegion": command.routing_region.trim(),
            "gameName": command.game_name.trim(),
            "tagLine": command.tag_line.trim(),
        })),
        now,
    )
    .await?;
    let snapshot = fetch_user_in_transaction(&mut transaction, user_id).await?;
    complete_idempotency(
        &mut transaction,
        &scope,
        record_id,
        200,
        &StoredDummyUser {
            user: snapshot.clone(),
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(snapshot)
}

pub async fn remove_riot_account(
    pool: &PgPool,
    user_id: &UserId,
    actor_id: &UserId,
    request_id: &str,
    idempotency_key: &str,
) -> Result<DummyUser, DummyUsersRepositoryError> {
    let request_hash = request_hash(user_id)?;
    let mut transaction = pool.begin().await?;
    let scope = TenantScope::global();
    let record_id = match acquire_idempotency::<StoredDummyUser>(
        &mut transaction,
        &scope,
        actor_id,
        "DELETE",
        "/api/dev/dummy-users/{userId}/riot-account",
        idempotency_key,
        &request_hash,
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(stored) => {
            transaction.commit().await?;
            return Ok(stored.user);
        }
    };
    lock_profile(&mut transaction, user_id).await?;
    let removed = sqlx::query(
        "DELETE FROM riot_accounts
         WHERE user_id = $1 AND origin = 'DUMMY'",
    )
    .bind(user_id)
    .execute(&mut *transaction)
    .await?;
    if removed.rows_affected() != 1 {
        return Err(DummyUsersRepositoryError::RiotAccountNotFound);
    }
    let now = OffsetDateTime::now_utc();
    sqlx::query(
        "UPDATE dummy_user_profiles
         SET revision = revision + 1, updated_by = $1, updated_at = $2
         WHERE user_id = $3",
    )
    .bind(actor_id)
    .bind(now)
    .bind(user_id)
    .execute(&mut *transaction)
    .await?;
    insert_audit(
        &mut transaction,
        actor_id,
        "DUMMY_RIOT_ACCOUNT_REMOVED",
        "DUMMY_USER",
        user_id.as_str(),
        request_id,
        None,
        None,
        now,
    )
    .await?;
    let snapshot = fetch_user_in_transaction(&mut transaction, user_id).await?;
    complete_idempotency(
        &mut transaction,
        &scope,
        record_id,
        200,
        &StoredDummyUser {
            user: snapshot.clone(),
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(snapshot)
}

pub async fn guild_catalog(pool: &PgPool) -> Result<DummyGuildCatalog, DummyUsersRepositoryError> {
    let guilds = sqlx::query_as::<_, DummyGuildCatalogItem>(
        "SELECT guild.guild_id,
                COALESCE(config.display_name, guild.discord_name) AS display_name,
                guild.status,
                season.id AS active_season_id
         FROM guilds AS guild
         LEFT JOIN guild_config AS config ON config.guild_id = guild.guild_id
         LEFT JOIN seasons AS season
           ON season.guild_id = guild.guild_id AND season.status = 'ACTIVE'
         WHERE guild.status <> 'LEFT'
         ORDER BY display_name, guild.guild_id",
    )
    .fetch_all(pool)
    .await?;
    Ok(DummyGuildCatalog { guilds })
}

pub async fn deletion_impact(
    pool: &PgPool,
    user_id: &UserId,
) -> Result<DummyDeletionImpact, DummyUsersRepositoryError> {
    let exists =
        sqlx::query_scalar::<_, bool>("SELECT true FROM dummy_user_profiles WHERE user_id = $1")
            .bind(user_id)
            .fetch_optional(pool)
            .await?
            .unwrap_or(false);
    if !exists {
        return Err(DummyUsersRepositoryError::UserNotFound);
    }
    fetch_deletion_impact(pool, user_id).await
}

pub async fn delete_user(
    pool: &PgPool,
    user_id: &UserId,
    actor_id: &UserId,
    request_id: &str,
    idempotency_key: &str,
) -> Result<DummyDeletionImpact, DummyUsersRepositoryError> {
    let request_hash = request_hash(user_id)?;
    let mut transaction = pool.begin().await?;
    let scope = TenantScope::global();
    let record_id = match acquire_idempotency::<DummyDeletionImpact>(
        &mut transaction,
        &scope,
        actor_id,
        "DELETE",
        "/api/dev/dummy-users/{userId}",
        idempotency_key,
        &request_hash,
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(impact) => {
            transaction.commit().await?;
            return Ok(impact);
        }
    };
    lock_profile(&mut transaction, user_id).await?;
    let impact = fetch_deletion_impact(&mut *transaction, user_id).await?;
    if !impact.can_delete {
        return Err(DummyUsersRepositoryError::DeletionBlocked(impact));
    }
    let now = OffsetDateTime::now_utc();
    insert_audit(
        &mut transaction,
        actor_id,
        "DUMMY_USER_DELETED",
        "DUMMY_USER",
        user_id.as_str(),
        request_id,
        Some(json!({ "deletionImpact": impact })),
        None,
        now,
    )
    .await?;
    sqlx::query("DELETE FROM dummy_user_group_members WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM user_champion_preferences WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM riot_accounts WHERE user_id = $1 AND origin = 'DUMMY'")
        .bind(user_id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM user_privacy_settings WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM user_guild_preferences WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM dummy_user_profiles WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM users WHERE user_id = $1 AND identity_kind = 'DUMMY'")
        .bind(user_id)
        .execute(&mut *transaction)
        .await?;
    complete_idempotency(&mut transaction, &scope, record_id, 200, &impact).await?;
    transaction.commit().await?;
    Ok(impact)
}

pub async fn list_groups(pool: &PgPool) -> Result<DummyUserGroupsData, DummyUsersRepositoryError> {
    let ids =
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM dummy_user_groups ORDER BY lower(name), id")
            .fetch_all(pool)
            .await?;
    let mut groups = Vec::with_capacity(ids.len());
    for id in ids {
        groups.push(fetch_group(pool, id).await?);
    }
    Ok(DummyUserGroupsData { groups })
}

pub async fn create_group(
    pool: &PgPool,
    actor_id: &UserId,
    request_id: &str,
    idempotency_key: &str,
    command: CreateDummyUserGroupCommand,
) -> Result<DummyUserGroup, DummyUsersRepositoryError> {
    validate_group(&command.name, command.description.as_deref())?;
    validate_member_ids(&command.member_ids)?;
    let request_hash = request_hash(&command)?;
    let mut transaction = pool.begin().await?;
    let scope = TenantScope::global();
    let record_id = match acquire_idempotency::<StoredDummyGroup>(
        &mut transaction,
        &scope,
        actor_id,
        "POST",
        "/api/dev/dummy-groups",
        idempotency_key,
        &request_hash,
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(stored) => {
            transaction.commit().await?;
            return Ok(stored.group);
        }
    };
    ensure_active_dummy_users(&mut transaction, &command.member_ids).await?;
    let id = Uuid::now_v7();
    let now = OffsetDateTime::now_utc();
    sqlx::query(
        "INSERT INTO dummy_user_groups (
             id, name, description, revision, created_by, updated_by, created_at, updated_at
         ) VALUES ($1, $2, $3, 0, $4, $4, $5, $5)",
    )
    .bind(id)
    .bind(command.name.trim())
    .bind(trim_optional(command.description.as_deref()))
    .bind(actor_id)
    .bind(now)
    .execute(&mut *transaction)
    .await
    .map_err(map_constraint_error)?;
    replace_group_member_rows(&mut transaction, id, &command.member_ids, now).await?;
    insert_audit(
        &mut transaction,
        actor_id,
        "DUMMY_GROUP_CREATED",
        "DUMMY_USER_GROUP",
        &id.to_string(),
        request_id,
        None,
        Some(json!({ "name": command.name.trim(), "memberIds": command.member_ids })),
        now,
    )
    .await?;
    let snapshot = fetch_group_in_transaction(&mut transaction, id).await?;
    complete_idempotency(
        &mut transaction,
        &scope,
        record_id,
        201,
        &StoredDummyGroup {
            group: snapshot.clone(),
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(snapshot)
}

pub async fn update_group(
    pool: &PgPool,
    group_id: Uuid,
    actor_id: &UserId,
    request_id: &str,
    idempotency_key: &str,
    command: UpdateDummyUserGroupCommand,
) -> Result<DummyUserGroup, DummyUsersRepositoryError> {
    if command.expected_revision < 0 {
        return Err(DummyUsersRepositoryError::InvalidExpectedRevision);
    }
    let request_hash = request_hash(&(group_id, &command))?;
    let mut transaction = pool.begin().await?;
    let scope = TenantScope::global();
    let record_id = match acquire_idempotency::<StoredDummyGroup>(
        &mut transaction,
        &scope,
        actor_id,
        "PATCH",
        "/api/dev/dummy-groups/{groupId}",
        idempotency_key,
        &request_hash,
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(stored) => {
            transaction.commit().await?;
            return Ok(stored.group);
        }
    };
    let current = lock_group(&mut transaction, group_id).await?;
    ensure_revision(command.expected_revision, current.revision)?;
    let name = command.name.as_deref().unwrap_or(&current.name);
    let description = command
        .description
        .as_ref()
        .map_or(current.description.as_deref(), |value| value.as_deref());
    validate_group(name, description)?;
    let now = OffsetDateTime::now_utc();
    sqlx::query(
        "UPDATE dummy_user_groups
         SET name = $1, description = $2, revision = revision + 1,
             updated_by = $3, updated_at = $4
         WHERE id = $5",
    )
    .bind(name.trim())
    .bind(trim_optional(description))
    .bind(actor_id)
    .bind(now)
    .bind(group_id)
    .execute(&mut *transaction)
    .await
    .map_err(map_constraint_error)?;
    insert_audit(
        &mut transaction,
        actor_id,
        "DUMMY_GROUP_UPDATED",
        "DUMMY_USER_GROUP",
        &group_id.to_string(),
        request_id,
        Some(json!({ "name": current.name, "description": current.description, "revision": current.revision })),
        Some(json!({ "name": name.trim(), "description": description, "revision": current.revision + 1 })),
        now,
    )
    .await?;
    let snapshot = fetch_group_in_transaction(&mut transaction, group_id).await?;
    complete_idempotency(
        &mut transaction,
        &scope,
        record_id,
        200,
        &StoredDummyGroup {
            group: snapshot.clone(),
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(snapshot)
}

pub async fn replace_group_members(
    pool: &PgPool,
    group_id: Uuid,
    actor_id: &UserId,
    request_id: &str,
    idempotency_key: &str,
    command: ReplaceDummyGroupMembersCommand,
) -> Result<DummyUserGroup, DummyUsersRepositoryError> {
    if command.expected_revision < 0 {
        return Err(DummyUsersRepositoryError::InvalidExpectedRevision);
    }
    validate_member_ids(&command.member_ids)?;
    let request_hash = request_hash(&(group_id, &command))?;
    let mut transaction = pool.begin().await?;
    let scope = TenantScope::global();
    let record_id = match acquire_idempotency::<StoredDummyGroup>(
        &mut transaction,
        &scope,
        actor_id,
        "PUT",
        "/api/dev/dummy-groups/{groupId}/members",
        idempotency_key,
        &request_hash,
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(stored) => {
            transaction.commit().await?;
            return Ok(stored.group);
        }
    };
    let current = lock_group(&mut transaction, group_id).await?;
    ensure_revision(command.expected_revision, current.revision)?;
    ensure_active_dummy_users(&mut transaction, &command.member_ids).await?;
    let now = OffsetDateTime::now_utc();
    replace_group_member_rows(&mut transaction, group_id, &command.member_ids, now).await?;
    sqlx::query(
        "UPDATE dummy_user_groups
         SET revision = revision + 1, updated_by = $1, updated_at = $2
         WHERE id = $3",
    )
    .bind(actor_id)
    .bind(now)
    .bind(group_id)
    .execute(&mut *transaction)
    .await?;
    insert_audit(
        &mut transaction,
        actor_id,
        "DUMMY_GROUP_MEMBERS_REPLACED",
        "DUMMY_USER_GROUP",
        &group_id.to_string(),
        request_id,
        None,
        Some(json!({ "memberIds": command.member_ids, "revision": current.revision + 1 })),
        now,
    )
    .await?;
    let snapshot = fetch_group_in_transaction(&mut transaction, group_id).await?;
    complete_idempotency(
        &mut transaction,
        &scope,
        record_id,
        200,
        &StoredDummyGroup {
            group: snapshot.clone(),
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(snapshot)
}

pub async fn delete_group(
    pool: &PgPool,
    group_id: Uuid,
    actor_id: &UserId,
    request_id: &str,
    idempotency_key: &str,
) -> Result<(), DummyUsersRepositoryError> {
    let mut transaction = pool.begin().await?;
    let scope = TenantScope::global();
    let request_hash = request_hash(&group_id)?;
    let record_id = match acquire_idempotency::<StoredDeleted>(
        &mut transaction,
        &scope,
        actor_id,
        "DELETE",
        "/api/dev/dummy-groups/{groupId}",
        idempotency_key,
        &request_hash,
    )
    .await?
    {
        IdempotencyDecision::Acquired(record_id) => record_id,
        IdempotencyDecision::Replay(stored) if stored.deleted => {
            transaction.commit().await?;
            return Ok(());
        }
        IdempotencyDecision::Replay(_) => {
            return Err(DummyUsersRepositoryError::StoredInvariantViolation);
        }
    };
    let current = lock_group(&mut transaction, group_id).await?;
    let now = OffsetDateTime::now_utc();
    sqlx::query("DELETE FROM dummy_user_groups WHERE id = $1")
        .bind(group_id)
        .execute(&mut *transaction)
        .await?;
    insert_audit(
        &mut transaction,
        actor_id,
        "DUMMY_GROUP_DELETED",
        "DUMMY_USER_GROUP",
        &group_id.to_string(),
        request_id,
        Some(json!({ "name": current.name, "revision": current.revision })),
        None,
        now,
    )
    .await?;
    complete_idempotency(
        &mut transaction,
        &scope,
        record_id,
        200,
        &StoredDeleted { deleted: true },
    )
    .await?;
    transaction.commit().await?;
    Ok(())
}

async fn create_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    actor_id: &UserId,
    request_id: &str,
    command: &CreateDummyUserCommand,
) -> Result<UserId, DummyUsersRepositoryError> {
    let user_id = UserId::parse(format!("dmy_{}", Uuid::now_v7()))
        .map_err(|_| DummyUsersRepositoryError::StoredInvariantViolation)?;
    let now = OffsetDateTime::now_utc();
    sqlx::query(
        "INSERT INTO users (
             user_id, identity_kind, display_name, avatar_hash, created_at, updated_at, deleted_at
         ) VALUES ($1, 'DUMMY', $2, NULL, $3, $3, NULL)",
    )
    .bind(&user_id)
    .bind(command.display_name.trim())
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO dummy_user_profiles (
             user_id, stable_key, state, avatar_seed, notes, revision,
             created_by, updated_by, created_at, updated_at
         ) VALUES ($1, $2, 'ACTIVE', $3, $4, 0, $5, $5, $6, $6)",
    )
    .bind(&user_id)
    .bind(&command.stable_key)
    .bind(trim_optional(command.avatar_seed.as_deref()))
    .bind(trim_optional(command.notes.as_deref()))
    .bind(actor_id)
    .bind(now)
    .execute(&mut **transaction)
    .await
    .map_err(map_constraint_error)?;
    upsert_privacy(transaction, &user_id, &command.privacy, now).await?;
    sqlx::query(
        "INSERT INTO user_guild_preferences (user_id, default_guild_id, updated_at)
         VALUES ($1, NULL, $2)",
    )
    .bind(&user_id)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    for membership in &command.guild_memberships {
        attach_initial_membership(transaction, &user_id, actor_id, request_id, membership, now)
            .await?;
    }
    if let Some(riot_account) = &command.riot_account {
        upsert_dummy_riot_in_transaction(transaction, &user_id, riot_account, now).await?;
    }
    insert_audit(
        transaction,
        actor_id,
        "DUMMY_USER_CREATED",
        "DUMMY_USER",
        user_id.as_str(),
        request_id,
        None,
        Some(json!({
            "stableKey": command.stable_key,
            "displayName": command.display_name.trim(),
            "privacy": command.privacy,
        })),
        now,
    )
    .await?;
    Ok(user_id)
}

async fn attach_initial_membership(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: &UserId,
    actor_id: &UserId,
    request_id: &str,
    input: &CreateDummyGuildMembershipInput,
    now: OffsetDateTime,
) -> Result<(), DummyUsersRepositoryError> {
    let guild_exists = sqlx::query_scalar::<_, bool>(
        "SELECT true FROM guilds WHERE guild_id = $1 AND status <> 'LEFT' FOR SHARE",
    )
    .bind(&input.guild_id)
    .fetch_optional(&mut **transaction)
    .await?
    .unwrap_or(false);
    if !guild_exists {
        return Err(DummyUsersRepositoryError::GuildNotFound);
    }
    sqlx::query(
        "INSERT INTO guild_members (
             guild_id, user_id, display_name, status, main_lane,
             first_seen_at, registered_at, discord_joined_at, last_active_at,
             last_synced_at, inactivated_at
         ) VALUES (
             $1, $2, $3, $4, $5, $6, $6, NULL, NULL, NULL,
             CASE WHEN $4 = 'INACTIVE' THEN $6 ELSE NULL END
         )",
    )
    .bind(&input.guild_id)
    .bind(user_id)
    .bind(input.membership.display_name.trim())
    .bind(input.membership.status)
    .bind(input.membership.main_lane)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    if let Some(lane_mmr) = &input.membership.lane_mmr {
        let season_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM seasons
             WHERE guild_id = $1 AND status = 'ACTIVE' FOR SHARE",
        )
        .bind(&input.guild_id)
        .fetch_optional(&mut **transaction)
        .await?
        .ok_or(DummyUsersRepositoryError::NoActiveSeason)?;
        for lane in LANES {
            sqlx::query(
                "INSERT INTO user_lane_mmr (
                     guild_id, season_id, user_id, lane, mmr,
                     games_played, wins, revision, updated_at
                 ) VALUES ($1, $2, $3, $4, $5, 0, 0, 0, $6)",
            )
            .bind(&input.guild_id)
            .bind(season_id)
            .bind(user_id)
            .bind(lane)
            .bind(lane_mmr.value(lane))
            .bind(now)
            .execute(&mut **transaction)
            .await?;
        }
    }
    touch_members_state(transaction, &input.guild_id, now).await?;
    insert_guild_audit(
        transaction,
        &input.guild_id,
        actor_id,
        "DUMMY_MEMBER_ATTACHED",
        user_id,
        request_id,
        None,
        Some(json!({
            "displayName": input.membership.display_name.trim(),
            "status": input.membership.status,
            "mainLane": input.membership.main_lane,
            "initialMmrProvided": input.membership.lane_mmr.is_some(),
        })),
        now,
    )
    .await?;
    Ok(())
}

async fn upsert_dummy_riot_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: &UserId,
    command: &UpsertDummyRiotAccountCommand,
    now: OffsetDateTime,
) -> Result<(), DummyUsersRepositoryError> {
    let existing_puuid = sqlx::query_scalar::<_, String>(
        "SELECT puuid FROM riot_accounts
         WHERE user_id = $1 AND origin = 'DUMMY' FOR UPDATE",
    )
    .bind(user_id)
    .fetch_optional(&mut **transaction)
    .await?;
    let puuid = existing_puuid.unwrap_or_else(|| format!("dummy:{}", Uuid::now_v7()));
    sqlx::query(
        "INSERT INTO riot_accounts (
             puuid, user_id, platform, routing_region, game_name, tag_line,
             normalized_game_name, normalized_tag_line, profile_icon_id, is_main,
             main_position, main_position_updated_at, last_synced_at,
             created_at, updated_at, origin
         ) VALUES (
             $1, $2, $3, $4, $5, $6, $7, $8, $9, true,
             $10, CASE WHEN $10::text IS NULL THEN NULL ELSE $11 END,
             NULL, $11, $11, 'DUMMY'
         )
         ON CONFLICT (puuid) DO UPDATE
         SET platform = EXCLUDED.platform,
             routing_region = EXCLUDED.routing_region,
             game_name = EXCLUDED.game_name,
             tag_line = EXCLUDED.tag_line,
             normalized_game_name = EXCLUDED.normalized_game_name,
             normalized_tag_line = EXCLUDED.normalized_tag_line,
             profile_icon_id = EXCLUDED.profile_icon_id,
             main_position = EXCLUDED.main_position,
             main_position_updated_at = EXCLUDED.main_position_updated_at,
             updated_at = EXCLUDED.updated_at
         WHERE riot_accounts.user_id = EXCLUDED.user_id
           AND riot_accounts.origin = 'DUMMY'",
    )
    .bind(&puuid)
    .bind(user_id)
    .bind(command.platform.trim())
    .bind(command.routing_region.trim())
    .bind(command.game_name.trim())
    .bind(command.tag_line.trim())
    .bind(super::riot::normalize_riot_id_v1(&command.game_name))
    .bind(super::riot::normalize_riot_id_v1(&command.tag_line))
    .bind(command.profile_icon_id)
    .bind(command.main_position.as_deref())
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn fetch_user(
    pool: &PgPool,
    user_id: &UserId,
) -> Result<DummyUser, DummyUsersRepositoryError> {
    let mut connection = pool.acquire().await?;
    fetch_user_connection(&mut connection, user_id).await
}

async fn fetch_user_page_relations(
    pool: &PgPool,
    rows: Vec<DummyUserRow>,
) -> Result<Vec<DummyUser>, DummyUsersRepositoryError> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let user_ids = rows
        .iter()
        .map(|row| row.user_id.as_str().to_owned())
        .collect::<Vec<_>>();

    let riot_rows = sqlx::query_as::<_, BatchDummyRiotAccountRow>(
        "SELECT user_id, platform, routing_region, game_name, tag_line,
                profile_icon_id, main_position
         FROM riot_accounts
         WHERE user_id = ANY($1::text[]) AND origin = 'DUMMY'",
    )
    .bind(&user_ids)
    .fetch_all(pool)
    .await?;
    let mut riot_by_user = HashMap::with_capacity(riot_rows.len());
    for row in riot_rows {
        let account = DummyRiotAccount {
            platform: row.platform,
            routing_region: row.routing_region,
            game_name: row.game_name,
            tag_line: row.tag_line,
            profile_icon_id: row.profile_icon_id,
            main_position: row.main_position,
        };
        if riot_by_user.insert(row.user_id, account).is_some() {
            return Err(DummyUsersRepositoryError::StoredInvariantViolation);
        }
    }

    let preference_rows = sqlx::query_as::<_, BatchChampionPreferenceRow>(
        "SELECT user_id, lane, position, champion_id
         FROM user_champion_preferences
         WHERE user_id = ANY($1::text[])
         ORDER BY user_id,
                  CASE lane
                    WHEN 'TOP' THEN 1 WHEN 'JUNGLE' THEN 2 WHEN 'MID' THEN 3
                    WHEN 'BOTTOM' THEN 4 WHEN 'SUPPORT' THEN 5
                  END,
                  position",
    )
    .bind(&user_ids)
    .fetch_all(pool)
    .await?;
    let mut preferences_by_user = HashMap::<UserId, Vec<ChampionPreferenceRow>>::new();
    for row in preference_rows {
        preferences_by_user
            .entry(row.user_id)
            .or_default()
            .push(ChampionPreferenceRow {
                lane: row.lane,
                position: row.position,
                champion_id: row.champion_id,
            });
    }

    let membership_rows = sqlx::query_as::<_, BatchDummyGuildMembershipRow>(
        "SELECT member.user_id,
                member.guild_id,
                COALESCE(config.display_name, guild.discord_name) AS guild_name,
                member.display_name,
                member.status,
                member.main_lane,
                season.id AS active_season_id
         FROM guild_members AS member
         JOIN guilds AS guild ON guild.guild_id = member.guild_id
         LEFT JOIN guild_config AS config ON config.guild_id = member.guild_id
         LEFT JOIN seasons AS season
           ON season.guild_id = member.guild_id AND season.status = 'ACTIVE'
         WHERE member.user_id = ANY($1::text[])
         ORDER BY member.user_id, guild_name, member.guild_id",
    )
    .bind(&user_ids)
    .fetch_all(pool)
    .await?;
    let mut memberships_by_user = HashMap::<UserId, Vec<DummyGuildMembershipRow>>::new();
    for row in membership_rows {
        memberships_by_user
            .entry(row.user_id)
            .or_default()
            .push(DummyGuildMembershipRow {
                guild_id: row.guild_id,
                guild_name: row.guild_name,
                display_name: row.display_name,
                status: row.status,
                main_lane: row.main_lane,
                active_season_id: row.active_season_id,
            });
    }

    let mmr_rows = sqlx::query_as::<_, BatchLaneMmrRow>(
        "SELECT mmr.user_id, mmr.guild_id, mmr.lane, mmr.mmr
         FROM user_lane_mmr AS mmr
         JOIN seasons AS season
           ON season.guild_id = mmr.guild_id
          AND season.id = mmr.season_id
          AND season.status = 'ACTIVE'
         WHERE mmr.user_id = ANY($1::text[])
         ORDER BY mmr.user_id, mmr.guild_id, mmr.lane",
    )
    .bind(&user_ids)
    .fetch_all(pool)
    .await?;
    let mut mmr_by_membership = HashMap::<(UserId, GuildId), Vec<LaneMmrRow>>::new();
    for row in mmr_rows {
        mmr_by_membership
            .entry((row.user_id, row.guild_id))
            .or_default()
            .push(LaneMmrRow {
                lane: row.lane,
                mmr: row.mmr,
            });
    }

    let group_rows = sqlx::query_as::<_, BatchGroupMembershipRow>(
        "SELECT user_id, group_id
         FROM dummy_user_group_members
         WHERE user_id = ANY($1::text[])
         ORDER BY user_id, group_id",
    )
    .bind(&user_ids)
    .fetch_all(pool)
    .await?;
    let mut groups_by_user = HashMap::<UserId, Vec<Uuid>>::new();
    for row in group_rows {
        groups_by_user
            .entry(row.user_id)
            .or_default()
            .push(row.group_id);
    }

    let mut users = Vec::with_capacity(rows.len());
    for row in rows {
        let membership_rows = memberships_by_user.remove(&row.user_id).unwrap_or_default();
        let mut guild_memberships = Vec::with_capacity(membership_rows.len());
        for membership in membership_rows {
            let mmr_rows = mmr_by_membership
                .remove(&(row.user_id.clone(), membership.guild_id.clone()))
                .unwrap_or_default();
            let lane_mmr = if membership.active_season_id.is_some() {
                lane_mmr_from_rows(mmr_rows)?
            } else if mmr_rows.is_empty() {
                None
            } else {
                return Err(DummyUsersRepositoryError::StoredInvariantViolation);
            };
            guild_memberships.push(DummyGuildMembership {
                guild_id: membership.guild_id,
                guild_name: membership.guild_name,
                display_name: membership.display_name,
                status: membership.status,
                main_lane: membership.main_lane,
                lane_mmr,
            });
        }

        users.push(DummyUser {
            riot_account: riot_by_user.remove(&row.user_id),
            champion_preferences: preferences_from_rows(
                preferences_by_user.remove(&row.user_id).unwrap_or_default(),
            )?,
            group_ids: groups_by_user.remove(&row.user_id).unwrap_or_default(),
            guild_memberships,
            user_id: row.user_id,
            identity_kind: "DUMMY".to_owned(),
            stable_key: row.stable_key,
            display_name: row.display_name,
            avatar_seed: row.avatar_seed,
            notes: row.notes,
            state: row.state,
            revision: row.revision,
            privacy: DummyPrivacySettings {
                public_profile_enabled: row.public_profile_enabled,
                show_riot_ids: row.show_riot_ids,
                allow_external_record_search: row.allow_external_record_search,
            },
            created_at: row.created_at,
            updated_at: row.updated_at,
        });
    }
    Ok(users)
}

async fn fetch_user_in_transaction(
    transaction: &mut DatabaseTransaction,
    user_id: &UserId,
) -> Result<DummyUser, DummyUsersRepositoryError> {
    fetch_user_connection(&mut *transaction, user_id).await
}

async fn fetch_user_connection(
    connection: &mut PgConnection,
    user_id: &UserId,
) -> Result<DummyUser, DummyUsersRepositoryError> {
    let row = sqlx::query_as::<_, DummyUserRow>(
        "SELECT user_record.user_id,
                user_record.display_name,
                profile.stable_key,
                profile.avatar_seed,
                profile.notes,
                profile.state,
                profile.revision,
                COALESCE(privacy.public_profile_enabled, false) AS public_profile_enabled,
                COALESCE(privacy.show_riot_ids, false) AS show_riot_ids,
                COALESCE(privacy.allow_external_record_search, false)
                    AS allow_external_record_search,
                profile.created_at,
                profile.updated_at
         FROM dummy_user_profiles AS profile
         JOIN users AS user_record
           ON user_record.user_id = profile.user_id
          AND user_record.identity_kind = 'DUMMY'
         LEFT JOIN user_privacy_settings AS privacy ON privacy.user_id = profile.user_id
         WHERE profile.user_id = $1",
    )
    .bind(user_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(DummyUsersRepositoryError::UserNotFound)?;

    let riot_account = sqlx::query_as::<_, DummyRiotAccountRow>(
        "SELECT platform, routing_region, game_name, tag_line,
                profile_icon_id, main_position
         FROM riot_accounts
         WHERE user_id = $1 AND origin = 'DUMMY'",
    )
    .bind(user_id)
    .fetch_optional(&mut *connection)
    .await?
    .map(DummyRiotAccountRow::into_record);

    let preference_rows = sqlx::query_as::<_, ChampionPreferenceRow>(
        "SELECT lane, position, champion_id
         FROM user_champion_preferences
         WHERE user_id = $1
         ORDER BY CASE lane
                    WHEN 'TOP' THEN 1 WHEN 'JUNGLE' THEN 2 WHEN 'MID' THEN 3
                    WHEN 'BOTTOM' THEN 4 WHEN 'SUPPORT' THEN 5
                  END, position",
    )
    .bind(user_id)
    .fetch_all(&mut *connection)
    .await?;
    let champion_preferences = preferences_from_rows(preference_rows)?;

    let membership_rows = sqlx::query_as::<_, DummyGuildMembershipRow>(
        "SELECT member.guild_id,
                COALESCE(config.display_name, guild.discord_name) AS guild_name,
                member.display_name,
                member.status,
                member.main_lane,
                season.id AS active_season_id
         FROM guild_members AS member
         JOIN guilds AS guild ON guild.guild_id = member.guild_id
         LEFT JOIN guild_config AS config ON config.guild_id = member.guild_id
         LEFT JOIN seasons AS season
           ON season.guild_id = member.guild_id AND season.status = 'ACTIVE'
         WHERE member.user_id = $1
         ORDER BY guild_name, member.guild_id",
    )
    .bind(user_id)
    .fetch_all(&mut *connection)
    .await?;
    let mut guild_memberships = Vec::with_capacity(membership_rows.len());
    for membership in membership_rows {
        let lane_mmr = if let Some(season_id) = membership.active_season_id {
            let mmr_rows = sqlx::query_as::<_, LaneMmrRow>(
                "SELECT lane, mmr FROM user_lane_mmr
                 WHERE guild_id = $1 AND season_id = $2 AND user_id = $3",
            )
            .bind(&membership.guild_id)
            .bind(season_id)
            .bind(user_id)
            .fetch_all(&mut *connection)
            .await?;
            lane_mmr_from_rows(mmr_rows)?
        } else {
            None
        };
        guild_memberships.push(DummyGuildMembership {
            guild_id: membership.guild_id,
            guild_name: membership.guild_name,
            display_name: membership.display_name,
            status: membership.status,
            main_lane: membership.main_lane,
            lane_mmr,
        });
    }

    let group_ids = sqlx::query_scalar::<_, Uuid>(
        "SELECT group_id FROM dummy_user_group_members
         WHERE user_id = $1 ORDER BY group_id",
    )
    .bind(user_id)
    .fetch_all(&mut *connection)
    .await?;

    Ok(DummyUser {
        user_id: row.user_id,
        identity_kind: "DUMMY".to_owned(),
        stable_key: row.stable_key,
        display_name: row.display_name,
        avatar_seed: row.avatar_seed,
        notes: row.notes,
        state: row.state,
        revision: row.revision,
        privacy: DummyPrivacySettings {
            public_profile_enabled: row.public_profile_enabled,
            show_riot_ids: row.show_riot_ids,
            allow_external_record_search: row.allow_external_record_search,
        },
        riot_account,
        champion_preferences,
        guild_memberships,
        group_ids,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

async fn fetch_deletion_impact<'e, E>(
    executor: E,
    user_id: &UserId,
) -> Result<DummyDeletionImpact, DummyUsersRepositoryError>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    let row = sqlx::query_as::<_, DummyDeletionImpactRow>(
        "SELECT
             (SELECT count(*) FROM guild_members WHERE user_id = $1)
                 AS guild_memberships,
             (SELECT count(*) FROM recruitment_participants WHERE user_id = $1)
                 AS recruitment_participants,
             (SELECT count(*) FROM auction_recruitment_participants WHERE user_id = $1)
                 AS auction_participants,
             (SELECT count(*) FROM game_participants WHERE user_id = $1)
                 AS game_participants,
             (SELECT count(*) FROM user_sessions WHERE user_id = $1)
                 AS user_sessions,
             (SELECT count(*) FROM user_discord_identities WHERE user_id = $1)
                 AS discord_identities,
             (SELECT count(*) FROM riot_accounts WHERE user_id = $1 AND origin = 'RIOT')
                 AS real_riot_accounts,
             (SELECT count(*) FROM dummy_user_group_members WHERE user_id = $1)
                 AS group_memberships,
             (SELECT count(*) FROM user_champion_preferences WHERE user_id = $1)
                 AS champion_preferences,
             (SELECT count(*) FROM riot_accounts WHERE user_id = $1 AND origin = 'DUMMY')
                 AS synthetic_riot_accounts",
    )
    .bind(user_id)
    .fetch_one(executor)
    .await?;
    let can_delete = row.guild_memberships == 0
        && row.user_sessions == 0
        && row.discord_identities == 0
        && row.real_riot_accounts == 0;
    Ok(DummyDeletionImpact {
        can_delete,
        blocking_references: DummyDeletionBlockingReferences {
            guild_memberships: row.guild_memberships,
            recruitment_participants: row.recruitment_participants,
            auction_participants: row.auction_participants,
            game_participants: row.game_participants,
            user_sessions: row.user_sessions,
            discord_identities: row.discord_identities,
            real_riot_accounts: row.real_riot_accounts,
        },
        removable_references: DummyDeletionRemovableReferences {
            group_memberships: row.group_memberships,
            champion_preferences: row.champion_preferences,
            synthetic_riot_accounts: row.synthetic_riot_accounts,
        },
    })
}

async fn fetch_group(
    pool: &PgPool,
    group_id: Uuid,
) -> Result<DummyUserGroup, DummyUsersRepositoryError> {
    let mut connection = pool.acquire().await?;
    fetch_group_connection(&mut connection, group_id).await
}

async fn fetch_group_in_transaction(
    transaction: &mut DatabaseTransaction,
    group_id: Uuid,
) -> Result<DummyUserGroup, DummyUsersRepositoryError> {
    fetch_group_connection(&mut *transaction, group_id).await
}

async fn fetch_group_connection(
    connection: &mut PgConnection,
    group_id: Uuid,
) -> Result<DummyUserGroup, DummyUsersRepositoryError> {
    let row = sqlx::query_as::<_, DummyGroupRow>(
        "SELECT id, name, description, revision, created_at, updated_at
         FROM dummy_user_groups WHERE id = $1",
    )
    .bind(group_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(DummyUsersRepositoryError::GroupNotFound)?;
    let members = sqlx::query_as::<_, DummyGroupMemberRow>(
        "SELECT member.user_id, user_record.display_name, member.position
         FROM dummy_user_group_members AS member
         JOIN users AS user_record ON user_record.user_id = member.user_id
         WHERE member.group_id = $1
         ORDER BY member.position",
    )
    .bind(group_id)
    .fetch_all(&mut *connection)
    .await?
    .into_iter()
    .map(DummyGroupMemberRow::into_record)
    .collect::<Vec<_>>();
    Ok(DummyUserGroup {
        id: row.id,
        name: row.name,
        description: row.description,
        revision: row.revision,
        member_count: members.len() as i64,
        members,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

async fn lock_profile(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: &UserId,
) -> Result<LockedDummyProfile, DummyUsersRepositoryError> {
    sqlx::query_as::<_, LockedDummyProfile>(
        "SELECT profile.user_id, user_record.display_name, profile.avatar_seed,
                profile.notes, profile.state, profile.revision
         FROM dummy_user_profiles AS profile
         JOIN users AS user_record
           ON user_record.user_id = profile.user_id
          AND user_record.identity_kind = 'DUMMY'
         WHERE profile.user_id = $1
         FOR UPDATE OF profile, user_record",
    )
    .bind(user_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(DummyUsersRepositoryError::UserNotFound)
}

async fn lock_group(
    transaction: &mut Transaction<'_, Postgres>,
    group_id: Uuid,
) -> Result<LockedDummyGroup, DummyUsersRepositoryError> {
    sqlx::query_as::<_, LockedDummyGroup>(
        "SELECT id, name, description, revision
         FROM dummy_user_groups WHERE id = $1 FOR UPDATE",
    )
    .bind(group_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(DummyUsersRepositoryError::GroupNotFound)
}

async fn upsert_privacy(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: &UserId,
    privacy: &DummyPrivacySettings,
    now: OffsetDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO user_privacy_settings (
             user_id, public_profile_enabled, show_riot_ids,
             allow_external_record_search, updated_at
         ) VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (user_id) DO UPDATE
         SET public_profile_enabled = EXCLUDED.public_profile_enabled,
             show_riot_ids = EXCLUDED.show_riot_ids,
             allow_external_record_search = EXCLUDED.allow_external_record_search,
             updated_at = EXCLUDED.updated_at",
    )
    .bind(user_id)
    .bind(privacy.public_profile_enabled)
    .bind(privacy.show_riot_ids)
    .bind(privacy.allow_external_record_search)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn replace_group_member_rows(
    transaction: &mut Transaction<'_, Postgres>,
    group_id: Uuid,
    member_ids: &[UserId],
    now: OffsetDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM dummy_user_group_members WHERE group_id = $1")
        .bind(group_id)
        .execute(&mut **transaction)
        .await?;
    for (index, member_id) in member_ids.iter().enumerate() {
        sqlx::query(
            "INSERT INTO dummy_user_group_members (group_id, user_id, position, added_at)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(group_id)
        .bind(member_id)
        .bind((index + 1) as i16)
        .bind(now)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

async fn ensure_active_dummy_users(
    transaction: &mut Transaction<'_, Postgres>,
    member_ids: &[UserId],
) -> Result<(), DummyUsersRepositoryError> {
    if member_ids.is_empty() {
        return Ok(());
    }
    let raw_ids = member_ids.iter().map(UserId::as_str).collect::<Vec<_>>();
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM dummy_user_profiles
         WHERE user_id = ANY($1) AND state = 'ACTIVE'",
    )
    .bind(&raw_ids)
    .fetch_one(&mut **transaction)
    .await?;
    if count != member_ids.len() as i64 {
        return Err(DummyUsersRepositoryError::GroupMemberUnavailable);
    }
    Ok(())
}

async fn touch_members_state(
    transaction: &mut Transaction<'_, Postgres>,
    guild_id: &GuildId,
    now: OffsetDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO guild_admin_state (guild_id, scope, revision, updated_by, updated_at)
         VALUES ($1, 'MEMBERS', 1, NULL, $2)
         ON CONFLICT (guild_id, scope) DO UPDATE
         SET revision = guild_admin_state.revision + 1,
             updated_by = NULL,
             updated_at = EXCLUDED.updated_at",
    )
    .bind(guild_id)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn lock_members_state(
    transaction: &mut Transaction<'_, Postgres>,
    guild_id: &GuildId,
    now: OffsetDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO guild_admin_state (guild_id, scope, revision, updated_by, updated_at)
         VALUES ($1, 'MEMBERS', 0, NULL, $2)
         ON CONFLICT (guild_id, scope) DO NOTHING",
    )
    .bind(guild_id)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    sqlx::query_scalar::<_, i64>(
        "SELECT revision
         FROM guild_admin_state
         WHERE guild_id = $1 AND scope = 'MEMBERS'
         FOR UPDATE",
    )
    .bind(guild_id)
    .fetch_one(&mut **transaction)
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_audit(
    transaction: &mut Transaction<'_, Postgres>,
    actor_id: &UserId,
    action: &str,
    target_type: &str,
    target_id: &str,
    request_id: &str,
    before_summary: Option<Value>,
    after_summary: Option<Value>,
    now: OffsetDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO dummy_user_audit_log (
             id, actor_user_id, action, target_type, target_id,
             request_id, before_summary, after_summary, created_at
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(Uuid::now_v7())
    .bind(actor_id)
    .bind(action)
    .bind(target_type)
    .bind(target_id)
    .bind(request_id)
    .bind(before_summary)
    .bind(after_summary)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_guild_audit(
    transaction: &mut Transaction<'_, Postgres>,
    guild_id: &GuildId,
    actor_id: &UserId,
    action: &str,
    target_id: &UserId,
    request_id: &str,
    before_summary: Option<Value>,
    after_summary: Option<Value>,
    now: OffsetDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO admin_audit_log (
             guild_id, id, principal_type, principal_id, action,
             target_type, target_id, checkpoint_id, request_id,
             before_summary, after_summary, result, note, created_at
         ) VALUES (
             $1, $2, 'USER', $3, $4, 'GUILD_MEMBER', $5,
             NULL, $6, $7, $8, 'APPLIED', NULL, $9
         )",
    )
    .bind(guild_id)
    .bind(Uuid::now_v7())
    .bind(actor_id)
    .bind(action)
    .bind(target_id)
    .bind(request_id)
    .bind(before_summary)
    .bind(after_summary)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn preferences_from_rows(
    rows: Vec<ChampionPreferenceRow>,
) -> Result<DummyChampionPreferences, DummyUsersRepositoryError> {
    let mut preferences = DummyChampionPreferences::default();
    let mut expected_positions = HashMap::<Lane, i16>::new();
    for row in rows {
        let expected = expected_positions.entry(row.lane).or_insert(1);
        if row.position != *expected {
            return Err(DummyUsersRepositoryError::StoredInvariantViolation);
        }
        match row.lane {
            Lane::Top => preferences.top.push(row.champion_id),
            Lane::Jungle => preferences.jungle.push(row.champion_id),
            Lane::Mid => preferences.mid.push(row.champion_id),
            Lane::Bottom => preferences.bottom.push(row.champion_id),
            Lane::Support => preferences.support.push(row.champion_id),
        }
        *expected += 1;
    }
    Ok(preferences)
}

fn lane_mmr_from_rows(
    rows: Vec<LaneMmrRow>,
) -> Result<Option<DummyLaneMmr>, DummyUsersRepositoryError> {
    if rows.is_empty() {
        return Ok(None);
    }
    if rows.len() != LANES.len() {
        return Err(DummyUsersRepositoryError::StoredInvariantViolation);
    }
    let mut values = DummyLaneMmr::default();
    let mut seen = HashSet::with_capacity(LANES.len());
    for row in rows {
        if !seen.insert(row.lane) {
            return Err(DummyUsersRepositoryError::StoredInvariantViolation);
        }
        values.set(row.lane, row.mmr);
    }
    Ok(Some(values))
}

fn validate_create(command: &CreateDummyUserCommand) -> Result<(), DummyUsersRepositoryError> {
    if command.stable_key.is_empty()
        || command.stable_key.len() > 64
        || !command.stable_key.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || (index > 0 && matches!(byte, b'_' | b'-'))
        })
    {
        return Err(DummyUsersRepositoryError::InvalidInput { field: "stableKey" });
    }
    validate_text(
        &command.display_name,
        "displayName",
        1,
        MAX_DISPLAY_NAME_CHARS,
    )?;
    validate_optional_text(
        command.avatar_seed.as_deref(),
        "avatarSeed",
        MAX_AVATAR_SEED_CHARS,
        false,
    )?;
    validate_optional_text(command.notes.as_deref(), "notes", MAX_NOTES_CHARS, true)?;
    let mut guild_ids = HashSet::with_capacity(command.guild_memberships.len());
    for membership in &command.guild_memberships {
        validate_membership(&membership.membership)?;
        if !guild_ids.insert(membership.guild_id.as_str()) {
            return Err(DummyUsersRepositoryError::InvalidInput {
                field: "guildMemberships",
            });
        }
    }
    if let Some(riot_account) = &command.riot_account {
        validate_riot_account(riot_account)?;
    }
    Ok(())
}

fn validate_update(command: &UpdateDummyUserCommand) -> Result<(), DummyUsersRepositoryError> {
    if command.expected_revision < 0 {
        return Err(DummyUsersRepositoryError::InvalidExpectedRevision);
    }
    if command.display_name.is_none()
        && command.avatar_seed.is_none()
        && command.notes.is_none()
        && command.privacy.is_none()
    {
        return Err(DummyUsersRepositoryError::InvalidInput { field: "body" });
    }
    if let Some(display_name) = &command.display_name {
        validate_text(display_name, "displayName", 1, MAX_DISPLAY_NAME_CHARS)?;
    }
    if let Some(avatar_seed) = &command.avatar_seed {
        validate_optional_text(
            avatar_seed.as_deref(),
            "avatarSeed",
            MAX_AVATAR_SEED_CHARS,
            false,
        )?;
    }
    if let Some(notes) = &command.notes {
        validate_optional_text(notes.as_deref(), "notes", MAX_NOTES_CHARS, true)?;
    }
    Ok(())
}

fn validate_membership(
    command: &UpsertDummyGuildMembershipCommand,
) -> Result<(), DummyUsersRepositoryError> {
    validate_text(
        &command.display_name,
        "displayName",
        1,
        MAX_DISPLAY_NAME_CHARS,
    )?;
    if let Some(mmr) = &command.lane_mmr
        && [mmr.top, mmr.jungle, mmr.mid, mmr.bottom, mmr.support]
            .into_iter()
            .any(|value| !value.is_finite() || !(0.0..=10_000.0).contains(&value))
    {
        return Err(DummyUsersRepositoryError::InvalidInput { field: "laneMmr" });
    }
    Ok(())
}

fn validate_champion_ids(champion_ids: &[String]) -> Result<(), DummyUsersRepositoryError> {
    if champion_ids.len() > 10 {
        return Err(DummyUsersRepositoryError::InvalidInput {
            field: "championIds",
        });
    }
    let mut seen = HashSet::with_capacity(champion_ids.len());
    if champion_ids.iter().any(|champion_id| {
        champion_id.is_empty()
            || champion_id.len() > 64
            || !champion_id.bytes().all(|byte| byte.is_ascii_alphanumeric())
            || !seen.insert(champion_id.as_str())
    }) {
        return Err(DummyUsersRepositoryError::InvalidInput {
            field: "championIds",
        });
    }
    Ok(())
}

fn validate_riot_account(
    command: &UpsertDummyRiotAccountCommand,
) -> Result<(), DummyUsersRepositoryError> {
    for (value, field, maximum) in [
        (command.platform.as_str(), "platform", 16),
        (command.routing_region.as_str(), "routingRegion", 16),
        (command.game_name.as_str(), "gameName", 64),
        (command.tag_line.as_str(), "tagLine", 16),
    ] {
        validate_text(value, field, 1, maximum)?;
    }
    if command.profile_icon_id.is_some_and(|value| value < 0) {
        return Err(DummyUsersRepositoryError::InvalidInput {
            field: "profileIconId",
        });
    }
    if command.main_position.as_deref().is_some_and(|value| {
        !matches!(
            value,
            "TOP" | "JUNGLE" | "MID" | "BOTTOM" | "SUPPORT" | "FLEX"
        )
    }) {
        return Err(DummyUsersRepositoryError::InvalidInput {
            field: "mainPosition",
        });
    }
    Ok(())
}

fn validate_group(name: &str, description: Option<&str>) -> Result<(), DummyUsersRepositoryError> {
    validate_text(name, "name", 1, MAX_GROUP_NAME_CHARS)?;
    validate_optional_text(
        description,
        "description",
        MAX_GROUP_DESCRIPTION_CHARS,
        true,
    )
}

fn validate_member_ids(member_ids: &[UserId]) -> Result<(), DummyUsersRepositoryError> {
    if member_ids.len() > MAX_GROUP_MEMBERS {
        return Err(DummyUsersRepositoryError::InvalidInput { field: "memberIds" });
    }
    let mut unique = HashSet::with_capacity(member_ids.len());
    if member_ids.iter().any(|id| !unique.insert(id.as_str())) {
        return Err(DummyUsersRepositoryError::InvalidInput { field: "memberIds" });
    }
    Ok(())
}

fn validate_text(
    value: &str,
    field: &'static str,
    minimum: usize,
    maximum: usize,
) -> Result<(), DummyUsersRepositoryError> {
    let count = value.chars().count();
    if value.trim() != value || !(minimum..=maximum).contains(&count) {
        return Err(DummyUsersRepositoryError::InvalidInput { field });
    }
    Ok(())
}

fn validate_optional_text(
    value: Option<&str>,
    field: &'static str,
    maximum: usize,
    allow_empty: bool,
) -> Result<(), DummyUsersRepositoryError> {
    if let Some(value) = value
        && ((!allow_empty && value.is_empty())
            || value.trim() != value
            || value.chars().count() > maximum)
    {
        return Err(DummyUsersRepositoryError::InvalidInput { field });
    }
    Ok(())
}

fn trim_optional(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn ensure_revision(expected: i64, current: i64) -> Result<(), DummyUsersRepositoryError> {
    if expected < 0 {
        return Err(DummyUsersRepositoryError::InvalidExpectedRevision);
    }
    if expected != current {
        return Err(DummyUsersRepositoryError::RevisionConflict { expected, current });
    }
    Ok(())
}

fn request_hash<T: Serialize>(value: &T) -> Result<Vec<u8>, DummyUsersRepositoryError> {
    let canonical = serde_json::to_string(value)?;
    Ok(AuthService::hash_secret(&canonical))
}

async fn acquire_idempotency<T: DeserializeOwned>(
    transaction: &mut DatabaseTransaction,
    scope: &TenantScope,
    actor_id: &UserId,
    method: &str,
    route_template: &str,
    idempotency_key: &str,
    request_hash: &[u8],
) -> Result<IdempotencyDecision<T>, DummyUsersRepositoryError> {
    let now = OffsetDateTime::now_utc();
    let outcome = idempotency::acquire(
        transaction,
        &AcquireIdempotency {
            record_id: IdempotencyRecordId::new(),
            scope,
            principal_type: IdempotencyPrincipalType::User,
            principal_id: actor_id.as_str(),
            method,
            route_template,
            idempotency_key,
            request_hash,
            created_at: now,
            expires_at: now + IDEMPOTENCY_TTL,
        },
    )
    .await?;
    match outcome {
        AcquireIdempotencyOutcome::Acquired { record_id } => {
            Ok(IdempotencyDecision::Acquired(record_id))
        }
        AcquireIdempotencyOutcome::Replay { response, .. } => {
            let body = response
                .body
                .ok_or(DummyUsersRepositoryError::StoredInvariantViolation)?;
            Ok(IdempotencyDecision::Replay(serde_json::from_value(body)?))
        }
        AcquireIdempotencyOutcome::Conflict { .. } => {
            Err(DummyUsersRepositoryError::IdempotencyConflict)
        }
        AcquireIdempotencyOutcome::InProgress => {
            Err(DummyUsersRepositoryError::IdempotencyInProgress)
        }
    }
}

async fn complete_idempotency<T: Serialize>(
    transaction: &mut DatabaseTransaction,
    scope: &TenantScope,
    record_id: IdempotencyRecordId,
    status_code: u16,
    value: &T,
) -> Result<(), DummyUsersRepositoryError> {
    let outcome = idempotency::complete(
        transaction,
        CompleteIdempotency {
            scope,
            record_id,
            response: IdempotentResponse {
                status_code,
                body: Some(serde_json::to_value(value)?),
            },
            completed_at: OffsetDateTime::now_utc(),
        },
    )
    .await?;
    if outcome != CompleteIdempotencyOutcome::Completed {
        return Err(DummyUsersRepositoryError::StoredInvariantViolation);
    }
    Ok(())
}

enum IdempotencyDecision<T> {
    Acquired(IdempotencyRecordId),
    Replay(T),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredDummyUsers {
    users: Vec<DummyUser>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredDummyUser {
    user: DummyUser,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredDummyGroup {
    group: DummyUserGroup,
}

#[derive(Debug, Deserialize, Serialize)]
struct StoredDeleted {
    deleted: bool,
}

fn map_constraint_error(error: sqlx::Error) -> DummyUsersRepositoryError {
    let constraint = match &error {
        sqlx::Error::Database(database) => database.constraint(),
        _ => None,
    };
    match constraint {
        Some("dummy_user_profiles_stable_key_key") => DummyUsersRepositoryError::DuplicateStableKey,
        Some("dummy_user_groups_name_ci_key") => DummyUsersRepositoryError::DuplicateGroupName,
        _ => DummyUsersRepositoryError::Database(error),
    }
}

fn map_membership_delete_error(error: sqlx::Error) -> DummyUsersRepositoryError {
    let is_foreign_key_violation = matches!(
        &error,
        sqlx::Error::Database(database) if database.code().as_deref() == Some("23503")
    );
    if is_foreign_key_violation {
        DummyUsersRepositoryError::MembershipHasHistory
    } else {
        DummyUsersRepositoryError::Database(error)
    }
}

#[derive(Debug, FromRow)]
struct DummyUserRow {
    user_id: UserId,
    display_name: String,
    stable_key: String,
    avatar_seed: Option<String>,
    notes: Option<String>,
    state: DummyUserState,
    revision: i64,
    public_profile_enabled: bool,
    show_riot_ids: bool,
    allow_external_record_search: bool,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

#[derive(Debug, FromRow)]
struct DummyDeletionImpactRow {
    guild_memberships: i64,
    recruitment_participants: i64,
    auction_participants: i64,
    game_participants: i64,
    user_sessions: i64,
    discord_identities: i64,
    real_riot_accounts: i64,
    group_memberships: i64,
    champion_preferences: i64,
    synthetic_riot_accounts: i64,
}

#[derive(Debug, FromRow)]
struct LockedDummyProfile {
    #[allow(dead_code)]
    user_id: UserId,
    display_name: String,
    avatar_seed: Option<String>,
    notes: Option<String>,
    state: DummyUserState,
    revision: i64,
}

#[derive(Debug, FromRow)]
struct DummyRiotAccountRow {
    platform: String,
    routing_region: String,
    game_name: String,
    tag_line: String,
    profile_icon_id: Option<i32>,
    main_position: Option<String>,
}

impl DummyRiotAccountRow {
    fn into_record(self) -> DummyRiotAccount {
        DummyRiotAccount {
            platform: self.platform,
            routing_region: self.routing_region,
            game_name: self.game_name,
            tag_line: self.tag_line,
            profile_icon_id: self.profile_icon_id,
            main_position: self.main_position,
        }
    }
}

#[derive(Debug, FromRow)]
struct BatchDummyRiotAccountRow {
    user_id: UserId,
    platform: String,
    routing_region: String,
    game_name: String,
    tag_line: String,
    profile_icon_id: Option<i32>,
    main_position: Option<String>,
}

#[derive(Debug, FromRow)]
struct ChampionPreferenceRow {
    lane: Lane,
    position: i16,
    champion_id: String,
}

#[derive(Debug, FromRow)]
struct BatchChampionPreferenceRow {
    user_id: UserId,
    lane: Lane,
    position: i16,
    champion_id: String,
}

#[derive(Debug, FromRow)]
struct DummyGuildMembershipRow {
    guild_id: GuildId,
    guild_name: String,
    display_name: String,
    status: GuildMemberStatus,
    main_lane: Option<Lane>,
    active_season_id: Option<Uuid>,
}

#[derive(Debug, FromRow)]
struct BatchDummyGuildMembershipRow {
    user_id: UserId,
    guild_id: GuildId,
    guild_name: String,
    display_name: String,
    status: GuildMemberStatus,
    main_lane: Option<Lane>,
    active_season_id: Option<Uuid>,
}

#[derive(Debug, FromRow)]
struct LaneMmrRow {
    lane: Lane,
    mmr: f64,
}

#[derive(Debug, FromRow)]
struct BatchLaneMmrRow {
    user_id: UserId,
    guild_id: GuildId,
    lane: Lane,
    mmr: f64,
}

#[derive(Debug, FromRow)]
struct BatchGroupMembershipRow {
    user_id: UserId,
    group_id: Uuid,
}

#[derive(Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
struct MembershipBeforeRow {
    display_name: String,
    status: GuildMemberStatus,
    main_lane: Option<Lane>,
}

#[derive(Debug, FromRow)]
struct DummyGroupRow {
    id: Uuid,
    name: String,
    description: Option<String>,
    revision: i64,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

#[derive(Debug, FromRow)]
struct LockedDummyGroup {
    #[allow(dead_code)]
    id: Uuid,
    name: String,
    description: Option<String>,
    revision: i64,
}

#[derive(Debug, FromRow)]
struct DummyGroupMemberRow {
    user_id: UserId,
    display_name: String,
    position: i16,
}

impl DummyGroupMemberRow {
    fn into_record(self) -> DummyGroupMember {
        DummyGroupMember {
            user_id: self.user_id,
            display_name: self.display_name,
            position: self.position,
        }
    }
}

#[derive(Debug, Error)]
pub enum DummyUsersRepositoryError {
    #[error("PostgreSQL dummy users repository operation failed")]
    Database(#[from] sqlx::Error),
    #[error("dummy user was not found")]
    UserNotFound,
    #[error("dummy group was not found")]
    GroupNotFound,
    #[error("guild was not found")]
    GuildNotFound,
    #[error("dummy guild membership was not found")]
    MembershipNotFound,
    #[error("dummy guild membership has retained workflow history")]
    MembershipHasHistory,
    #[error("dummy user deletion is blocked by retained domain references")]
    DeletionBlocked(DummyDeletionImpact),
    #[error("dummy user archive is blocked by joined participants in active recruitments")]
    ArchiveBlockedByActiveRecruitment {
        recruitments: i64,
        auction_recruitments: i64,
    },
    #[error("dummy Riot account was not found")]
    RiotAccountNotFound,
    #[error("archived dummy user cannot be assigned or edited")]
    ArchivedUser,
    #[error("guild has no active season")]
    NoActiveSeason,
    #[error("stable key is already in use")]
    DuplicateStableKey,
    #[error("dummy group name is already in use")]
    DuplicateGroupName,
    #[error("dummy group contains a missing or archived user")]
    GroupMemberUnavailable,
    #[error("invalid dummy user input field: {field}")]
    InvalidInput { field: &'static str },
    #[error("expected revision must be non-negative")]
    InvalidExpectedRevision,
    #[error("revision conflict: expected {expected}, current {current}")]
    RevisionConflict { expected: i64, current: i64 },
    #[error("same idempotency key was used for a different request")]
    IdempotencyConflict,
    #[error("same idempotent request is still in progress")]
    IdempotencyInProgress,
    #[error("dummy user JSON serialization failed")]
    Serialization(#[from] serde_json::Error),
    #[error("dummy user idempotency repository operation failed")]
    Idempotency(#[from] IdempotencyRepositoryError),
    #[error("stored dummy user invariant is violated")]
    StoredInvariantViolation,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_command(stable_key: &str) -> CreateDummyUserCommand {
        CreateDummyUserCommand {
            stable_key: stable_key.to_owned(),
            display_name: "Player 01".to_owned(),
            avatar_seed: None,
            notes: None,
            privacy: DummyPrivacySettings::default(),
            guild_memberships: Vec::new(),
            riot_account: None,
        }
    }

    #[test]
    fn stable_keys_are_portable_and_predictable() {
        assert!(validate_create(&create_command("balanced-top-01")).is_ok());
        for invalid in ["", "Uppercase", "has space", "_prefix"] {
            assert!(matches!(
                validate_create(&create_command(invalid)),
                Err(DummyUsersRepositoryError::InvalidInput { field: "stableKey" })
            ));
        }
    }

    #[test]
    fn privacy_defaults_to_fully_private() {
        assert_eq!(
            DummyPrivacySettings::default(),
            DummyPrivacySettings {
                public_profile_enabled: false,
                show_riot_ids: false,
                allow_external_record_search: false,
            }
        );
    }

    #[test]
    fn lane_mmr_rejects_non_finite_and_out_of_range_values() {
        let command = UpsertDummyGuildMembershipCommand {
            display_name: "Player".to_owned(),
            status: GuildMemberStatus::Active,
            main_lane: Some(Lane::Mid),
            lane_mmr: Some(DummyLaneMmr {
                mid: f64::NAN,
                ..DummyLaneMmr::default()
            }),
        };
        assert!(matches!(
            validate_membership(&command),
            Err(DummyUsersRepositoryError::InvalidInput { field: "laneMmr" })
        ));
    }
}
