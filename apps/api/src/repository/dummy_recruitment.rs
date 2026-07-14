use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{FromRow, PgConnection, PgPool};
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
    domain::{
        DiscordChannelId, DiscordMessageId, GuildId, GuildMemberStatus, IdempotencyRecordId, Lane,
        NotificationId, OutboxEventId, RecruitmentId, RecruitmentStatus, UserId,
    },
    infrastructure::postgres::DatabaseTransaction,
    repository::{
        dummy_users::DummyUserState,
        notification_outbox::{
            self, EnqueueNotification, NotificationOutboxError, RECRUITMENT_MESSAGE_UPDATE_TYPE,
        },
    },
};

const MAX_BATCH_SIZE: usize = 10;
const IDEMPOTENCY_TTL: Duration = Duration::hours(24);
const ADD_METHOD: &str = "POST";
const REMOVE_METHOD: &str = "DELETE";
const ROUTE_TEMPLATE: &str = "/api/dev/guilds/{guildId}/recruitments/{recruitmentId}/participants";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DummyRecruitmentAction {
    Add,
    Remove,
}

impl DummyRecruitmentAction {
    const fn method(self) -> &'static str {
        match self {
            Self::Add => ADD_METHOD,
            Self::Remove => REMOVE_METHOD,
        }
    }

    const fn audit_action(self) -> &'static str {
        match self {
            Self::Add => "DUMMY_RECRUITMENT_PARTICIPANTS_ADDED",
            Self::Remove => "DUMMY_RECRUITMENT_PARTICIPANTS_REMOVED",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, sqlx::Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(type_name = "text", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DummyParticipationState {
    Available,
    Joined,
    Withdrawn,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreviewDummyParticipantsCommand {
    pub action: DummyRecruitmentAction,
    pub user_ids: Vec<UserId>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplyDummyParticipantsCommand {
    pub user_ids: Vec<UserId>,
    pub expected_revision: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DummyRecruitmentSummary {
    pub id: RecruitmentId,
    pub title: String,
    pub status: RecruitmentStatus,
    pub target_count: i16,
    pub joined_count: i64,
    pub remaining_slots: i64,
    pub revision: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DummyRecruitmentCandidate {
    pub user_id: UserId,
    pub display_name: String,
    pub main_lane: Option<Lane>,
    pub profile_state: DummyUserState,
    pub membership_status: GuildMemberStatus,
    pub participation_state: DummyParticipationState,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DummyRecruitmentOverview {
    pub recruitment: DummyRecruitmentSummary,
    pub candidates: Vec<DummyRecruitmentCandidate>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DummyRecruitmentPreviewIssue {
    pub user_id: Option<UserId>,
    pub code: &'static str,
    pub message: &'static str,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DummyRecruitmentPreview {
    pub action: DummyRecruitmentAction,
    pub can_apply: bool,
    pub current_revision: i64,
    pub projected_joined_count: i64,
    pub remaining_slots: i64,
    pub selected: Vec<DummyRecruitmentCandidate>,
    pub issues: Vec<DummyRecruitmentPreviewIssue>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DummyRecruitmentApplyResult {
    pub action: DummyRecruitmentAction,
    pub changed_user_ids: Vec<UserId>,
    pub overview: DummyRecruitmentOverview,
}

pub async fn overview(
    pool: &PgPool,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    actor_id: &UserId,
) -> Result<DummyRecruitmentOverview, DummyRecruitmentError> {
    let mut connection = pool.acquire().await?;
    authorize_operator(&mut connection, guild_id, actor_id).await?;
    fetch_overview(&mut connection, guild_id, recruitment_id).await
}

pub async fn preview(
    pool: &PgPool,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    actor_id: &UserId,
    command: PreviewDummyParticipantsCommand,
) -> Result<DummyRecruitmentPreview, DummyRecruitmentError> {
    let overview = overview(pool, guild_id, recruitment_id, actor_id).await?;
    Ok(build_preview(overview, command.action, &command.user_ids))
}

#[allow(clippy::too_many_arguments)]
pub async fn apply(
    pool: &PgPool,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    actor_id: &UserId,
    idempotency_key: &str,
    request_id: &str,
    action: DummyRecruitmentAction,
    command: ApplyDummyParticipantsCommand,
) -> Result<DummyRecruitmentApplyResult, DummyRecruitmentError> {
    validate_user_ids(&command.user_ids)?;
    if command.expected_revision < 0 {
        return Err(DummyRecruitmentError::InvalidExpectedRevision);
    }
    let request_hash = request_hash(guild_id, recruitment_id, action, &command)?;
    let now = OffsetDateTime::now_utc();
    let scope = TenantScope::guild(guild_id.clone());
    let mut transaction = pool.begin().await?;
    authorize_operator(&mut transaction, guild_id, actor_id).await?;

    let locked = lock_recruitment(&mut transaction, guild_id, recruitment_id).await?;
    let record_id = match idempotency::acquire(
        &mut transaction,
        &AcquireIdempotency {
            record_id: IdempotencyRecordId::new(),
            scope: &scope,
            principal_type: IdempotencyPrincipalType::User,
            principal_id: actor_id.as_str(),
            method: action.method(),
            route_template: ROUTE_TEMPLATE,
            idempotency_key,
            request_hash: &request_hash,
            created_at: now,
            expires_at: now + IDEMPOTENCY_TTL,
        },
    )
    .await?
    {
        AcquireIdempotencyOutcome::Acquired { record_id } => record_id,
        AcquireIdempotencyOutcome::Replay { response, .. } => {
            if response.status_code != 200 {
                return Err(DummyRecruitmentError::StoredInvariantViolation);
            }
            let body = response
                .body
                .ok_or(DummyRecruitmentError::StoredInvariantViolation)?;
            return serde_json::from_value(body)
                .map_err(|_| DummyRecruitmentError::StoredInvariantViolation);
        }
        AcquireIdempotencyOutcome::Conflict { .. } => {
            return Err(DummyRecruitmentError::IdempotencyConflict);
        }
        AcquireIdempotencyOutcome::InProgress => {
            return Err(DummyRecruitmentError::IdempotencyInProgress);
        }
    };

    if locked.revision != command.expected_revision {
        return Err(DummyRecruitmentError::RevisionConflict {
            expected: command.expected_revision,
            current: locked.revision,
        });
    }
    if !participant_changes_allowed(locked.status) {
        return Err(DummyRecruitmentError::RecruitmentNotEditable(locked.status));
    }

    let before = fetch_overview(&mut transaction, guild_id, recruitment_id).await?;
    let selected = selected_candidates(
        &mut transaction,
        guild_id,
        recruitment_id,
        &command.user_ids,
    )
    .await?;
    validate_selected(action, &command.user_ids, &selected)?;

    if action == DummyRecruitmentAction::Add
        && locked.joined_count + i64::try_from(selected.len()).unwrap_or(i64::MAX)
            > i64::from(locked.target_count)
    {
        return Err(DummyRecruitmentError::CapacityExceeded {
            joined: locked.joined_count,
            requested: selected.len(),
            target: locked.target_count,
        });
    }

    for candidate in &selected {
        match action {
            DummyRecruitmentAction::Add => {
                add_participant(&mut transaction, guild_id, recruitment_id, candidate, now).await?;
            }
            DummyRecruitmentAction::Remove => {
                remove_participant(
                    &mut transaction,
                    guild_id,
                    recruitment_id,
                    &candidate.user_id,
                    now,
                )
                .await?;
            }
        }
    }

    bump_recruitment_and_draft(&mut transaction, guild_id, recruitment_id, actor_id, now).await?;
    let after = fetch_overview(&mut transaction, guild_id, recruitment_id).await?;
    let changed_user_ids = selected
        .iter()
        .map(|candidate| candidate.user_id.clone())
        .collect::<Vec<_>>();
    let result = DummyRecruitmentApplyResult {
        action,
        changed_user_ids,
        overview: after.clone(),
    };

    sqlx::query(
        "INSERT INTO admin_audit_log (
             guild_id, id, principal_type, principal_id, action, target_type, target_id,
             checkpoint_id, request_id, before_summary, after_summary, result, note, created_at
         ) VALUES ($1, $2, 'USER', $3, $4, 'RECRUITMENT', $5,
                   NULL, $6, $7, $8, 'APPLIED', NULL, $9)",
    )
    .bind(guild_id)
    .bind(Uuid::now_v7())
    .bind(actor_id)
    .bind(action.audit_action())
    .bind(recruitment_id.to_string())
    .bind(request_id)
    .bind(serde_json::to_value(&before)?)
    .bind(serde_json::to_value(&after)?)
    .bind(now)
    .execute(&mut *transaction)
    .await?;

    let event_id = OutboxEventId::new();
    sqlx::query(
        "INSERT INTO outbox_events (
             id, guild_id, topic, aggregate_type, aggregate_id, aggregate_revision,
             event_type, payload, occurred_at, available_at
         ) VALUES ($1, $2, $3, 'RECRUITMENT', $4, $5,
                   'RECRUITMENT_PARTICIPANTS_CHANGED', $6, $7, $7)",
    )
    .bind(event_id.as_uuid())
    .bind(guild_id)
    .bind(format!("guild:{guild_id}:recruitment:{recruitment_id}"))
    .bind(recruitment_id.to_string())
    .bind(after.recruitment.revision)
    .bind(json!({
        "type": "invalidate",
        "resourceId": recruitment_id,
        "revision": after.recruitment.revision,
    }))
    .bind(now)
    .execute(&mut *transaction)
    .await?;

    enqueue_bound_recruitment_message_update(
        &mut transaction,
        guild_id,
        recruitment_id,
        locked.channel_id.as_ref(),
        locked.message_id.as_ref(),
        after.recruitment.revision,
        now,
    )
    .await?;

    let completed = idempotency::complete(
        &mut transaction,
        CompleteIdempotency {
            scope: &scope,
            record_id,
            response: IdempotentResponse {
                status_code: 200,
                body: Some(serde_json::to_value(&result)?),
            },
            completed_at: now,
        },
    )
    .await?;
    if completed != CompleteIdempotencyOutcome::Completed {
        return Err(DummyRecruitmentError::StoredInvariantViolation);
    }

    transaction.commit().await?;
    Ok(result)
}

fn build_preview(
    overview: DummyRecruitmentOverview,
    action: DummyRecruitmentAction,
    user_ids: &[UserId],
) -> DummyRecruitmentPreview {
    let mut issues = Vec::new();
    let mut seen = HashSet::new();
    let unique_user_ids = user_ids
        .iter()
        .filter(|user_id| seen.insert((*user_id).clone()))
        .cloned()
        .collect::<Vec<_>>();
    if user_ids.is_empty()
        || user_ids.len() > MAX_BATCH_SIZE
        || unique_user_ids.len() != user_ids.len()
    {
        issues.push(issue(
            None,
            "invalid_dummy_participant_selection",
            "더미 참가자는 중복 없이 1명 이상 10명 이하로 선택해야 합니다.",
        ));
    }
    let selected = unique_user_ids
        .iter()
        .filter_map(|user_id| {
            overview
                .candidates
                .iter()
                .find(|candidate| &candidate.user_id == user_id)
                .cloned()
        })
        .collect::<Vec<_>>();

    if !participant_changes_allowed(overview.recruitment.status) {
        issues.push(issue(
            None,
            "recruitment_not_editable",
            "현재 모집 상태에서는 참가자를 변경할 수 없습니다.",
        ));
    }
    for user_id in &unique_user_ids {
        let Some(candidate) = selected
            .iter()
            .find(|candidate| &candidate.user_id == user_id)
        else {
            issues.push(issue(
                Some(user_id.clone()),
                "dummy_user_not_found",
                "이 길드에 배치된 더미 사용자를 찾을 수 없습니다.",
            ));
            continue;
        };
        match action {
            DummyRecruitmentAction::Add => {
                if candidate.profile_state == DummyUserState::Archived {
                    issues.push(issue(
                        Some(user_id.clone()),
                        "dummy_user_archived",
                        "보관된 더미 사용자는 모집에 추가할 수 없습니다.",
                    ));
                } else if candidate.membership_status == GuildMemberStatus::Inactive {
                    issues.push(issue(
                        Some(user_id.clone()),
                        "dummy_membership_inactive",
                        "비활성 길드 회원은 모집에 추가할 수 없습니다.",
                    ));
                } else if candidate.participation_state == DummyParticipationState::Joined {
                    issues.push(issue(
                        Some(user_id.clone()),
                        "already_participant",
                        "이미 모집에 참가 중인 더미 사용자입니다.",
                    ));
                }
            }
            DummyRecruitmentAction::Remove => {
                if candidate.participation_state != DummyParticipationState::Joined {
                    issues.push(issue(
                        Some(user_id.clone()),
                        "not_participant",
                        "현재 모집 참가자가 아닌 더미 사용자입니다.",
                    ));
                }
            }
        }
    }

    let valid_change_count = selected
        .iter()
        .filter(|candidate| match action {
            DummyRecruitmentAction::Add => {
                candidate.profile_state == DummyUserState::Active
                    && candidate.membership_status == GuildMemberStatus::Active
                    && candidate.participation_state != DummyParticipationState::Joined
            }
            DummyRecruitmentAction::Remove => {
                candidate.participation_state == DummyParticipationState::Joined
            }
        })
        .count();
    let projected_joined_count = match action {
        DummyRecruitmentAction::Add => {
            overview.recruitment.joined_count
                + i64::try_from(valid_change_count).unwrap_or(i64::MAX)
        }
        DummyRecruitmentAction::Remove => {
            overview.recruitment.joined_count - i64::try_from(valid_change_count).unwrap_or(0)
        }
    };
    if action == DummyRecruitmentAction::Add
        && projected_joined_count > i64::from(overview.recruitment.target_count)
    {
        issues.push(issue(
            None,
            "recruitment_capacity_exceeded",
            "선택한 사용자를 추가하면 모집 정원을 초과합니다.",
        ));
    }
    let remaining_slots =
        (i64::from(overview.recruitment.target_count) - projected_joined_count).max(0);

    DummyRecruitmentPreview {
        action,
        can_apply: issues.is_empty(),
        current_revision: overview.recruitment.revision,
        projected_joined_count,
        remaining_slots,
        selected,
        issues,
    }
}

fn issue(
    user_id: Option<UserId>,
    code: &'static str,
    message: &'static str,
) -> DummyRecruitmentPreviewIssue {
    DummyRecruitmentPreviewIssue {
        user_id,
        code,
        message,
    }
}

fn participant_changes_allowed(status: RecruitmentStatus) -> bool {
    matches!(status, RecruitmentStatus::Open | RecruitmentStatus::Closed)
}

async fn authorize_operator(
    connection: &mut PgConnection,
    guild_id: &GuildId,
    actor_id: &UserId,
) -> Result<(), DummyRecruitmentError> {
    let authorized = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1
             FROM guild_members AS member
             JOIN guilds AS guild
               ON guild.guild_id = member.guild_id AND guild.status = 'ACTIVE'
             JOIN guild_config AS config ON config.guild_id = member.guild_id
             JOIN guild_member_discord_roles AS member_role
               ON member_role.guild_id = member.guild_id
              AND member_role.user_id = member.user_id
              AND member_role.role_id = config.operator_role_id
             WHERE member.guild_id = $1
               AND member.user_id = $2
               AND member.status = 'ACTIVE'
         )",
    )
    .bind(guild_id)
    .bind(actor_id)
    .fetch_one(connection)
    .await?;
    if authorized {
        Ok(())
    } else {
        Err(DummyRecruitmentError::OperatorRequired)
    }
}

async fn fetch_overview(
    connection: &mut PgConnection,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
) -> Result<DummyRecruitmentOverview, DummyRecruitmentError> {
    let row = sqlx::query_as::<_, RecruitmentOverviewRow>(
        "SELECT recruitment.title,
                recruitment.status,
                recruitment.target_count,
                recruitment.revision,
                count(participant.user_id) FILTER (WHERE participant.status = 'JOINED')
                    AS joined_count
         FROM recruitments AS recruitment
         LEFT JOIN recruitment_participants AS participant
           ON participant.guild_id = recruitment.guild_id
          AND participant.recruitment_id = recruitment.id
         WHERE recruitment.guild_id = $1 AND recruitment.id = $2
         GROUP BY recruitment.guild_id, recruitment.id",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(DummyRecruitmentError::RecruitmentNotFound)?;
    let candidates = sqlx::query_as::<_, CandidateRow>(
        "SELECT profile.user_id,
                member.display_name,
                member.main_lane,
                profile.state AS profile_state,
                member.status AS membership_status,
                CASE participant.status
                    WHEN 'JOINED' THEN 'JOINED'
                    WHEN 'WITHDRAWN' THEN 'WITHDRAWN'
                    ELSE 'AVAILABLE'
                END AS participation_state
         FROM dummy_user_profiles AS profile
         JOIN guild_members AS member
           ON member.user_id = profile.user_id AND member.guild_id = $1
         LEFT JOIN recruitment_participants AS participant
           ON participant.guild_id = member.guild_id
          AND participant.recruitment_id = $2
          AND participant.user_id = member.user_id
         ORDER BY lower(member.display_name), member.user_id",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_all(&mut *connection)
    .await?
    .into_iter()
    .map(CandidateRow::into_candidate)
    .collect();

    Ok(DummyRecruitmentOverview {
        recruitment: DummyRecruitmentSummary {
            id: recruitment_id,
            title: row.title,
            status: row.status,
            target_count: row.target_count,
            joined_count: row.joined_count,
            remaining_slots: (i64::from(row.target_count) - row.joined_count).max(0),
            revision: row.revision,
        },
        candidates,
    })
}

async fn lock_recruitment(
    connection: &mut PgConnection,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
) -> Result<LockedRecruitmentRow, DummyRecruitmentError> {
    sqlx::query_as::<_, LockedRecruitmentRow>(
        "SELECT recruitment.channel_id,
                recruitment.message_id,
                recruitment.status,
                recruitment.target_count,
                recruitment.revision,
                (SELECT count(*)
                 FROM recruitment_participants AS participant
                 WHERE participant.guild_id = recruitment.guild_id
                   AND participant.recruitment_id = recruitment.id
                   AND participant.status = 'JOINED') AS joined_count
         FROM recruitments AS recruitment
         WHERE recruitment.guild_id = $1 AND recruitment.id = $2
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .fetch_optional(connection)
    .await?
    .ok_or(DummyRecruitmentError::RecruitmentNotFound)
}

async fn selected_candidates(
    connection: &mut PgConnection,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    user_ids: &[UserId],
) -> Result<Vec<DummyRecruitmentCandidate>, DummyRecruitmentError> {
    let raw_ids = user_ids
        .iter()
        .map(|user_id| user_id.as_str().to_owned())
        .collect::<Vec<_>>();
    Ok(sqlx::query_as::<_, CandidateRow>(
        "SELECT profile.user_id,
                member.display_name,
                member.main_lane,
                profile.state AS profile_state,
                member.status AS membership_status,
                CASE participant.status
                    WHEN 'JOINED' THEN 'JOINED'
                    WHEN 'WITHDRAWN' THEN 'WITHDRAWN'
                    ELSE 'AVAILABLE'
                END AS participation_state
         FROM dummy_user_profiles AS profile
         JOIN guild_members AS member
           ON member.user_id = profile.user_id AND member.guild_id = $1
         LEFT JOIN recruitment_participants AS participant
           ON participant.guild_id = member.guild_id
          AND participant.recruitment_id = $2
          AND participant.user_id = member.user_id
         WHERE profile.user_id = ANY($3::text[])
         ORDER BY profile.user_id
         FOR UPDATE OF profile, member",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .bind(raw_ids)
    .fetch_all(connection)
    .await?
    .into_iter()
    .map(CandidateRow::into_candidate)
    .collect())
}

fn validate_selected(
    action: DummyRecruitmentAction,
    requested: &[UserId],
    selected: &[DummyRecruitmentCandidate],
) -> Result<(), DummyRecruitmentError> {
    for user_id in requested {
        let candidate = selected
            .iter()
            .find(|candidate| &candidate.user_id == user_id)
            .ok_or_else(|| DummyRecruitmentError::DummyUserUnavailable(user_id.clone()))?;
        match action {
            DummyRecruitmentAction::Add => {
                if candidate.profile_state == DummyUserState::Archived {
                    return Err(DummyRecruitmentError::DummyUserArchived(user_id.clone()));
                }
                if candidate.membership_status == GuildMemberStatus::Inactive {
                    return Err(DummyRecruitmentError::DummyMembershipInactive(
                        user_id.clone(),
                    ));
                }
                if candidate.participation_state == DummyParticipationState::Joined {
                    return Err(DummyRecruitmentError::AlreadyParticipant(user_id.clone()));
                }
            }
            DummyRecruitmentAction::Remove => {
                if candidate.participation_state != DummyParticipationState::Joined {
                    return Err(DummyRecruitmentError::NotParticipant(user_id.clone()));
                }
            }
        }
    }
    Ok(())
}

async fn add_participant(
    connection: &mut PgConnection,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    candidate: &DummyRecruitmentCandidate,
    now: OffsetDateTime,
) -> Result<(), DummyRecruitmentError> {
    sqlx::query(
        "INSERT INTO recruitment_participants (
             guild_id, recruitment_id, user_id, status, joined_at, withdrawn_at
         ) VALUES ($1, $2, $3, 'JOINED', $4, NULL)
         ON CONFLICT (guild_id, recruitment_id, user_id) DO UPDATE
         SET status = 'JOINED', joined_at = EXCLUDED.joined_at, withdrawn_at = NULL",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .bind(&candidate.user_id)
    .bind(now)
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        "DELETE FROM recruitment_preferred_lanes
         WHERE guild_id = $1 AND recruitment_id = $2 AND user_id = $3",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .bind(&candidate.user_id)
    .execute(&mut *connection)
    .await?;
    if let Some(main_lane) = candidate.main_lane {
        sqlx::query(
            "INSERT INTO recruitment_preferred_lanes (
                 guild_id, recruitment_id, user_id, priority, lane
             ) VALUES ($1, $2, $3, 1, $4)",
        )
        .bind(guild_id)
        .bind(recruitment_id.as_uuid())
        .bind(&candidate.user_id)
        .bind(main_lane)
        .execute(connection)
        .await?;
    }
    Ok(())
}

async fn remove_participant(
    connection: &mut PgConnection,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    user_id: &UserId,
    now: OffsetDateTime,
) -> Result<(), DummyRecruitmentError> {
    sqlx::query(
        "DELETE FROM recruitment_entry_assignments
         WHERE guild_id = $1 AND recruitment_id = $2 AND user_id = $3",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .bind(user_id)
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        "DELETE FROM recruitment_preferred_lanes
         WHERE guild_id = $1 AND recruitment_id = $2 AND user_id = $3",
    )
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .bind(user_id)
    .execute(&mut *connection)
    .await?;
    let updated = sqlx::query(
        "UPDATE recruitment_participants
         SET status = 'WITHDRAWN', withdrawn_at = $1
         WHERE guild_id = $2 AND recruitment_id = $3 AND user_id = $4 AND status = 'JOINED'",
    )
    .bind(now)
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .bind(user_id)
    .execute(connection)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(DummyRecruitmentError::NotParticipant(user_id.clone()));
    }
    Ok(())
}

async fn bump_recruitment_and_draft(
    connection: &mut PgConnection,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    actor_id: &UserId,
    now: OffsetDateTime,
) -> Result<(), DummyRecruitmentError> {
    let recruitment = sqlx::query(
        "UPDATE recruitments
         SET revision = revision + 1, updated_by = $1, updated_at = $2
         WHERE guild_id = $3 AND id = $4",
    )
    .bind(actor_id)
    .bind(now)
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .execute(&mut *connection)
    .await?;
    let draft = sqlx::query(
        "UPDATE recruitment_entry_drafts
         SET revision = revision + 1, updated_by = $1, updated_at = $2
         WHERE guild_id = $3 AND recruitment_id = $4",
    )
    .bind(actor_id)
    .bind(now)
    .bind(guild_id)
    .bind(recruitment_id.as_uuid())
    .execute(connection)
    .await?;
    if recruitment.rows_affected() != 1 || draft.rows_affected() != 1 {
        return Err(DummyRecruitmentError::StoredInvariantViolation);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn enqueue_bound_recruitment_message_update(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    channel_id: Option<&DiscordChannelId>,
    message_id: Option<&DiscordMessageId>,
    revision: i64,
    now: OffsetDateTime,
) -> Result<(), DummyRecruitmentError> {
    let (Some(channel_id), Some(_message_id)) = (channel_id, message_id) else {
        return Ok(());
    };
    let payload =
        notification_outbox::recruitment_message_payload(transaction, guild_id, recruitment_id)
            .await
            .map_err(map_notification_outbox_error)?;
    let deduplication_key = format!("recruitment:{recruitment_id}:message:update:{revision}");
    notification_outbox::enqueue(
        transaction,
        &EnqueueNotification {
            id: NotificationId::new(),
            guild_id,
            deduplication_key: &deduplication_key,
            channel_id,
            notification_type: RECRUITMENT_MESSAGE_UPDATE_TYPE,
            payload: &payload,
            created_at: now,
        },
    )
    .await
    .map_err(map_notification_outbox_error)?;
    Ok(())
}

fn map_notification_outbox_error(source: NotificationOutboxError) -> DummyRecruitmentError {
    match source {
        NotificationOutboxError::Sql(source) => DummyRecruitmentError::Database(source),
        source => {
            tracing::error!(%source, "dummy recruitment message update enqueue invariant failed");
            DummyRecruitmentError::StoredInvariantViolation
        }
    }
}

fn validate_user_ids(user_ids: &[UserId]) -> Result<(), DummyRecruitmentError> {
    if user_ids.is_empty() || user_ids.len() > MAX_BATCH_SIZE {
        return Err(DummyRecruitmentError::InvalidSelection);
    }
    let unique = user_ids.iter().collect::<HashSet<_>>();
    if unique.len() != user_ids.len() {
        return Err(DummyRecruitmentError::InvalidSelection);
    }
    Ok(())
}

fn request_hash(
    guild_id: &GuildId,
    recruitment_id: RecruitmentId,
    action: DummyRecruitmentAction,
    command: &ApplyDummyParticipantsCommand,
) -> Result<Vec<u8>, DummyRecruitmentError> {
    let canonical = serde_json::to_string(&(guild_id, recruitment_id, action, command))?;
    Ok(AuthService::hash_secret(&canonical))
}

#[derive(Debug, FromRow)]
struct RecruitmentOverviewRow {
    title: String,
    status: RecruitmentStatus,
    target_count: i16,
    revision: i64,
    joined_count: i64,
}

#[derive(Debug, FromRow)]
struct LockedRecruitmentRow {
    channel_id: Option<DiscordChannelId>,
    message_id: Option<DiscordMessageId>,
    status: RecruitmentStatus,
    target_count: i16,
    revision: i64,
    joined_count: i64,
}

#[derive(Debug, FromRow)]
struct CandidateRow {
    user_id: UserId,
    display_name: String,
    main_lane: Option<Lane>,
    profile_state: DummyUserState,
    membership_status: GuildMemberStatus,
    participation_state: DummyParticipationState,
}

impl CandidateRow {
    fn into_candidate(self) -> DummyRecruitmentCandidate {
        DummyRecruitmentCandidate {
            user_id: self.user_id,
            display_name: self.display_name,
            main_lane: self.main_lane,
            profile_state: self.profile_state,
            membership_status: self.membership_status,
            participation_state: self.participation_state,
        }
    }
}

#[derive(Debug, Error)]
pub enum DummyRecruitmentError {
    #[error("PostgreSQL dummy recruitment operation failed")]
    Database(#[from] sqlx::Error),
    #[error("dummy recruitment JSON operation failed")]
    Serialization(#[from] serde_json::Error),
    #[error("idempotency repository operation failed")]
    Idempotency(#[from] IdempotencyRepositoryError),
    #[error("guild operator permission is required")]
    OperatorRequired,
    #[error("recruitment was not found")]
    RecruitmentNotFound,
    #[error("recruitment participant list is not editable: {0}")]
    RecruitmentNotEditable(RecruitmentStatus),
    #[error("expected revision is invalid")]
    InvalidExpectedRevision,
    #[error("recruitment revision conflict: expected {expected}, current {current}")]
    RevisionConflict { expected: i64, current: i64 },
    #[error("dummy participant selection is invalid")]
    InvalidSelection,
    #[error("dummy user is not available in the guild: {0}")]
    DummyUserUnavailable(UserId),
    #[error("dummy user is archived: {0}")]
    DummyUserArchived(UserId),
    #[error("dummy guild membership is inactive: {0}")]
    DummyMembershipInactive(UserId),
    #[error("dummy user is already a participant: {0}")]
    AlreadyParticipant(UserId),
    #[error("dummy user is not a participant: {0}")]
    NotParticipant(UserId),
    #[error("recruitment capacity exceeded")]
    CapacityExceeded {
        joined: i64,
        requested: usize,
        target: i16,
    },
    #[error("idempotency key was reused with a different request")]
    IdempotencyConflict,
    #[error("the idempotent request is already in progress")]
    IdempotencyInProgress,
    #[error("stored dummy recruitment invariant is violated")]
    StoredInvariantViolation,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(id: &str, state: DummyParticipationState) -> DummyRecruitmentCandidate {
        DummyRecruitmentCandidate {
            user_id: UserId::parse(id).expect("user ID"),
            display_name: id.to_owned(),
            main_lane: Some(Lane::Mid),
            profile_state: DummyUserState::Active,
            membership_status: GuildMemberStatus::Active,
            participation_state: state,
        }
    }

    #[test]
    fn preview_reports_duplicates_and_capacity_without_mutating() {
        let selected = UserId::parse("dmy_01900000-0000-7000-8000-000000000001").unwrap();
        let overview = DummyRecruitmentOverview {
            recruitment: DummyRecruitmentSummary {
                id: RecruitmentId::new(),
                title: "Test".to_owned(),
                status: RecruitmentStatus::Open,
                target_count: 10,
                joined_count: 10,
                remaining_slots: 0,
                revision: 2,
            },
            candidates: vec![candidate(
                selected.as_str(),
                DummyParticipationState::Available,
            )],
        };
        let preview = build_preview(overview, DummyRecruitmentAction::Add, &[selected]);
        assert!(!preview.can_apply);
        assert!(
            preview
                .issues
                .iter()
                .any(|issue| issue.code == "recruitment_capacity_exceeded")
        );
    }
}
